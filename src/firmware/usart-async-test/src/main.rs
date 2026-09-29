//! usart-async-test：阶段 2g 验收——USART6 中断驱动接收 + embassy 异步任务
//!
//! 架构（HAL 独占 UART6 向量，固件零 unsafe）：
//! - HAL usart.rs：RXNE ISR -> CS 接收环（uart6_ring_enable 接管接收）
//! - 本任务：50ms Ticker 消费环回显（中断环撑住连续字节流）
//! 事实：UART6 TX=PE7 / RX=PE8（AF8）；console=/dev/ttyUSB0 (ATEN)
#![no_std]
#![no_main]

use embassy_executor::Spawner;
use embassy_time::{Duration, Ticker};
use embassy_gd32::{Pin, Port, Rcc, Uart};
use panic_halt as _;

#[embassy_executor::main]
async fn main(_spawner: Spawner) {
    let p = gd32f470::Peripherals::take().expect("peripherals already taken");

    // 时间驱动初始化（TIMER1@1MHz）——缺失则 Ticker 闹钟永不触发，
    // 任务卡死在首次 await（此前踩坑）
    embassy_gd32::init_time_driver();

    let rcc = Rcc::new(&p.rcu);
    rcc.enable_gpio_port(Port::E);
    rcc.enable_uart6();

    let _tx = Pin::alternate(&p.gpioe, 7, 8);
    let _rx = Pin::alternate(&p.gpioe, 8, 8);

    let mut uart = Uart::new(&p.uart6);
    uart.enable(16_000_000, 115_200);
    uart.write(b"usart-async ready\r\n");

    // 接管接收：RBNEIE + NVIC（HAL 内收敛，环由 HAL 持有）
    embassy_gd32::usart::uart6_ring_enable(&p.uart6);

    let mut ticker = Ticker::every(Duration::from_millis(50));
    loop {
        ticker.next().await;
        // 消费接收环：回显全部已收字节
        loop {
            match embassy_gd32::usart::uart6_ring_pop() {
                Some(b) => uart.write_byte(b),
                None => break,
            }
        }
    }
}
