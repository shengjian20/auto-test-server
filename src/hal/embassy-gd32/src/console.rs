//! 控制台基础设施：println!/print! 宏（UART6 定向）+ 确定性延时 +
//! msh 风格 shell 内核（需求 3/4/5 三合一）
//!
//! 设计（对齐 RT-Thread msh 交互习惯）：
//! - `console::init()`：UART6 初始化 + 全局 CONSOLE 单例（take 后可
//!   多任务共享写——critical-section 串行化）
//! - `println!`/`print!`：经 CONSOLE 写，格式化走 core::fmt（defmt 无关）
//! - `delay_ms/delay_us`：TIMER1 CNT 轮询（确定性，无 O1 空转问题——
//!   教训 #15 的正解），在 time_driver 初始化前调用会 panic
//! - `msh::Shell`：命令表驱动（name/fn 对），prompt + 退格 + 历史留待
//!   上层
#![no_std]
extern crate alloc;

use core::cell::RefCell;
use core::fmt::Write as _;

use critical_section::Mutex;

use crate::usart::Uart;
use crate::{Pin, Port, Rcc};

/// Sync 包裹（PAC 寄存器块含 UnsafeCell 非 Sync；安全性依据：寄存器
/// 访问全 volatile 且写路径经 critical-section 串行化，单端口专用）
struct SyncUart(Uart<'static>);
unsafe impl Send for SyncUart {}
unsafe impl Sync for SyncUart {}

/// 控制台单例（init 后有效；写操作 critical-section 串行化）
static CONSOLE: Mutex<RefCell<Option<SyncUart>>> = Mutex::new(RefCell::new(None));

/// 控制台初始化（UART6 @ PE7/PE8 AF8 115200；RCC 时钟门在内）。
/// 安全性：console::init 全局只调一次（多调则 UART 重复使能，无害但
/// 约定单次）；CONSOLE 单例占用后所有写路径经 critical-section 串行。
pub fn init() {
    let p = crate::periph_steal();
    let rcc = Rcc::new(p.rcu);
    rcc.enable_gpio_port(Port::E);
    rcc.enable_uart6();
    let _utx = Pin::alternate(p.gpioe, 7, 8);
    let _urx = Pin::alternate(p.gpioe, 8, 8);
    let uart = Uart::new(p.uart6);
    uart.enable(16_000_000, 115_200);
    // 切换中断驱动接收（msh 的 read_line 需要非阻塞逐字节）
    crate::usart::uart6_ring_enable(p.uart6);
    critical_section::with(|cs| *CONSOLE.borrow_ref_mut(cs) = Some(SyncUart(uart)));
}


/// 流式输出缓冲（print! 逐段 format 产生的碎片在此攒批，flush 时一次
/// critical-section 写出——降低逐字节锁开销）
static OUT_BUF: Mutex<RefCell<heapless::Vec<u8, 512>>> =
    Mutex::new(RefCell::new(heapless::Vec::new()));

/// 立即写出缓冲（write_bytes 底层；CONSOLE 未初始化时清缓冲静默丢弃）
fn flush_out_buf() {
    critical_section::with(|cs| {
        let mut buf = OUT_BUF.borrow_ref_mut(cs);
        if let Some(sync) = CONSOLE.borrow_ref_mut(cs).as_mut() {
            sync.0.write(&buf[..]);
        }
        buf.clear();
    });
}

/// 写原始字节（流式：攒批进 OUT_BUF，满 512 或显式 flush 落盘；
/// console 未初始化时静默丢弃）
pub fn write_bytes(b: &[u8]) {
    critical_section::with(|cs| {
        let mut buf = OUT_BUF.borrow_ref_mut(cs);
        if CONSOLE.borrow_ref_mut(cs).is_none() {
            buf.clear();
            return;
        }
        for &byte in b {
            if buf.push(byte).is_err() {
                // 满：先落盘再续
                if let Some(sync) = CONSOLE.borrow_ref_mut(cs).as_mut() {
                    sync.0.write(&buf[..]);
                }
                buf.clear();
                let _ = buf.push(byte);
            }
        }
    });
}

/// 显式冲刷流式缓冲（行结束/命令边界处调用；UART 直写无缓冲代价）
pub fn flush() {
    flush_out_buf();
}

/// msh 风格宏出口（core::fmt 对 CONSOLE 的适配）
struct ConsoleWrite;

impl core::fmt::Write for ConsoleWrite {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        write_bytes(s.as_bytes());
        Ok(())
    }
}

/// 控制台打印（println! 语义）
#[macro_export]
macro_rules! println {
    () => {
        $crate::console_write_bytes(b"\r\n")
    };
    ($($arg:tt)*) => {
        $crate::console_write_fmt(format_args!("{}\r\n", format_args!($($arg)*)))
    };
}

/// 控制台打印（print! 语义，无换行）
#[macro_export]
macro_rules! print {
    ($($arg:tt)*) => {
        $crate::console_write_fmt(format_args!($($arg)*))
    };
}

/// 宏底层出口（lib crate 根 re-export 供宏展开解析）
pub fn console_write_fmt(args: core::fmt::Arguments) {
    ConsoleWrite.write_fmt(args).ok();
    // println 结尾 flush（流式缓冲不满 512 也要立即落盘）
    flush_out_buf();
}

pub fn console_write_bytes(b: &[u8]) {
    write_bytes(b);
}

// ---- 确定性延时（需求 5：延时去空跑化）----

/// TIMER1 CNT 基准（time_driver 初始化后 CNT 自由跑 @1MHz = 1 tick/us）。
/// 确定性延时：读 CNT 差值轮询——无 O1 空转问题（教训 #15 的正解），
/// 且 us 级精度（spin_loop 循环的校准漂移彻底消除）
pub fn delay_us(us: u32) {
    let start = crate::time_driver::now_us();
    while crate::time_driver::now_us().wrapping_sub(start) < us as u64 {}
}

pub fn delay_ms(ms: u32) {
    delay_us(ms.saturating_mul(1000));
}

// ---- msh 风格 shell 内核（需求 4）----

/// 命令条目（name/usage/handler；handler 收分词后的参数）
pub struct CmdEntry {
    pub name: &'static str,
    pub usage: &'static str,
    /// 返回响应文本（shell 统一加 \r\n）；args[0] 为命令名自身
    pub handler: fn(args: &[&str]) -> alloc::string::String,
}

/// shell 实例（命令表 + 行缓冲；read_line 非阻塞逐字节消费 RX 环）
pub struct Shell<const MAX_CMDS: usize, const LINE_MAX: usize> {
    cmds: heapless::Vec<CmdEntry, MAX_CMDS>,
    line: heapless::String<LINE_MAX>,
    prompt: heapless::String<32>,
}

impl<const MAX_CMDS: usize, const LINE_MAX: usize> Shell<MAX_CMDS, LINE_MAX> {
    /// 构造（prompt 形如 "msh> "）
    pub fn new(prompt: &str) -> Self {
        let mut p = heapless::String::new();
        let _ = p.push_str(prompt);
        Self {
            cmds: heapless::Vec::new(),
            line: heapless::String::new(),
            prompt: p,
        }
    }

    pub fn add_cmd(&mut self, entry: CmdEntry) {
        let _ = self.cmds.push(entry);
    }

    /// 提示符打印（banner 后/每行回显前）
    pub fn print_prompt(&self) {
        write_bytes(self.prompt.as_bytes());
    }

    /// 非阻塞消费一行（RX 环逐字节；收到 \n/\r 返回 Some(行)，含回显
    /// 与退格处理——RT-Thread msh 交互习惯）
    pub fn poll_line(&mut self) -> Option<alloc::string::String> {
        let mut done = None;
        while let Some(ch) = crate::usart::uart6_ring_pop() {
            match ch {
                b'\n' | b'\r' => {
                    // 回车回显无条件 \r\n（含空行——终端 \n 不回列首，
                    // 缺 \r 则 prompt 出现在屏幕中部）
                    write_bytes(b"\r\n");
                    if !self.line.is_empty() {
                        done = Some(alloc::string::String::from(self.line.as_str()));
                        self.line.clear();
                    }
                    self.print_prompt();
                    // 行结束 flush（响应+prompt 一次性落盘）
                    flush_out_buf();
                }
                0x08 | 0x7F => {
                    // 退格：终端侧擦除（\b \x1b[K）+ 缓冲收缩
                    if self.line.pop().is_some() {
                        write_bytes(b"\x08\x1b[K");
                    }
                }
                0x20..=0x7E => {
                    if self.line.push(ch as char).is_ok() {
                        write_bytes(&[ch]); // 回显
                    }
                }
                _ => {}
            }
        }
        done
    }

    /// 执行一行（分词 -> 命令表匹配 -> handler；未知命令打印 help 提示）
    pub fn exec(&mut self, line: &str) {
        let mut toks: [&str; 8] = [""; 8];
        let mut nt = 0usize;
        for tok in line.split_whitespace() {
            if nt < 8 {
                toks[nt] = tok;
                nt += 1;
            }
        }
        if nt == 0 {
            return;
        }
        for cmd in &self.cmds {
            if cmd.name == toks[0] {
                let resp = (cmd.handler)(&toks[..nt]);
                write_bytes(resp.as_bytes());
                write_bytes(b"\r\n");
                // 响应行 flush
                flush_out_buf();
                return;
            }
        }
        write_bytes(b"unknown cmd, try 'help'\r\n");
        flush_out_buf();
    }

    /// help 命令（命令表自动生成）
    pub fn help_text(&self) -> alloc::string::String {
        let mut s = alloc::string::String::new();
        for cmd in &self.cmds {
            use core::fmt::Write as _;
            let _ = write!(s, "{}: {}\r\n", cmd.name, cmd.usage);
        }
        s
    }
}
