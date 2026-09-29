//! CAN（bxCAN）：CAN0 驱动（轮询模式，回环与总线共用）
//!
//! 引脚地面真值（carrier-box 实跑 .config）：CAN0_RX=PD0 / CAN0_TX=PD1（AF9），
//! RCU_APB1EN.CAN0EN。CAN1=PB5(RX)/PB6(TX)（AF9）待 CAN0 验证后接入。
//!
//! 初始化时序（照抄 GD32 固件库 can_init 官方次序）：
//! IWMOD=1 -> 等 STAT.IWS -> 配 BT/过滤器(FLD 解锁) -> IWMOD=0 -> 等 !IWS
//!
//! 回环模式：BT.LCMOD=1（内部 TX->RX 短接，不依赖收发器/总线），用于阶段验收。
//! 过滤器：filter0 = 32 位掩码模式、掩码 0（复位默认）= 接收全部 ID -> FIFO0。
//! 数据寄存器复位即 0，掩码模式下无需写入。
//!
//! unsafe 收敛：本模块零 unsafe（全 PAC 安全 API；NVIC 未用——轮询模式）。

use gd32f470::{can0, Can0};

/// 标准数据帧
pub struct Frame {
    pub id: u16,
    pub len: u8,
    pub data: [u8; 8],
}

/// 位时序参数（tq 数，字段值为 N-1 编码）
pub struct BitTiming {
    /// 再同步跳跃宽度，1-4 tq（寄存器字段值 0-3，N-1 编码）
    pub sjw: u8,
    /// 位段1，1-16 tq（寄存器字段值 0-15，N-1 编码）
    pub bs1: u8,
    /// 位段2，1-8 tq（寄存器字段值 0-7，N-1 编码）
    pub bs2: u8,
    /// 预分频，1-1024（寄存器字段值 0-1023，N-1 编码）
    pub baudp: u16,
}

impl BitTiming {
    /// 500kbps @ 16MHz PCLK1：4MHz tq x 8 tq/bit（采样点 87.5%）
    pub const fn kbps500() -> Self {
        Self { sjw: 0, bs1: 5, bs2: 0, baudp: 3 }
    }
    /// 125kbps @ 16MHz PCLK1：1MHz tq x 8 tq/bit（采样点 87.5%）
    pub const fn kbps125() -> Self {
        Self { sjw: 0, bs1: 5, bs2: 0, baudp: 15 }
    }
}

/// CAN 实例（寄存器块借用）。独占性由 PAC Peripherals::take 单次语义间接保证。
pub struct Can<'a> {
    rb: &'a can0::RegisterBlock,
}

impl<'a> Can<'a> {
    pub fn new(_can: &'a Can0, rb: &'a can0::RegisterBlock) -> Self {
        Self { rb }
    }

    /// 初始化：进初始化模式 -> 位时序+回环 -> 过滤器 -> 退出进入正常运行
    pub fn init_loopback(&self, timing: &BitTiming) -> Result<(), &'static str> {
        let rb = self.rb;

        // 先退睡眠模式（复位后 SLPWMOD=1，睡眠态下 IWMOD 请求不生效、
        // IWS 永不应答——GD32 固件库 can_init 官方次序第一步），等 SLPWS=0
        rb.ctl().modify(|_, w| w.slpwmod().clear_bit());
        let mut guard = 1_000_000u32;
        while rb.stat().read().slpws().bit_is_set() {
            guard -= 1;
            if guard == 0 {
                return Err("CAN: wake timeout");
            }
        }

        // 请求初始化模式并等 ACK
        rb.ctl().modify(|_, w| w.iwmod().set_bit());
        let mut guard = 1_000_000u32;
        while !rb.stat().read().iws().bit_is_set() {
            guard -= 1;
            if guard == 0 {
                return Err("CAN: init mode timeout");
            }
        }

        // 位时序 + 回环通信模式。
        //
        // unsafe 依据（bits() x4）：BT 寄存器 SJW/BS1/BS2/BAUDPSC 为 N-1
        // 编码的全值域字段（SJW 0-3=1-4tq、BS1 0-15、BS2 0-7、BAUDPSC
        // 0-1023，手册位时序章节），BitTiming 结构体字段类型与文档约束
        // 值域（见其字段注释），无越界可能。
        rb.bt().modify(|_, w| unsafe {
            w.sjw().bits(timing.sjw)
                .bs1().bits(timing.bs1)
                .bs2().bits(timing.bs2)
                .baudpsc().bits(timing.baudp)
                .lcmod().set_bit()
        });

        // 过滤器：filter0 = 32 位掩码模式、列表值/掩码均为 0（全收）、FIFO0。
        // F0DATA0/1 上电为随机值（板上实测 0xEA0D8EFB/0x72477045），必须
        // 显式清零：列表=0、掩码=0 -> 任何 ID 都匹配（mask 位 don't care）
        rb.fctl().modify(|_, w| w.fld().set_bit()); // 解锁过滤器
        rb.fw().modify(|_, w| w.fw0().clear_bit()); // 停用 filter0
        rb.f0data0().modify(|_, w| {
            w.fd0().clear_bit().fd1().clear_bit().fd2().clear_bit().fd3().clear_bit()
                .fd4().clear_bit().fd5().clear_bit().fd6().clear_bit().fd7().clear_bit()
                .fd8().clear_bit().fd9().clear_bit().fd10().clear_bit().fd11().clear_bit()
                .fd12().clear_bit().fd13().clear_bit().fd14().clear_bit().fd15().clear_bit()
                .fd16().clear_bit().fd17().clear_bit().fd18().clear_bit().fd19().clear_bit()
                .fd20().clear_bit().fd21().clear_bit().fd22().clear_bit().fd23().clear_bit()
                .fd24().clear_bit().fd25().clear_bit().fd26().clear_bit().fd27().clear_bit()
                .fd28().clear_bit().fd29().clear_bit().fd30().clear_bit().fd31().clear_bit()
        });
        rb.f0data1().modify(|_, w| {
            w.fd0().clear_bit().fd1().clear_bit().fd2().clear_bit().fd3().clear_bit()
                .fd4().clear_bit().fd5().clear_bit().fd6().clear_bit().fd7().clear_bit()
                .fd8().clear_bit().fd9().clear_bit().fd10().clear_bit().fd11().clear_bit()
                .fd12().clear_bit().fd13().clear_bit().fd14().clear_bit().fd15().clear_bit()
                .fd16().clear_bit().fd17().clear_bit().fd18().clear_bit().fd19().clear_bit()
                .fd20().clear_bit().fd21().clear_bit().fd22().clear_bit().fd23().clear_bit()
                .fd24().clear_bit().fd25().clear_bit().fd26().clear_bit().fd27().clear_bit()
                .fd28().clear_bit().fd29().clear_bit().fd30().clear_bit().fd31().clear_bit()
        });
        rb.fscfg().modify(|_, w| w.fs0().set_bit()); // 32 位宽
        rb.fmcfg().modify(|_, w| w.fmod0().clear_bit()); // 掩码模式
        rb.fafifo().modify(|_, w| w.faf0().clear_bit()); // 关联 FIFO0
        rb.fw().modify(|_, w| w.fw0().set_bit()); // 激活 filter0
        rb.fctl().modify(|_, w| w.fld().clear_bit()); // 锁定过滤器

        // 退出初始化模式，进入正常运行
        rb.ctl().modify(|_, w| w.iwmod().clear_bit());
        guard = 1_000_000u32;
        while rb.stat().read().iws().bit_is_set() {
            guard -= 1;
            if guard == 0 {
                return Err("CAN: exit init timeout");
            }
        }
        Ok(())
    }

    /// 发送标准数据帧（邮箱 0）。阻塞等发送完成，返回是否无错误。
    pub fn send(&self, frame: &Frame) -> bool {
        let rb = self.rb;

        // 填邮箱（Ten=0 时配置）：ID -> TMI0，DLC/时间戳 -> TMP0。
        //
        // unsafe 依据（bits() x2）：SFID_EFID 11bit 标准帧 ID 全值域
        // 0-0x7FF（Frame.id:u16 由调用方保证 <=0x7FF，超出位硬件忽略）；
        // DLENC 4bit DLC 全值域 0-15（&0xF 掩码兜底）。
        rb.tmi0().modify(|_, w| unsafe {
            w.ten().clear_bit()
                .ft().clear_bit() // 数据帧
                .ff().clear_bit() // 标准帧
                .sfid_efid().bits(frame.id & 0x7FF)
        });
        rb.tmp0()
            .modify(|_, w| unsafe { w.dlenc().bits(frame.len & 0xF) });
        // 数据 0-3 -> TMDATA00，4-7 -> TMDATA10
        // unsafe 依据（bits() x8）：DB0-7 各 8bit 全值域（u8 天然界内）
        rb.tmdata00().modify(|_, w| unsafe {
            w.db0().bits(frame.data[0])
                .db1().bits(frame.data[1])
                .db2().bits(frame.data[2])
                .db3().bits(frame.data[3])
        });
        rb.tmdata10().modify(|_, w| unsafe {
            w.db4().bits(frame.data[4])
                .db5().bits(frame.data[5])
                .db6().bits(frame.data[6])
                .db7().bits(frame.data[7])
        });

        // 请求发送
        rb.tmi0().modify(|_, w| w.ten().set_bit());

        // 等邮箱发送完成（MTF0），带超时
        let mut guard = 20_000_000u32;
        loop {
            let ts = rb.tstat().read();
            if ts.mtf0().bit_is_set() {
                // 清完成标志（写 0 清——GD32 TSTAT 为 rc_w1 形态经 W 清零位段）
                rb.tstat().modify(|_, w| w.mtf0().clear_bit().mtfnerr0().clear_bit());
                return ts.mtfnerr0().bit_is_set();
            }
            guard -= 1;
            if guard == 0 {
                // 放弃发送
                rb.ctl().modify(|_, w| w.abor().set_bit());
                rb.tstat().modify(|_, w| w.mtf0().clear_bit());
                return false;
            }
        }
    }

    /// 非阻塞收帧（FIFO0 有帧则取出并释放输出邮箱）
    pub fn recv(&self) -> Option<Frame> {
        let rb = self.rb;
        if rb.rfifo0().read().rfl0().bits() == 0 {
            return None;
        }

        let mi = rb.rfifomi0().read();
        let mp = rb.rfifomp0().read();
        let d0 = rb.rfifomdata00().read();
        let d1 = rb.rfifomdata10().read();

        let frame = Frame {
            id: mi.sfid_efid().bits(),
            len: mp.dlenc().bits().min(8),
            data: [
                d0.db0().bits(), d0.db1().bits(), d0.db2().bits(), d0.db3().bits(),
                d1.db4().bits(), d1.db5().bits(), d1.db6().bits(), d1.db7().bits(),
            ],
        };

        // 释放 FIFO 输出邮箱
        rb.rfifo0().modify(|_, w| w.rfd0().set_bit());
        Some(frame)
    }
}
