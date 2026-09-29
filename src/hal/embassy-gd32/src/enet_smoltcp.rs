//! smoltcp Device 适配层（HAL 收编：tcp-echo 与 control-server-tcp 单一来源）
//!
//! 对接 enet_dma 描述符环：
//! - RX：first_valid() 扫描 → FCS 剥离 → 栈上 scratch 拷贝后立即归还描述符
//! - TX：tx_commit 零拷贝提交（调用方写入环缓冲后放所有权）
//! - TBU/RBU 挂起恢复：poll 循环条件化清除 + poll-demand（板上实证：
//!   无恢复则 smoltcp 提交的帧滞留描述符永不发出——ARP 永不解析）
#![no_std]

use crate::enet_dma;
use smoltcp::phy::{Device, DeviceCapabilities, Medium, RxToken, TxToken};
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
