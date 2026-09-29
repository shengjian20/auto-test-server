//! usart-async-test：USART6 中断驱动接收 + embassy 异步任务（诊断版）
//!
//! 自报告设计（不依赖调试链路）：
//! - TX 心跳：每 500ms 发 'A'（Ticker 路径存活证据）
//! - RX 探针：收到任意字节回显之（ISR→环→任务→TX 全链路证据）
//! - ISR 计数：静态计数器，PC 侧经 openocd 读取（可选）
#![no_std]
#![no_main]

use core::cell::RefCell;
use core::sync::atomic::{AtomicU32, Ordering};
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

/// ISR 命中计数（openocd 可读，诊断用）
static ISR_HITS: AtomicU32 = AtomicU32::new(0);

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
    fn push(&mut self, b: u8) {
        let next = (self.head + 1) % self.buf.len();
        if next != self.tail {
            self.buf[self.head] = b;
            self.head = next;
        }
    }
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
/// unsafe 依据（steal）：UART6 仅被本 ISR 与 main 初始化访问（main 先
/// take 建立外设，ISR 触发时外设已就绪）。
#[cortex_m_rt::interrupt]
#[allow(unsafe_code)]
unsafe fn UART6() {
    ISR_HITS.fetch_add(1, Ordering::Relaxed);
    let uart = &unsafe { gd32f470::Peripherals::steal() }.uart6;
    let st = uart.stat0().read();
    if st.rbne().bit_is_set() {
        let b = uart.data().read().data().bits() as u8;
        critical_section::with(|cs| RX_RING.borrow(cs).borrow_mut().push(b));
    } else if st.orerr().bit_is_set() {
        let _ = uart.data().read();
    }
}

#[embassy_executor::main]
async fn main(_spawner: Spawner) {
    let p = gd32f470::Peripherals::take().expect("peripherals already taken");

    // 时间驱动初始化（TIMER1@1MHz）——缺失则 Ticker 闹钟永不触发，
    // 任务卡死在首次 await（banner 正常但心跳/回显全无的根因）
    embassy_gd32::init_time_driver();

    let rcc = Rcc::new(&p.rcu);
    rcc.enable_gpio_port(Port::E);
    rcc.enable_uart6();

    let _tx = Pin::alternate(&p.gpioe, 7, 8);
    let _rx = Pin::alternate(&p.gpioe, 8, 8);

    let uart = Uart::new(&p.uart6);
    uart.enable(16_000_000, 115_200);
    uart.write(b"usart-async ready\r\n");

    // RXNE 中断使能 + NVIC
    p.uart6.ctl0().modify(|_, w| w.rbneie().set_bit());
    // unsafe 依据：UART6 vector 由本文件 #[interrupt] 函数独占接线
    unsafe {
        cortex_m::peripheral::NVIC::unmask(Interrupt::UART6);
    }

    let mut tx = Uart::new(&p.uart6);
    let mut ticker = Ticker::every(Duration::from_millis(500));
    loop {
        ticker.next().await;
        // 心跳（Ticker 路径存活证据）
        tx.write_byte(b'A');
        // 消费接收环：回显全部已收字节
        loop {
            let b = critical_section::with(|cs| RX_RING.borrow(cs).borrow_mut().pop());
            match b {
                Some(b) => tx.write_byte(b),
                None => break,
            }
        }
    }
}
