//! blink-async：embassy 异步栈验证固件
//!
//! 验证链：TIMER1 time driver（链接期注册）→ embassy-time Timer →
//! executor-thread 任务调度 → HAL gpio toggle
//! 硬件事实：LED_1 = PD2 低电平点亮；主 SRAM 128KB @ 0x20000000
#![no_std]
#![no_main]
#![deny(unsafe_code)]

use embassy_executor::Spawner;
use embassy_gd32::{Pin, Port, Rcc};
use embassy_time::{Duration, Ticker};
use panic_halt as _;

#[embassy_executor::main]
async fn main(_spawner: Spawner) {
    let p = gd32f470::Peripherals::take().expect("peripherals already taken");

    embassy_gd32::init_time_driver();

    let rcc = Rcc::new(&p.rcu);
    rcc.enable_gpio_port(Port::D);

    let mut led = Pin::output(&p.gpiod, 2);

    let mut ticker = Ticker::every(Duration::from_millis(375));
    loop {
        led.toggle();
        ticker.next().await;
    }
}
