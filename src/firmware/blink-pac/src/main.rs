//! blink-pac：阶段1 PAC 验证固件——阶段0 的手写寄存器 blink 迁移到 svd2rust 安全 API
//!
//! 硬件事实（实测+原理图+用户核对）：
//! - LED_1 = PD2，低电平点亮（510R 限流灌电流）
//! - RCU @ 0x40023800（AHB1_BUS_BASE+0x3800），AHB1EN bit3 = PDEN
//! - 主 SRAM 128KB @ 0x20000000（DFP 定义，512KB 连续是错误假设，栈越界即 HardFault）
//!
//! 约束：#![deny(unsafe_code)]——PAC 安全 API（read/write/modify/set_bit）全程，
//! cortex-m-rt 的 entry 宏内部 unsafe 与本 crate 无涉。
#![no_std]
#![no_main]
#![deny(unsafe_code)]

use core::hint::black_box;
use panic_halt as _;

#[cortex_m_rt::entry]
fn main() -> ! {
    let p = gd32f470::Peripherals::take().expect("peripherals already taken");

    // GPIOD 时钟使能：RCU_AHB1EN.PDEN（bit3）
    p.rcu.ahb1en().modify(|_, w| w.pden().set_bit());

    let gpiod = &p.gpiod;

    // PD2 输出模式：CTL.CTL2 = 01（GD 命名，CTLx 两位一组；复位默认输入 00）
    gpiod.ctl().modify(|_, w| w.ctl2().output());

    // 速度/上下拉保持复位默认（低速、浮空）——低速对 blink 足够

    loop {
        // TG（bit toggle register）：写 1 翻转，免读改写
        gpiod.tg().write(|w| w.tg2().set_bit());
        delay(1_000_000);
    }
}

/// 忙等延时（默认 HSI 16MHz）。black_box 防优化删除。
fn delay(n: u32) {
    for i in 0..n {
        black_box(i);
    }
}
