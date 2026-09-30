//! ariel-os-gd32：Ariel OS 的 GD32F470 HAL family（官方契约形态）
//!
//! 按 ariel-os-stm32 的 13 项导出契约逐项对接（官方清单 §3.2 表），
//! 底层实现复用本仓库已板上验证的 embassy-gd32 模块（gpio/spi/usart/
//! enet/timer/console——13/13 E2E + MSH_E2E 等验收）。
//!
//! 契约项状态：
//! - peripheral/identity/init：本文件（第一批）
//! - gpio/uart：模块占位，逐步对接 embassy-gd32
//! - extint_registry/spi/i2c/ethernet/hwrng/storage/usb：后续批次
#![no_std]

pub mod gpio;
pub mod identity;
pub mod periph;
pub mod peripheral {
    //! PAC 外设句柄集（'static 引用形态，zst 语义同 HAL periph 模块）
    pub use crate::periph::Peripherals;
}
pub mod uart;

/// embassy-executor 中断执行器（executor-interrupt laze 模块要求；
/// SWI 软中断机制见 build.rs 的 swi.rs 生成——后续批次接线）
#[doc(hidden)]
pub static EXECUTOR: embassy_executor::InterruptExecutor = embassy_executor::InterruptExecutor::new();

pub use periph::steal as init;

/// 外设初始化（官方 init() 契约：返回 OptionalPeripherals——GD32 形态
/// 直接返回句柄集，Optional 语义由调用方按需取用）
pub fn init_detail() -> periph::Peripherals {
    periph::steal()
}
