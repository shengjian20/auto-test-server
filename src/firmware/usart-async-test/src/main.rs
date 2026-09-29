//! usart-async-test：阶段 2g 验收——USART6 中断驱动接收 + embassy 异步任务回显
//!
//! 架构：UART6 RXNE 中断（`#[interrupt]` 官方宏）把字节推入 CS 保护的
//! 环形缓冲；异步任务轮询缓冲回显。证明 HAL/usart 与 embassy 栈集成，
//! 且中断接收解决同步轮询版的 CPU 占用问题。
//! 硬件：UART6 TX=PE7 / RX=PE8（AF8），PC 侧 /dev/ttyUSB0 (ATEN)。
#![no_std]
#![no_main]
// 注：本固件含 ISR 与 NVIC unmask 两处 unsafe（下方点级注释依据），
// 无法像轮询固件一样声明模块级 deny(unsafe_code)

use core::cell::RefCell;
use critical_section::Mutex;
use embassy_executor::Spawner;
use embassy_time::{Duration, Ticker};
use embassy_gd32::{Pin, Port, Rcc, Uart};
use gd32f470::Interrupt;
use panic_halt as _;

/// cortex-m-rt #[interrupt] 宏的存在性检查作用域（展开生成
/// interrupt::UART6; 语句，从本 crate 根解析）
#[allow(non_snake_case)]
pub mod interrupt {
    pub use gd32f470::Interrupt::*;
}

/// 中断接收环（CS 保护；RXNE ISR 生产，异步任务消费）
static RX_RING: Mutex<RefCell<Ring>> = Mutex::new(RefCell::new(Ring::new()));

struct Ring {
    buf: [u8; 256],
    head: usize,
    tail: usize,
}

impl Ring {
    const fn new() -> Self {
        Self { buf: [0; 256], head: 0, tail: 0 }
    }
    /// ISR 生产侧（CS 内调用）
    fn push(&mut self, b: u8) {
        let next = (self.head + 1) % self.buf.len();
        if next != self.tail {
            self.buf[self.head] = b;
            self.head = next;
        }
        // 满：丢弃最新字节（ORE 同效）
    }
    /// 任务消费侧（CS 内调用）
    fn pop(&mut self) -> Option<u8> {
        if self.tail == self.head {
            return None;
        }
        let b = self.buf[self.tail];
        self.tail = (self.tail + 1) % self.buf.len();
        Some(b)
    }
}

/// UART6 中断：读 DATA（清 RBNE），推入接收环。
///
/// unsafe 依据（steal）：UART6 外设仅被本 ISR 与 main 的初始化访问；
/// steal 单次语义由 main 先 take 后本函数才可能被触发保证。
#[cortex_m_rt::interrupt]
#[allow(unsafe_code)]
unsafe fn UART6() {
    // 读 STAT0 后读 DATA（手册推荐流），错误字节丢弃
    let uart = &unsafe { gd32f470::Peripherals::steal() }.uart6;
    let st = uart.stat0().read();
    if st.rbne().bit_is_set() {
        let b = uart.data().read().data().bits() as u8;
        critical_section::with(|cs| RX_RING.borrow(cs).borrow_mut().push(b));
    } else if st.orerr().bit_is_set() {
        // ORE 清除：读 STAT0 已做，读 DATA 收尾
        let _ = uart.data().read();
    }
}

#[embassy_executor::main]
async fn main(_spawner: Spawner) {
    let p = gd32f470::Peripherals::take().expect("peripherals already taken");

    let rcc = Rcc::new(&p.rcu);
    rcc.enable_gpio_port(Port::E);
    rcc.enable_uart6();

    let _tx = Pin::alternate(&p.gpioe, 7, 8);
    let _rx = Pin::alternate(&p.gpioe, 8, 8);

    let uart = Uart::new(&p.uart6);
    uart.enable(16_000_000, 115_200);
    // 使能 RXNE 中断（RBNEIE）
    p.uart6.ctl0().modify(|_, w| w.rbneie().set_bit());
    // unsafe 依据：UART6 vector 符号由本文件 #[interrupt] 函数独占接线；
    // unmask 只打开该中断线（time_driver 同款模式）
    // unsafe 依据：UART6 vector 由本文件 #[interrupt] 函数独占接线
    unsafe {
        cortex_m::peripheral::NVIC::unmask(Interrupt::UART6);
    }

    let mut tx = Uart::new(&p.uart6);
    tx.write(b"usart-async ready\r\n");

    let mut ticker = Ticker::every(Duration::from_millis(50));
    loop {
        ticker.next().await;
        // 异步任务消费接收环：回显全部已收字节
        loop {
            let b = critical_section::with(|cs| RX_RING.borrow(cs).borrow_mut().pop());
            match b {
                Some(b) => tx.write_byte(b),
                None => break,
            }
        }
    }
}
