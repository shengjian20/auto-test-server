//! GPIO（官方契约：gpio 模块——SBD 声明的引脚映射 + Output/Input 抽象）
//!
//! 官方形态对照 ariel-os-stm32/src/gpio.rs：按 SBD 生成的引脚常量 +
//! Output/Input 驱动类型。GD32 形态：复用 embassy-gd32::gpio::Pin
//! （板上验证的 BOP/BC/TG 寄存器级实现），按官方命名重新导出。
//!
//! 第一批骨架：引脚类型别名 + Output/Input 包装。SBD 生成接线后续
//! 批次（需要 laze/sbd-gen 工具链就位）。

use crate::periph;
use embassy_gd32::Port;

/// 输出引脚（官方契约：Output 驱动）
pub struct Output {
    inner: embassy_gd32::Pin<'static>,
}

impl Output {
    /// 构造输出引脚（port/n 与板上验证的 BOP/BC 路径一致）
    pub fn new(port: Port, n: u8) -> Self {
        let p = periph::steal();
        let inner = match port {
            Port::A => embassy_gd32::Pin::output(p.gpio_a(), n),
            Port::B => embassy_gd32::Pin::output(p.gpio_b(), n),
            Port::C => embassy_gd32::Pin::output(p.gpio_c(), n),
            Port::D => embassy_gd32::Pin::output(p.gpio_d(), n),
            Port::E => embassy_gd32::Pin::output(p.gpio_e(), n),
            // GD32F470VGT6 LQFP100 仅引出 A-E（F-I 编译期存在、板上无引脚）
            _ => panic!("port not available on GD32F470VGT6 LQFP100"),
        };
        Self { inner }
    }

    pub fn set_high(&mut self) {
        self.inner.set_high();
    }

    pub fn set_low(&mut self) {
        self.inner.set_low();
    }
}

/// 输入引脚（官方契约：Input 驱动）
pub struct Input {
    inner: embassy_gd32::Pin<'static>,
}

impl Input {
    pub fn new(port: Port, n: u8) -> Self {
        let p = periph::steal();
        let inner = match port {
            Port::A => embassy_gd32::Pin::input(p.gpio_a(), n),
            Port::B => embassy_gd32::Pin::input(p.gpio_b(), n),
            Port::C => embassy_gd32::Pin::input(p.gpio_c(), n),
            Port::D => embassy_gd32::Pin::input(p.gpio_d(), n),
            Port::E => embassy_gd32::Pin::input(p.gpio_e(), n),
            _ => panic!("port not available on GD32F470VGT6 LQFP100"),
        };
        Self { inner }
    }

    pub fn level(&self) -> bool {
        self.inner.input_level()
    }
}
