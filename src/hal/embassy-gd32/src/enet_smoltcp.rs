//! smoltcp Device 适配层（HAL 收编：tcp-echo 与 control-server-tcp 单一来源）
//!
//! 对接 enet_dma 描述符环：
//! - RX：first_valid() 扫描 → FCS 剥离 → 栈上 scratch 拷贝后立即归还描述符
//! - TX：tx_commit 零拷贝提交（调用方写入环缓冲后放所有权）
//! - TBU/RBU 挂起恢复：poll 循环条件化清除 + poll-demand（板上实证：
//!   无恢复则 smoltcp 提交的帧滞留描述符永不发出——ARP 永不解析）
#![no_std]

use crate::enet_dma;
#[cfg(feature = "smoltcp-device")]
use smoltcp::phy::{Device, DeviceCapabilities, Medium, RxToken, TxToken};
#[cfg(feature = "smoltcp-device")]
use smoltcp::time::Instant;

pub struct EnetDevice<'a> {
    pub rx: &'a mut enet_dma::RDesRing,
    pub tx: &'a mut enet_dma::TDesRing,
    pub dma: &'a gd32f470::EnetDma,
}

pub struct EnetRxToken<'a> {
    len: usize,
    rx: &'a mut enet_dma::RDesRing,
    idx: usize,
}

impl<'a> EnetRxToken<'a> {
    /// 构造（HAL 内部由 Device::receive 调用；跨 crate 复用此入口）
    pub fn new(len: usize, rx: &'a mut enet_dma::RDesRing, idx: usize) -> Self {
        Self { len, rx, idx }
    }
}

#[cfg(feature = "smoltcp-device")]
impl RxToken for EnetRxToken<'_> {
    fn consume<R, F>(self, f: F) -> R
    where
        F: FnOnce(&mut [u8]) -> R,
    {
        // FL 含 4 字节 FCS，剥离后经栈上 scratch 交付（描述符立即归还，
        // 避免环内 buf 被 f 闭包借用导致的复用悬垂）
        let n = self.len.min(enet_dma::BUF_SIZE);
        let mut scratch = [0u8; enet_dma::BUF_SIZE];
        scratch[..n].copy_from_slice(&self.rx.buf[self.idx][..n]);
        self.rx.release(self.idx);
        f(&mut scratch[..n])
    }
}

pub struct EnetTxToken<'a> {
    tx: &'a mut enet_dma::TDesRing,
}

impl<'a> EnetTxToken<'a> {
    /// 构造（HAL 内部由 Device::transmit 调用；跨 crate 复用此入口）
    pub fn new(tx: &'a mut enet_dma::TDesRing) -> Self {
        Self { tx }
    }
}

#[cfg(feature = "smoltcp-device")]
impl TxToken for EnetTxToken<'_> {
    fn consume<R, F>(self, len: usize, f: F) -> R
    where
        F: FnOnce(&mut [u8]) -> R,
    {
        let i = self.tx.index;
        let r = f(&mut self.tx.buf[i][..len]);
        if self.tx.tx_commit(len) {
            enet_dma_stats::tx_ok();
        } else {
            enet_dma_stats::tx_fail();
        }
        r
    }
}

#[cfg(feature = "smoltcp-device")]
impl Device for EnetDevice<'_> {
    type RxToken<'a>
        = EnetRxToken<'a>
    where
        Self: 'a;
    type TxToken<'a>
        = EnetTxToken<'a>
    where
        Self: 'a;

    fn receive(&mut self, _timestamp: Instant) -> Option<(Self::RxToken<'_>, Self::TxToken<'_>)> {
        let idx = self.rx.first_valid()?;
        enet_dma_stats::rx_hit();
        let fl = self.rx.frame_len(idx);
        let len = fl.saturating_sub(4).min(enet_dma::BUF_SIZE); // 剥 FCS
        let rx = &mut *self.rx;
        let tx = &mut *self.tx;
        Some((EnetRxToken { len, rx, idx }, EnetTxToken { tx }))
    }

    fn transmit(&mut self, _timestamp: Instant) -> Option<Self::TxToken<'_>> {
        if self.tx.available() {
            Some(EnetTxToken { tx: &mut *self.tx })
        } else {
            None
        }
    }

    fn capabilities(&self) -> DeviceCapabilities {
        let mut caps = DeviceCapabilities::default();
        caps.medium = Medium::Ethernet;
        caps.max_transmission_unit = 1500;
        caps.max_burst_size = Some(enet_dma::RING_LEN as usize);
        caps
    }
}

/// 帧计数器（诊断心跳；Atomic 避免诊断状态侵入 Device 的借用结构）
pub mod enet_dma_stats {
    use core::sync::atomic::{AtomicU32, Ordering};

    static RX_HITS: AtomicU32 = AtomicU32::new(0);
    static TX_OK: AtomicU32 = AtomicU32::new(0);
    static TX_FAIL: AtomicU32 = AtomicU32::new(0);

    pub fn rx_hit() {
        RX_HITS.fetch_add(1, Ordering::Relaxed);
    }
    pub fn tx_ok() {
        TX_OK.fetch_add(1, Ordering::Relaxed);
    }
    pub fn tx_fail() {
        TX_FAIL.fetch_add(1, Ordering::Relaxed);
    }
    pub fn snapshot() -> (u32, u32, u32) {
        (
            RX_HITS.load(Ordering::Relaxed),
            TX_OK.load(Ordering::Relaxed),
            TX_FAIL.load(Ordering::Relaxed),
        )
    }
}

/// TX/RX 挂起恢复（主循环每拍调用；板上实证缺失后果见模块注释）
pub fn recover_suspended(dma: &gd32f470::EnetDma) {
    if dma.dma_stat().read().rbu().bit_is_set() {
        dma.dma_stat().write(|w| w.rbu().clear_bit());
        unsafe { dma.dma_rpen().write(|w| w.rpe().bits(1)) };
    }
    if dma.dma_stat().read().tbu().bit_is_set() {
        dma.dma_stat().write(|w| w.tbu().clear_bit());
        unsafe { dma.dma_tpen().write(|w| w.tpe().bits(1)) };
    }
}

// ---- embassy-net Driver 适配（socket 风格 async API 的载体；需求 1）----
#[cfg(feature = "embassy-net")]
use embassy_net_driver;
// embassy-net-driver::Driver 与本模块上方的 smoltcp phy::Device 几乎同构
// （RxToken/TxToken consume 签名一致），仅多 link_state/hardware_address
// 与 Context 参数——薄包装复用同一组 token 类型。

/// Runner poll 唤醒设施：receive/transmit 挂起时注册 Runner 任务的
/// waker（embassy-net-driver 契约：返回 None 必须注册，否则 Runner::run
/// 的 poll_fn 永挂——板上实测 connect 超时的根因）；wake() 由外部节拍
/// 任务周期调用（轮询式网卡无 RX 中断的"外部 kick"正统形态）。
/// 实现选 critical_section::Mutex（HAL 已依赖，token 天然匹配；
/// embassy-sync 0.6 Mutex 仅有 async lock，同步场景不适用）
#[cfg(feature = "embassy-net")]
pub mod net_poll {
    use core::cell::RefCell;

    static WAKER: critical_section::Mutex<RefCell<Option<core::task::Waker>>> =
        critical_section::Mutex::new(RefCell::new(None));
    static KICKS: core::sync::atomic::AtomicU32 = core::sync::atomic::AtomicU32::new(0);
    static POLLS: core::sync::atomic::AtomicU32 = core::sync::atomic::AtomicU32::new(0);

    /// Driver::receive/transmit 返回 None 时注册（挂起方 waker 入队；
    /// will_wake 去重避免 clone 泛滥）
    pub fn register(cx: &mut core::task::Context) {
        critical_section::with(|cs| {
            let mut w = WAKER.borrow_ref_mut(cs);
            let need = w.as_ref().map(|w| !w.will_wake(cx.waker())).unwrap_or(true);
            if need {
                *w = Some(cx.waker().clone());
            }
        });
    }

    /// 外部节拍唤醒（kick 任务的 2ms 拍调用；take 防重复 wake）
    pub fn wake() {
        KICKS.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
        critical_section::with(|cs| {
            if let Some(w) = WAKER.borrow_ref_mut(cs).take() {
                w.wake();
            }
        });
    }

    /// kicker 存活探针（wake 计数；诊断用）
    pub fn kicks() -> u32 {
        KICKS.load(core::sync::atomic::Ordering::Relaxed)
    }

    /// accept/协议任务轮询计数（对照 kicker 计数判定任务调度健康）
    pub fn polls() -> u32 {
        POLLS.load(core::sync::atomic::Ordering::Relaxed)
    }

    /// 协议任务 poll 打点（main 循环每拍调用；诊断用）
    pub fn poll_tick() {
        POLLS.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
    }
}

#[cfg(feature = "embassy-net")]
impl embassy_net_driver::RxToken for EnetRxToken<'_> {
    fn consume<R, F>(self, f: F) -> R
    where
        F: FnOnce(&mut [u8]) -> R,
    {
        let n = self.len.min(enet_dma::BUF_SIZE);
        let mut scratch = [0u8; enet_dma::BUF_SIZE];
        scratch[..n].copy_from_slice(&self.rx.buf[self.idx][..n]);
        self.rx.release(self.idx);
        f(&mut scratch[..n])
    }
}

#[cfg(feature = "embassy-net")]
impl embassy_net_driver::TxToken for EnetTxToken<'_> {
    fn consume<R, F>(self, len: usize, f: F) -> R
    where
        F: FnOnce(&mut [u8]) -> R,
    {
        let i = self.tx.index;
        let r = f(&mut self.tx.buf[i][..len]);
        let _ = self.tx.tx_commit(len);
        r
    }
}

/// 外部节拍唤醒 Runner poll（bin 的 poll_kicker 2ms 拍调用）
#[cfg(feature = "embassy-net")]
pub fn wake_net_poll() {
    net_poll::wake();
}

#[cfg(feature = "embassy-net")]
pub struct EnetNetDriver<'a> {
    pub rx: &'a mut enet_dma::RDesRing,
    pub tx: &'a mut enet_dma::TDesRing,
    pub mac: [u8; 6],
    pub link_up: fn() -> bool,
}

#[cfg(feature = "embassy-net")]
impl embassy_net_driver::Driver for EnetNetDriver<'_> {
    type RxToken<'a>
        = EnetRxToken<'a>
    where
        Self: 'a;
    type TxToken<'a>
        = EnetTxToken<'a>
    where
        Self: 'a;

    fn receive(&mut self, cx: &mut core::task::Context) -> Option<(Self::RxToken<'_>, Self::TxToken<'_>)> {
        let idx = match self.rx.first_valid() {
            Some(i) => i,
            None => {
                // 契约：返回 None 必须注册 waker（否则 Runner poll_fn 永挂）
                net_poll::register(cx);
                return None;
            }
        };
        enet_dma_stats::rx_hit();
        let fl = self.rx.frame_len(idx);
        let len = fl.saturating_sub(4).min(enet_dma::BUF_SIZE); // 剥 FCS
        let rx = &mut *self.rx;
        let tx = &mut *self.tx;
        Some((EnetRxToken { len, rx, idx }, EnetTxToken { tx }))
    }

    fn transmit(&mut self, _cx: &mut core::task::Context) -> Option<Self::TxToken<'_>> {
        if self.tx.available() {
            Some(EnetTxToken { tx: &mut *self.tx })
        } else {
            None
        }
    }

    fn link_state(&mut self, _cx: &mut core::task::Context) -> embassy_net_driver::LinkState {
        if (self.link_up)() {
            embassy_net_driver::LinkState::Up
        } else {
            embassy_net_driver::LinkState::Down
        }
    }

    fn capabilities(&self) -> embassy_net_driver::Capabilities {
        let mut caps = embassy_net_driver::Capabilities::default();
        caps.max_transmission_unit = 1500;
        caps.max_burst_size = Some(enet_dma::RING_LEN as usize);
        caps
    }

    fn hardware_address(&self) -> embassy_net_driver::HardwareAddress {
        embassy_net_driver::HardwareAddress::Ethernet(self.mac)
    }
}
