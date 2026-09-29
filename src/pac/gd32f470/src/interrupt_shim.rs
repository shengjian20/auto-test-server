//! svd2rust 0.37 兼容 shim：cortex-m-rt 0.7 的 `#[interrupt]` 宏展开时引用
//! `interrupt::<NAME>;` 做存在性检查，而 0.37 在 CortexM target 下不再生成
//! `pub mod interrupt`。此 shim 把根层 Interrupt 枚举的变体映射到该路径。
//! 重新生成 PAC 不会覆盖本文件（gen-pac.sh 只拷 lib.rs/build.rs/device.x，
//! lib.rs 尾部的模块挂载由脚本自动追加）。
pub use crate::Interrupt;

/// 每个中断一个常量名，供 `#[interrupt]` 宏的存在性检查语句引用
#[allow(non_snake_case)]
pub mod consts {
    pub use crate::Interrupt::*;
}
