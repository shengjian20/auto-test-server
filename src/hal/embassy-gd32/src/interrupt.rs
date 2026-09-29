//! 中断基础设施：typelevel 中断 + Handler trait + bind_interrupts! 宏
//!
//! 架构照搬 embassy-stm32 / embassy-hal-internal：
//! - `interrupt::typelevel::*`：每中断一个零大小类型，`IRQ` 常量回链 PAC 枚举
//! - `Handler<I>`：driver 实现的中断处理 trait；`Binding<I, H>` 编译期断言
//!   "该中断已绑定该 handler"——驱动可以把它作为参数约束，杜绝漏绑
//! - `bind_interrupts!`：用户侧声明式绑定，展开为 vector 符号 + Binding impl
//!
//! 与裸 `#[no_mangle] extern "C" fn TIMER1()` 的差异：结构化绑定 +
//! 编译期检查；vector 符号生成本质相同（embassy-stm32 同样是
//! `#[no_mangle] unsafe extern "C" fn $irq()`，见其 lib.rs bind_interrupts! 展开）。

use cortex_m::peripheral::NVIC;
use cortex_m::interrupt::InterruptNumber;

/// 重导出 PAC 中断枚举（VECTOR 表来源：PAC rt feature 的 device.x）
pub use gd32f470::Interrupt;

/// cortex-m-rt `#[interrupt]` 宏的存在性检查作用域（宏展开生成
/// `interrupt::TIMER1;` 裸路径语句，从 crate 根解析）。
/// svd2rust 0.37 在 CortexM target 下不再生成 `pub mod interrupt`，
/// 此处从 PAC 根层 Interrupt 枚举映射变体名。
// 以下 glob 使 `hal::interrupt::TIMER1` 等变体路径可用，
// 供 cortex-m-rt #[interrupt] 宏展开的存在性检查语句解析
pub use gd32f470::Interrupt::*;

/// 中断使能/关断扩展（对 PAC 枚举的安全封装层，unmask 本身 unsafe，
/// 此处收敛为带 compiler_fence 的规范次序，语义同 embassy-hal-internal）
pub trait InterruptExt: InterruptNumber + Copy {
    /// 使能中断（unsafe：使能前必须已注册对应 handler，见 time_driver 模块注释）
    unsafe fn enable(self) {
        cortex_m::asm::delay(8);
        NVIC::unmask(self)
    }

    /// 关闭中断（安全）
    fn disable(self) {
        NVIC::mask(self);
    }
}

impl InterruptExt for Interrupt {}

/// typelevel 中断类型生成：每中断一个零大小类型
macro_rules! typelevel_interrupts {
    ($($name:ident),* $(,)?) => {
        pub mod typelevel {
            /// Sealed 封锁 trait（模块外不可命名，阻止外部实现 Irq）
            pub trait Sealed {}
            $(impl Sealed for $name {})*

            /// Type-level interrupt：零大小类型，IRQ 回链 PAC 枚举
            #[allow(non_camel_case_types)]
            pub trait Irq: Sealed + Copy {
                const IRQ: super::Interrupt;
            }

            $(
                #[allow(non_camel_case_types)]
                #[derive(Copy, Clone)]
                pub enum $name {}
                impl Irq for $name {
                    const IRQ: super::Interrupt = super::Interrupt::$name;
                }
            )*

            /// driver 实现的中断处理 trait（在 interrupt 上下文被调用）
            ///
            /// # Safety
            /// 只能由 `I` 中断的处理函数同步调用
            pub unsafe trait Handler<I: Irq> {
                /// 中断触发时同步调用
                unsafe fn on_interrupt();
            }

            /// 编译期断言：中断 I 已绑定 handler H
            ///
            /// # Safety
            /// 实现即承诺：I 触发时 H::on_interrupt() 会被调用
            pub unsafe trait Binding<I: Irq, H: Handler<I>> {}
        }
    };
}

// 本 HAL 需要的 typelevel 中断按需展开（全表太长，无必要）
typelevel_interrupts!(TIMER1);

/// 声明式中断绑定（用法同 embassy-stm32）：
///
/// ```ignore
/// embassy_gd32::bind_interrupts!(struct Irqs {
///     TIMER1 => embassy_gd32::time_driver::Timer1Handler;
/// });
/// ```
#[macro_export]
macro_rules! bind_interrupts {
    ($vis:vis struct $name:ident {
        $($irq:ident => $handler:ty),* $(,)?
    }) => {
        #[derive(Copy, Clone)]
        #[allow(non_snake_case)]
        $vis struct $name;

        $(
            // vector 表符号：PAC device.x 的弱默认符号由此覆盖。
            // 本质与 embassy-stm32 相同的 #[no_mangle] extern "C"，
            // 但经由宏统一生成并伴随 Binding 编译期断言。
            #[allow(non_snake_case)]
            #[unsafe(no_mangle)]
            unsafe extern "C" fn $irq() {
                unsafe {
                    <$handler as $crate::interrupt::typelevel::Handler<$crate::interrupt::typelevel::$irq>>::on_interrupt();
                }
            }

            unsafe impl $crate::interrupt::typelevel::Binding<
                $crate::interrupt::typelevel::$irq,
                $handler,
            > for $name {}
        )*
    };
}
