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
pub mod console;
pub mod cpld;
pub mod crc32;
pub mod enet;
pub mod flash;
pub mod enet_dma;
#[cfg(feature = "smoltcp-device")]
pub mod enet_smoltcp;
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

/// PAC 外设句柄集的安全获取（Peripherals::take 的绕行——跳转链/二次
/// 初始化场景下 take() 因 DEVICE_PERIPHERALS 标志返回 None；字段为
/// 'static 寄存器块引用，子句柄自动获得 'static）。
/// unsafe 收敛点：外设地址为 SVD 定案物理常驻，引用永不悬垂；调用方
/// 需自行保证无别名写（console::init 仅写 UART6 寄存器）
#[allow(unsafe_code)]
pub fn periph_steal() -> periph::Peripherals {
    periph::steal()
}

pub mod periph;

pub use console::{console_write_bytes, console_write_fmt, delay_ms, delay_us};
