//! embassy-gd32：GD32F470 的 Embassy 风格 HAL（骨架：gpio + rcc 同步 API）
//!
//! 设计原则（项目约束：最少 unsafe）：
//! - 公共 API 全安全；寄存器访问只走 PAC 字段级安全方法
//!   （`write()/modify()` + 具名字段 writer）。svd2rust 0.37 的整寄存器
//!   `W::bits()` 为 unsafe，本 crate 不使用
//! - PAC 实例所有权即外设独占证明：`Peripherals::take()` 的
//!   critical-section 单次语义保证实例唯一；Pin 仅借用寄存器块
//! - 引脚号运行期存储 + match 分发表（PAC 字段 writer 按引脚号命名，
//!   无 const 泛型索引手段；类型级引脚唯一性留待 async 阶段设计）
#![no_std]

pub mod can;
pub mod cpld;
pub mod enet;
pub mod flash;
pub mod gpio;
pub mod interrupt;
pub mod rcc;
pub mod spi;
pub mod time_driver;
pub mod usart;
pub mod w25q;

pub use time_driver::init_time_driver;

pub use gpio::{Pin, PinMode, Port};
pub use can::Can;
pub use spi::Spi;
pub use usart::{Uart, Usart};
pub use rcc::Rcc;
