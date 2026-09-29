//! ENET DMA 描述符层：TX/RX 环形描述符（Synopsys MAC v1 布局，GD32 固件库
//! enet_descriptors_struct 同构——status/control/buffer1/next 四字，非增强模式）
//!
//! 地面真值（carrier-box 实跑 drv_eth.c -> 固件库 enet_descriptors_chain_init）：
//! - 描述符 4 字 × 5 个/方向 × 1524B 缓冲
//! - TX：FS|LS|TCH|IOC 于 tdes0，长度 tdes1[12:0]，缓冲地址 tdes2，链表 tdes3，
//!   环尾 TER，OWN 位 31 归属 DMA
//! - RX：RCH|RBUFF 于 rdes1，OWN 位 31，FS(9)/LS(8)/ES(15)/FL[29:16] 于写回
//!
//! 缓冲区为 Packet 对齐数组（4 字节对齐即可；DMA 一致性靠 Cortex-M4 无 DCache
//! 天然满足——GD32F470 与 ST F4 同为无 D-Cache 的 M4F，无需维护缓存行）。
#![no_std]

use core::sync::atomic::{compiler_fence, fence, Ordering};

// ---- TX 描述符位域（固件库 gd32f4xx_enet.h 同值） ----
pub const TXDESC_OWN: u32 = 1 << 31;
pub const TXDESC_IOC: u32 = 1 << 30;
pub const TXDESC_FS: u32 = 1 << 28;
pub const TXDESC_LS: u32 = 1 << 29;
pub const TXDESC_TER: u32 = 1 << 21;
pub const TXDESC_TCH: u32 = 1 << 20;
pub const TXDESC_ES: u32 = 1 << 15;
const TXDESC_TBS_MASK: u32 = 0x0FFF;

// ---- RX 描述符位域 ----
pub const RXDESC_OWN: u32 = 1 << 31;
pub const RXDESC_FS: u32 = 1 << 9;
pub const RXDESC_LS: u32 = 1 << 8;
pub const RXDESC_ES: u32 = 1 << 15;
pub const RXDESC_FL_SHIFT: usize = 16;
const RXDESC_FL_MASK: u32 = 0x3FFF;
pub const RXDESC_RCH: u32 = 1 << 14;
pub const RXDESC_RER: u32 = 1 << 15;
const RXDESC_RBS_MASK: u32 = 0x0FFF;

/// 单向环容量（与 carrier-box 一致取 5；扩容只改这里）
pub const RING_LEN: usize = 5;
/// 单缓冲容量（GD32 固件库 ENET_MAX_FRAME_SIZE）
pub const BUF_SIZE: usize = 1524;

/// TX 描述符（4 字，非增强模式）
#[repr(C)]
#[derive(Copy, Clone)]
pub struct TDes {
    status: u32,
    control: u32,
    buf1: u32,
    next: u32,
}

/// RX 描述符
#[repr(C)]
#[derive(Copy, Clone)]
pub struct RDes {
    status: u32,
    control: u32,
    buf1: u32,
    next: u32,
}

/// TX 环（描述符 + 数据缓冲，整体一起放 static）
pub struct TDesRing {
    pub desc: [TDes; RING_LEN],
    pub buf: [[u8; BUF_SIZE]; RING_LEN],
    pub index: usize,
}

/// RX 环
pub struct RDesRing {
    pub desc: [RDes; RING_LEN],
    pub buf: [[u8; BUF_SIZE]; RING_LEN],
    pub index: usize,
}

impl TDes {
    pub const fn new() -> Self {
        Self { status: 0, control: 0, buf1: 0, next: 0 }
    }
}

impl RDes {
    pub const fn new() -> Self {
        Self { status: 0, control: 0, buf1: 0, next: 0 }
    }
}

impl Default for TDes {
    fn default() -> Self {
        Self::new()
    }
}

impl Default for RDes {
    fn default() -> Self {
        Self::new()
    }
}

impl TDesRing {
    pub const fn new() -> Self {
        Self {
            desc: [TDes::new(); RING_LEN],
            buf: [[0; BUF_SIZE]; RING_LEN],
            index: 0,
        }
    }

    /// 环初始化：链表互连 + 首段/末段标志 + 缓冲地址绑定。
    /// 由固件在 DMA 停止态调用一次（DMA 启动后描述符由硬件所有权接管）
    pub fn init(&mut self) {
        for i in 0..RING_LEN {
            let next_addr = if i == RING_LEN - 1 {
                // 环尾：TCH 不设 + TER 置位，next 归零
                0
            } else {
                &self.desc[i + 1] as *const TDes as u32
            };
            // 字序勘误（GD32 固件库 TDES0 位定义实证）：TDES0=状态+控制混合
            // （FS28|LS29|TCH20|IOC30|TER21），TDES1=纯长度 TBS[12:0]。
            // 曾把 FS/LS/TCH/IOC 写进 TDES1——高位污染长度字段、TDES0 缺
            // FS/LS，DMA 取到畸形描述符（TBU 假象/零长帧），MAC LBM 全断
            self.desc[i].status = TXDESC_TCH | TXDESC_IOC | TXDESC_FS | TXDESC_LS;
            if i == RING_LEN - 1 {
                self.desc[i].status |= TXDESC_TER;
            }
            self.desc[i].control = 0; // TBS1 长度在 submit 时填
            self.desc[i].buf1 = &self.buf[i] as *const [u8; BUF_SIZE] as u32;
            self.desc[i].next = next_addr;
        }
        self.index = 0;
    }

    /// 描述符可用（DMA 已归还）
    pub fn available(&self) -> bool {
        self.desc[self.index].status & TXDESC_OWN == 0
    }

    /// 装载一帧并交 DMA（单描述符 FS|LS 全帧模式，<= BUF_SIZE）
    pub fn submit(&mut self, frame: &[u8]) -> bool {
        if frame.is_empty() || frame.len() > BUF_SIZE {
            return false;
        }
        if !self.available() {
            return false;
        }
        let i = self.index;
        self.buf[i][..frame.len()].copy_from_slice(frame);
        self.desc[i].control = frame.len() as u32; // TDES1 = TBS1 纯长度
        // 提交前用数据填充，之后才放所有权
        fence(Ordering::Release);
        compiler_fence(Ordering::Release);
        self.desc[i].status |= TXDESC_OWN;
        fence(Ordering::SeqCst);

        self.index = (self.index + 1) % RING_LEN;
        true
    }
}

impl RDesRing {
    pub const fn new() -> Self {
        Self {
            desc: [RDes::new(); RING_LEN],
            buf: [[0; BUF_SIZE]; RING_LEN],
            index: 0,
        }
    }

    /// 环初始化：RCH 链 + 缓冲地址 + 所有权交 DMA
    pub fn init(&mut self) {
        for i in 0..RING_LEN {
            let next_addr = if i == RING_LEN - 1 {
                &self.desc[0] as *const RDes as u32
            } else {
                &self.desc[i + 1] as *const RDes as u32
            };
            self.desc[i].control = RXDESC_RCH | BUF_SIZE as u32;
            self.desc[i].buf1 = &self.buf[i] as *const [u8; BUF_SIZE] as u32;
            self.desc[i].next = next_addr;
            // 交 DMA：RX 环始终 OWN=DMA
            fence(Ordering::Release);
            compiler_fence(Ordering::Release);
            self.desc[i].status |= RXDESC_OWN;
            fence(Ordering::SeqCst);
        }
        self.index = 0;
    }

    /// 扫描环找首个"OWN=0 且 FS|LS 完整"的描述符（接收完成后定位用）。
    /// rx.index 不由硬件推进（硬件按描述符链顺序写回 OWN），不能用
    /// index-1 推断刚收的帧——扫描式查找（板上踩坑：恒指 desc4 误报）
    pub fn first_valid(&self) -> Option<usize> {
        (0..RING_LEN).find(|&d| {
            self.desc[d].status & RXDESC_OWN == 0 && self.frame_valid(d)
        })
    }

    /// 可收帧数（OWN=0 的连续段）
    pub fn pending(&self) -> usize {
        self.desc
            .iter()
            .filter(|d| d.status & RXDESC_OWN == 0)
            .count()
    }
}

/// 全局环实例（TX/RX 一起；描述符地址会被写入 DMA 寄存器，必须常驻 RAM）
pub struct Rings {
    pub tx: TDesRing,
    pub rx: RDesRing,
}

static mut RINGS: Rings = Rings {
    tx: TDesRing::new(),
    rx: RDesRing::new(),
};

/// 取全局环。
///
/// unsafe 依据：单消费者约定（阶段验收固件为单任务轮询，无并发访问者；
/// 描述符地址仅在 init 时写入 DMA 寄存器一次）。多任务共享将由后续
/// 阶段的内存池/spin 互斥重构，届时删除本函数。
pub fn take_rings() -> &'static mut Rings {
    unsafe { &mut *core::ptr::addr_of_mut!(RINGS) }
}

impl RDesRing {
    /// 归还描述符给 DMA（重新武装 OWN；缓冲地址/长度已由 init 配好）
    pub fn release(&mut self, i: usize) {
        fence(Ordering::Release);
        compiler_fence(Ordering::Release);
        self.desc[i].status |= RXDESC_OWN;
        fence(Ordering::SeqCst);
    }

    /// 读描述符 i 的接收帧长（FL 字段，含 MAC 去除 CRC 前的帧长语义由
    /// Apcd 决定；验收用 payload 子串匹配，不依赖精确长度）
    pub fn frame_len(&self, i: usize) -> usize {
        ((self.desc[i].status >> RXDESC_FL_SHIFT) & RXDESC_FL_MASK) as usize
    }

    /// 描述符 i 是否为完整帧（FS|LS 且无 ES）
    pub fn frame_valid(&self, i: usize) -> bool {
        let st = self.desc[i].status;
        (st & (RXDESC_ES | RXDESC_FS | RXDESC_LS)) == (RXDESC_FS | RXDESC_LS)
    }
}

/// DMA 启动（环绑定 + 表地址 + store-and-forward + 收发启动）。
///
/// unsafe 依据：`&rings` 的描述符地址写入 DMA_RDTADDR/STT——地址来自
/// 调用方持有的 static 环（take_rings 返回的同一实例），生命周期 'static，
/// DMA 与 CPU 的并发由固件侧单任务轮询约定保证。
pub fn start_dma(dma: &gd32f470::EnetDma, rings: &Rings) {
    let base_rx = &rings.rx.desc as *const _ as u32;
    let base_tx = &rings.tx.desc as *const _ as u32;
    // unsafe 依据（bits x3）：DMA_RDTADDR/STT 32 位环基地址指针全值域；
    // DMA_BCTL.PGBL 6bit 突发长度（1-32 beat 全部为合法档位，32 beat 为
    // GD32 固件库 ENET_PGBL_32BEAT 推荐值——未配置时 DMA 传输异常 TBU，
    // 板上实测）。注意 DPSL(bits2-6) 是描述符跳隔字数（TCH/RCH 链表
    // 模式下相邻描述符间的跳过字数），非增强 4 字描述符保持 0——
    // 曾误把突发长度写进 DPSL(31)（板上实测踩坑）。
    unsafe {
        dma.dma_bctl().modify(|_, w| w.pgbl().bits(32));
        dma.dma_rdtaddr().write(|w| w.srt().bits(base_rx));
        dma.dma_tdtaddr().write(|w| w.stt().bits(base_tx));
    }
    dma.dma_ctl().modify(|_, w| {
        w.rsfd().set_bit().tsfd().set_bit().sre().set_bit().ste().set_bit()
    });
}

/// 清 TBU 标志（发送缓冲不可用置位后 DMA 停等，须清标志+重 poll）
pub fn clear_tbu(dma: &gd32f470::EnetDma) {
    dma.dma_stat().write(|w| w.tbu().clear_bit());
}

/// TX poll-demand 触发（写任意值使 DMA 重读 TX 描述符）
pub fn tx_poll(dma: &gd32f470::EnetDma) {
    unsafe {
        dma.dma_tpen().write(|w| w.tpe().bits(1));
    }
}

/// RX poll-demand（RX DMA 挂起态（RP=4/RBU）恢复：写任意值重读 RX 描述符）
pub fn rx_poll(dma: &gd32f470::EnetDma) {
    unsafe {
        dma.dma_rpen().write(|w| w.rpe().bits(1));
    }
}

impl TDesRing {
    /// 零拷贝提交：数据已由调用方写入 buf[index]（smoltcp TxToken 路径），
    /// 本方法仅填长度 + 放所有权（submit() 是先拷贝后提交的两步式，不适合
    /// TxToken 的闭包借用形态）
    pub fn tx_commit(&mut self, len: usize) -> bool {
        if len == 0 || len > BUF_SIZE {
            return false;
        }
        if !self.available() {
            return false;
        }
        let i = self.index;
        self.desc[i].control = len as u32; // TDES1 = TBS1 纯长度
        fence(Ordering::Release);
        compiler_fence(Ordering::Release);
        self.desc[i].status |= TXDESC_OWN;
        fence(Ordering::SeqCst);
        self.index = (self.index + 1) % RING_LEN;
        true
    }
}
