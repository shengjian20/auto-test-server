//! UART（官方契约：define_uart_drivers! 宏——SBD 声明的每路 UART 生成
//! 单例驱动，embedded-io traits 形态）
//!
//! GD32 形态：宏展开的结构按官方模板（单例防重静态 + Active 标志），
//! 底层读写复用 embassy-gd32::usart 的 UART6 中断环（板上验证形态）。
//! 第一批骨架：UART6 一路的宏展开示例；SBD 接线后续批次。

use embassy_gd32::usart;

/// 官方契约宏（形态对照 ariel-os-stm32/src/uart.rs::define_uart_drivers!）：
/// 每个 SBD 声明的 UART 生成一个单例驱动结构，实现 embedded-io
/// Read/Write traits。
///
/// GD32 第一批：Uart6 一路（板上验证的 console/协议通道）
macro_rules! define_uart_drivers {
    ($( $interrupt:ident => $peripheral:ident ),* $(,)?) => {
        $(
            paste::paste! {
                #[allow(dead_code)]
                static [<PREVENT_MULTIPLE_ $peripheral>]: () = ();
            }

            /// GD32 UART 单例驱动（官方契约：embedded-io traits）
            pub struct $peripheral {
                _private: (),
            }

            impl $peripheral {
                /// 构造（单例：宏静态保证全局唯一；首次调用完成
                /// UART6 时钟门+引脚+RXNE 环初始化）
                pub fn new() -> Self {
                    embassy_gd32::usart::uart6_ring_enable_dyn();
                    Self { _private: () }
                }

                /// 写字节流（embedded-io::Write 语义：轮询 TX，逐字节
                /// 等待 TBE——与板上验证的 Uart::write 同一路径）
                pub fn write_bytes(&self, buf: &[u8]) {
                    let p = crate::periph::steal();
                    let uart = usart::Uart::new(p.uart6);
                    uart.write(buf);
                }

                /// 读单字节（非阻塞；RXNE 环形态下由 msh 上层驱动
                /// poll——embassy async 化在后续批次）
                pub fn read_byte(&self) -> Option<u8> {
                    let p = crate::periph::steal();
                    let uart = usart::Uart::new(p.uart6);
                    uart.read_byte()
                }
            }
        )*
    };
}

define_uart_drivers! {
    USART6 => Uart6Driver,
}
