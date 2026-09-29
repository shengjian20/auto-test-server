//! app-at-offset：阶段 5 验收应用（确定性 delay 版——volatile 递减计数，
//! 编译器无法优化；排除 spin_loop 被优化导致的 delay 失效假设）
//! LED_1=PD2 低有效；bootloader 检查后跳入。
#![no_std]
#![no_main]

use embassy_gd32::{Pin, Port, Rcc};
use panic_halt as _;

/// 确定性忙等（volatile 递减，不可被优化）
fn delay_ms(ms: u32) {
    for _ in 0..ms {
        let mut c = 4000u32;
        while c > 0 {
            core::hint::black_box(c);
            c -= 1;
        }
    }
}

#[cortex_m_rt::entry]
fn main() -> ! {
    let p = gd32f470::Peripherals::take().expect("peripherals already taken");

    let rcc = Rcc::new(&p.rcu);
    rcc.enable_gpio_port(Port::D);

    // LED_1 = PD2（低有效）
    let mut led = Pin::output(&p.gpiod, 2);

    loop {
        led.set_low();
        delay_ms(900);
        led.set_high();
        delay_ms(900);
    }
}
