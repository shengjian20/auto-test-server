//! blink-hal：embassy-gd32 同步 GPIO API 验证固件
//!
//! 硬件事实：LED_1 = PD2 低电平点亮；主 SRAM 128KB @ 0x20000000（DFP 定义）
#![no_std]
#![no_main]
#![deny(unsafe_code)]

use core::hint::black_box;
use embassy_gd32::{Pin, Port, Rcc};
use panic_halt as _;

#[cortex_m_rt::entry]
fn main() -> ! {
    let p = gd32f470::Peripherals::take().expect("peripherals already taken");

    let rcc = Rcc::new(&p.rcu);
    rcc.enable_gpio_port(Port::D);

    let mut led = Pin::output(&p.gpiod, 2);

    loop {
        led.toggle();
        delay(1_000_000);
    }
}

/// 忙等延时（默认 HSI 16MHz）。black_box 防优化删除。
fn delay(n: u32) {
    for i in 0..n {
        black_box(i);
    }
}
