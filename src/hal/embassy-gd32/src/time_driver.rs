//! embassy-gd32 time driver：TIMER1（32 位计数器）@1MHz + CH1 单比较闹钟
//!
//! 实测事实（本板）：TIMER1 计数器/自动重装/比较通道均为 32 位（SVD CNT/CAR/
//! CHxCV 字段宽 32，板上读回 CNT>0xFFFF 佐证）——与 ST TIM2 血统一致，
//! 区别于 embassy-stm32 gp16 假设的 16 位通用定时器，故无需周期记账：
//! - now() = 单次读 CNT（u32），零扩展为 u64。embassy 闹钟跨度恒小于
//!   溢出周期（71.6min@1MHz），32 位模语义由队列滚动窗口处理
//! - 闹钟：CH1 单比较通道，diff 直接写比较值恒使能
//! - 时钟链：RCU_APB1EN.TIMER1EN 必须先开（外设时钟未开时寄存器写入
//!   静默丢弃——板上实测踩坑）
//! - 中断接线：官方 #[interrupt] 属性宏（cortex-m-rt device feature，
//!   embassy-stm32 low_power.rs 同款写法），HAL 内部单例，用户无需绑定
//!
//! unsafe 收敛（2 处）：regs() PTR 派生引用（time_driver_impl! 链接期
//! 单例语义保证独占）；NVIC unmask（handler 由本模块独占提供）

use core::cell::RefCell;
use core::sync::atomic::{compiler_fence, Ordering};
use core::task::Waker;

use critical_section::{CriticalSection, Mutex};
use embassy_time_driver::Driver;
use embassy_time_queue_utils::Queue;
use crate::interrupt;
use gd32f470::timer1;
use cortex_m_rt::interrupt as interrupt_attr;
use gd32f470::Timer1;

/// tick 分频：CK_AHB(16MHz HSI)/16 = 1MHz
const PSC_DIV: u16 = 15;

/// 闹钟时刻（CS 保护；u64::MAX = 无闹钟）
struct AlarmState {
    timestamp: core::cell::Cell<u64>,
}

unsafe impl Send for AlarmState {}

impl AlarmState {
    const fn new() -> Self {
        Self { timestamp: core::cell::Cell::new(u64::MAX) }
    }
}

struct Timer1Driver {
    queue: Mutex<RefCell<Queue>>,
    alarm: Mutex<AlarmState>,
}

/// unsafe 依据：time_driver_impl! 保证 Driver 全局唯一（重复注册链接期失败），
/// TIMER1 寄存器块仅被本单例访问。引用从 Periph::PTR 静态地址派生，无临时值。
fn regs() -> &'static timer1::RegisterBlock {
    unsafe { &*Timer1::PTR }
}

impl Timer1Driver {
    fn init(&self) {
        let rb = regs();

        // 外设时钟（APB1）：不开则寄存器写丢弃（板上实测踩坑）
        let rcu = unsafe { &*gd32f470::Rcu::PTR };
        rcu.apb1en().modify(|_, w| w.timer1en().set_bit());
        // 写后读回同步（GD32 RCU 手册要求）
        let _ = rb.cnt().read();

        // 1MHz tick，自由跑到 2^32。
        // PSC 是影子寄存器：写入后必须发 UG 更新事件才锁存生效，
        // 否则计数器按 PSC=0（16MHz）跑——板上实测"闪烁过快"即此因
        // unsafe 依据（bits() x2）：TIMER1 PSC 16b / CAR 32b 均为无保留位
        // 全值域计数字段（手册 TIMER 章节），PSC_DIV/MAX 为合法值
        unsafe {
            rb.psc().write(|w| w.psc().bits(PSC_DIV));
            rb.car().write(|w| w.carl().bits(u32::MAX));
        }
        rb.swevg().write(|w| w.upg().set_bit()); // UG: 装载 PSC/CAR 影子值
        rb.intf().write(|w| w.upif().clear_bit()); // UG 会置 UPIF，清掉
        rb.intf().write(|w| w.upif().clear_bit().ch1if().clear_bit());
        rb.dmainten().write(|w| w.ch1ie().set_bit());
        rb.ctl0().modify(|_, w| w.cen().set_bit().arse().clear_bit());

        // unsafe 依据：TIMER1 vector 符号由本模块 #[interrupt] 函数独占接线；
        // unmask 只打开该中断线
        unsafe {
            compiler_fence(Ordering::SeqCst);
            cortex_m::peripheral::NVIC::unmask(gd32f470::Interrupt::TIMER1);
        }
    }

    /// 设置闹钟。false = 时刻已过（调用方重排队列）。
    fn set_alarm(&self, cs: CriticalSection, timestamp: u64) -> bool {
        let rb = regs();
        self.alarm.borrow(cs).timestamp.set(timestamp);

        let t = self.now();
        let diff = timestamp.wrapping_sub(t);
        if diff < 2 {
            rb.dmainten().modify(|_, w| w.ch1ie().clear_bit());
            self.alarm.borrow(cs).timestamp.set(u64::MAX);
            return false;
        }
        // 队列空时 embassy 传 u64::MAX 作为"无闹钟"哨兵：排一个 32 位最远闹钟
        // 让循环以 true 退出（gp16 同款语义：CCV=MAX 的远闹钟作废性触发无害）。
        // 若不做此分支，next_expiration 的 MAX 会让 while 永远 false -> ISR 死锁
        let diff_capped = diff.min(u32::MAX as u64);

        // 32 位模语义：比较值 = (now + diff) mod 2^32，硬件自由匹配
        let target = (t as u32).wrapping_add(diff_capped as u32);
        // unsafe 依据（bits()）：CH1CV 32b 全值域计数字段
        unsafe { rb.ch1cv().write(|w| w.ch1val().bits(target)); }
        rb.intf().write(|w| w.ch1if().clear_bit());
        rb.dmainten().modify(|_, w| w.ch1ie().set_bit());

        // 竞态复核：设置过程中时刻已过（gp16 同款）
        if timestamp <= self.now() {
            rb.dmainten().modify(|_, w| w.ch1ie().clear_bit());
            self.alarm.borrow(cs).timestamp.set(u64::MAX);
            return false;
        }
        true
    }
}

impl Driver for Timer1Driver {
    fn now(&self) -> u64 {
        regs().cnt().read().cnt().bits() as u64
    }

    fn schedule_wake(&self, at: u64, waker: &Waker) {
        critical_section::with(|cs| {
            let mut queue = self.queue.borrow(cs).borrow_mut();
            if queue.schedule_wake(at, waker) {
                let mut next = queue.next_expiration(self.now());
                while !self.set_alarm(cs, next) {
                    next = queue.next_expiration(self.now());
                }
            }
        });
    }
}

embassy_time_driver::time_driver_impl!(static DRIVER: Timer1Driver = Timer1Driver {
    queue: Mutex::new(RefCell::new(Queue::new())),
    alarm: Mutex::new(AlarmState::new()),
});

/// 固件 main 任务开头调用：初始化 TIMER1 时基 + NVIC
pub fn init_time_driver() {
    DRIVER.init();
}

/// TIMER1 中断处理体
///
/// 官方接线：`#[interrupt]` 属性宏（cortex-m-rt rt/device feature 提供，
/// svd2rust 0.37 的 PAC 在 CortexM target 下不重复导出该宏，文档 660 行
/// 指明 device 中断处理统一走此属性）。符号覆盖 device.x 的弱默认，
/// 链接期完成中断名校验。
#[interrupt_attr]
fn TIMER1() {
    let rb = regs();
    let flags = rb.intf().read();
    if flags.ch1if().bit_is_set() {
        rb.intf().write(|w| w.ch1if().clear_bit());
        critical_section::with(|cs| {
            let mut next = DRIVER.queue.borrow(cs).borrow_mut().next_expiration(DRIVER.now());
            while !DRIVER.set_alarm(cs, next) {
                next = DRIVER.queue.borrow(cs).borrow_mut().next_expiration(DRIVER.now());
            }
        });
    }
    // 溢出标志（71.6 分钟一次）：清掉即可，u64 语义由 embassy 队列处理
    if flags.upif().bit_is_set() {
        rb.intf().write(|w| w.upif().clear_bit());
    }
}


