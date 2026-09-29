//! RCC：复位时钟控制单元抽象（GD 命名 RCU）
//!
//! 仅封装端口时钟使能（AHB1EN.PxEN）；系统时钟树配置（PLL/分频）在
//! async 阶段引入，届时按 GD32F470 手册 240MHz 时序实现。

use crate::gpio::Port;
use gd32f470::rcu;

/// RCU 寄存器块借用封装
pub struct Rcc<'a> {
    rb: &'a rcu::RegisterBlock,
}

impl<'a> Rcc<'a> {
    pub fn new(rb: &'a rcu::RegisterBlock) -> Self {
        Self { rb }
    }

    /// 使能 UART6 外设时钟（RCU_APB1EN.UART6EN，PC_RS232_1 接口）
    pub fn enable_uart6(&self) {
        self.rb.apb1en().modify(|_, w| w.uart6en().set_bit());
    }

    /// 使能 USART1 外设时钟（RCU_APB1EN.USART1EN，RS485_1 接口）
    pub fn enable_usart1(&self) {
        self.rb.apb1en().modify(|_, w| w.usart1en().set_bit());
    }

    /// 使能 GPIO 端口时钟（RCU_AHB1EN.PxEN）。
    /// modify() 为读改写，天然满足 GD32 手册"写后读回同步"要求。
    pub fn enable_gpio_port(&self, port: Port) {
        match port {
            Port::A => self.rb.ahb1en().modify(|_, w| w.paen().set_bit()),
            Port::B => self.rb.ahb1en().modify(|_, w| w.pben().set_bit()),
            Port::C => self.rb.ahb1en().modify(|_, w| w.pcen().set_bit()),
            Port::D => self.rb.ahb1en().modify(|_, w| w.pden().set_bit()),
            Port::E => self.rb.ahb1en().modify(|_, w| w.peen().set_bit()),
            Port::F => self.rb.ahb1en().modify(|_, w| w.pfen().set_bit()),
            Port::G => self.rb.ahb1en().modify(|_, w| w.pgen().set_bit()),
            Port::H => self.rb.ahb1en().modify(|_, w| w.phen().set_bit()),
            Port::I => self.rb.ahb1en().modify(|_, w| w.pien().set_bit()),
        }; // svd2rust 0.37 modify() 返回 u32，语句位置丢弃
    }
}
