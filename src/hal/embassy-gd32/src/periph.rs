//! PAC 外设句柄集（'static 寄存器块引用形态；console/延迟等 HAL 基础
//! 设施的公共入口——跳转链/二次初始化场景下 take() 因 DEVICE_PERIPHERALS
//! 标志返回 None，steal 语义是板上实证的正解）
//!
//! unsafe 收敛点：外设地址为 SVD 定案物理常驻（PAC Periph::PTR 为 const），
//! 引用永不悬垂；调用方需自行保证无别名写（本模块仅供 HAL 基础设施
//! 初始化使用，应用外设操作仍应走 Peripherals::take 的独占分配）
#![allow(unsafe_code)]

use gd32f470::{
    syscfg, Can0, EnetDma, EnetMac, Gpioa, Gpiob, Gpioc, Gpiod, Gpioe, Rcu, Spi2, Spi3, Syscfg,
    Timer1, Uart6,
};

/// 外设句柄集（'static 寄存器块引用；子句柄自动获得 'static 生命周期）
pub struct Peripherals {
    pub rcu: &'static Rcu,
    pub gpioa: &'static Gpioa,
    pub gpiob: &'static Gpiob,
    pub gpioc: &'static Gpioc,
    pub gpiod: &'static Gpiod,
    pub gpioe: &'static Gpioe,
    pub uart6: &'static Uart6,
    pub spi2: &'static Spi2,
    pub spi3: &'static Spi3,
    pub can0: &'static Can0,
    pub timer1: &'static Timer1,
    pub enet_mac: &'static EnetMac,
    pub enet_dma: &'static EnetDma,
    pub syscfg: &'static Syscfg,
}

/// steal 语义获取：字段为 PAC 别名引用（'static），ZST 句柄经
/// NonNull::dangling 派生引用——Periph<RB, A> 是 PhantomData 零大小
/// 结构，ZST 引用永不解引用、悬垂无副作用（svd2rust 0.37 语义）；
/// 寄存器块实际访问经 Deref -> PTR 物理地址（与 control-server 的
/// app::periph 同款已验证形态）
pub fn steal() -> Peripherals {
    unsafe fn zst<T>(v: T) -> &'static T {
        unsafe { core::ptr::NonNull::dangling().as_ref() }
    }
    unsafe {
        Peripherals {
            rcu: zst(Rcu::steal()),
            gpioa: zst(Gpioa::steal()),
            gpiob: zst(Gpiob::steal()),
            gpioc: zst(Gpioc::steal()),
            gpiod: zst(Gpiod::steal()),
            gpioe: zst(Gpioe::steal()),
            uart6: zst(Uart6::steal()),
            spi2: zst(Spi2::steal()),
            spi3: zst(Spi3::steal()),
            can0: zst(Can0::steal()),
            timer1: zst(Timer1::steal()),
            enet_mac: zst(EnetMac::steal()),
            enet_dma: zst(EnetDma::steal()),
            syscfg: zst(Syscfg::steal()),
        }
    }
}
