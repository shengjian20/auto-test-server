//! ENET：MAC/PHY 基础设施（MDIO 原语 + LAN8720A PHY 驱动）
//!
//! 事实链（阶段 3 首个闸门板测实证）：
//! - RMII 模式 = SYSCFG_CFG1.ENETPHYSEL=1（须在 ENET 复位前设置）
//! - 时钟：RCU_AHB1EN.{ENETTXE,ENETRXE,ENETPTPE,ENETE}；REF_CLK 来自
//!   LAN8720A 25MHz 晶振板载倍频输出（50MHz），SWR 自动清零即存活的证据
//! - PHY：LAN8720A @ MDIO addr 0，ID1/ID2 = 0x0007/0xC0F1（SMSC 签名）
//! - 引脚（全 AF11）：PA1=REF_CLK、PA2=MDIO、PA7=CRS_DV、PC1=MDC、
//!   PC4/5=RXD0/1、PB11=TX_EN、PB12/13=TXD0/1；ETH_nRST=PC0（低有效）
//!
//! MDIO 时序（GD32 手册 ENET 章节）：PA/PR/PW 写 MAC_PHY_CTL -> PB=1 启动
//! -> 硬件完成自动清 PB -> 读 MAC_PHY_DATA。CLR=0 时 MDC≈HCLK/42（16MHz
//! HSI 下 ~380kHz，MDIO 上限 2.5MHz 内）。
//!
//! unsafe 收敛：本模块零 unsafe（全 PAC 安全 API）。

use gd32f470::{enet_dma, enet_mac};

/// LAN8720A 固定 MDIO 地址（板上实测，见 enet-phy-probe）
pub const PHY_ADDR: u8 = 0;

/// PHY 寄存器（BMCR/BMSR 标准定义）
pub const REG_BMCR: u8 = 0; // 基本控制：复位/自协商使能
pub const REG_BMSR: u8 = 1; // 基本状态：自协商完成/链路
pub const REG_PHY_ID1: u8 = 2;
pub const REG_PHY_ID2: u8 = 3;

/// BMCR 位（W25Q 式命名收敛点：全部用位掩码，语义见 LAN8720A 手册）
pub const BMCR_RESET: u16 = 0x8000;
pub const BMCR_LOOPBACK: u16 = 0x4000;
pub const BMCR_SPEED100: u16 = 0x2000;
pub const BMCR_DUPLEX: u16 = 0x0100;
pub const BMCR_ANENABLE: u16 = 0x1000;
pub const BMCR_ANRESTART: u16 = 0x0200;

/// MDIO 控制器封装（MAC_PHY_CTL/DATA）
pub struct Mdio<'a> {
    mac: &'a enet_mac::RegisterBlock,
}

impl<'a> Mdio<'a> {
    pub fn new(mac: &'a enet_mac::RegisterBlock) -> Self {
        // MDC 时钟范围：CLR=0（HCLK/(42+2*2^0)≈380kHz @16MHz HSI）
        mac.mac_phy_ctl().modify(|_, w| w.clr().set(0));
        Self { mac }
    }

    /// 读 PHY 寄存器（阻塞等 PB 完成，带超时护栏）
    pub fn read(&self, phy: u8, reg: u8) -> Option<u16> {
        self.mac
            .mac_phy_ctl()
            .modify(|_, w| w.pa().set(phy).pr().set(reg).pw().clear_bit());
        self.mac.mac_phy_ctl().modify(|_, w| w.pb().set_bit());
        let mut guard = 2_000_000u32;
        while self.mac.mac_phy_ctl().read().pb().bit_is_set() {
            guard -= 1;
            if guard == 0 {
                return None;
            }
        }
        Some(self.mac.mac_phy_data().read().pd().bits())
    }

    /// 写 PHY 寄存器
    pub fn write(&self, phy: u8, reg: u8, value: u16) -> Option<()> {
        self.mac
            .mac_phy_data()
            .write(|w| w.pd().set(value));
        self.mac
            .mac_phy_ctl()
            .modify(|_, w| w.pa().set(phy).pr().set(reg).pw().set_bit());
        self.mac.mac_phy_ctl().modify(|_, w| w.pb().set_bit());
        let mut guard = 2_000_000u32;
        while self.mac.mac_phy_ctl().read().pb().bit_is_set() {
            guard -= 1;
            if guard == 0 {
                return None;
            }
        }
        Some(())
    }
}

/// LAN8720A PHY 驱动（MDIO 之上）
pub struct Phy<'a> {
    mdio: Mdio<'a>,
    addr: u8,
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum LinkState {
    Down,
    Up10Half,
    Up10Full,
    Up100Half,
    Up100Full,
}

impl<'a> Phy<'a> {
    pub fn new(mac: &'a enet_mac::RegisterBlock) -> Self {
        Self { mdio: Mdio::new(mac), addr: PHY_ADDR }
    }

    /// 读 PHY ID（ID1/ID2 拼接），None = MDIO 无应答
    pub fn read_id(&self) -> Option<u32> {
        let id1 = self.mdio.read(self.addr, REG_PHY_ID1)?;
        let id2 = self.mdio.read(self.addr, REG_PHY_ID2)?;
        Some((u32::from(id1) << 16) | u32::from(id2))
    }

    /// BMCR 复位位自清即复位完成。None 情形（MDIO 无应答）按失败处理。
    pub fn reset(&self) -> bool {
        if self.mdio.write(self.addr, REG_BMCR, 0x8000).is_none() {
            return false;
        }
        let mut guard = 1_000_000u32;
        loop {
            match self.mdio.read(self.addr, REG_BMCR) {
                Some(bmcr) if bmcr & 0x8000 == 0 => return true,
                Some(_) => {}
                None => return false,
            }
            guard -= 1;
            if guard == 0 {
                return false;
            }
        }
    }

    /// 启动自协商（BMCR.ANENABLE|ANRESTART = 0x1200）
    pub fn start_autoneg(&self) -> bool {
        self.mdio.write(self.addr, REG_BMCR, 0x1200).is_some()
    }

    /// PHY 内部回环模式（BMCR.LOOPBACK bit14=1）：TX 引脚数据短接回 RX，
    /// 不依赖网线/对端。100M 全双工固定态（回环下自协商失效，需手动设速）
    pub fn set_loopback_100m(&self) -> bool {
        let v = BMCR_SPEED100 | BMCR_DUPLEX | BMCR_LOOPBACK;
        self.mdio.write(self.addr, REG_BMCR, v).is_some()
    }

    /// 退出回环，恢复正常（自协商重启动）
    pub fn clear_loopback(&self) -> bool {
        self.mdio.write(self.addr, REG_BMCR, 0x1200).is_some()
    }

    /// 链路状态（BMSR.bit2 LinkStatus + bit5 AutonegComplete；
    /// 解析自协商结果寄存器 31/5 的并行检测位）
    pub fn link_state(&self) -> Option<LinkState> {
        let bmsr = self.mdio.read(self.addr, REG_BMSR)?;
        if bmsr & 0x0004 == 0 {
            return Some(LinkState::Down);
        }
        // LAN8720A 寄存器 31 (0x1F) 页选择/特殊模式：bit[4:3] = 速度/双工
        // (00=10H 01=10F 10=100H 11=100F)，自协商完成自动反映
        let special = self.mdio.read(self.addr, 31)?;
        let speed = (special >> 3) & 0b11;
        let _ = bmsr;
        Some(match speed {
            0b00 => LinkState::Up10Half,
            0b01 => LinkState::Up10Full,
            0b10 => LinkState::Up100Half,
            0b11 => LinkState::Up100Full,
            _ => LinkState::Down,
        })
    }

    /// 原始 MDIO 访问（诊断用）
    pub fn mdio(&mut self) -> &mut Mdio<'a> {
        &mut self.mdio
    }
}

/// ENET 时钟使能（rcc.rs 之外的补充：DMA 时钟已在 enable_enet 覆盖）
pub fn sw_reset(dma: &enet_dma::RegisterBlock) -> bool {
    dma.dma_bctl().modify(|_, w| w.swr().set_bit());
    let mut guard = 2_000_000u32;
    while dma.dma_bctl().read().swr().bit_is_set() {
        guard -= 1;
        if guard == 0 {
            return false; // REF_CLK 死线
        }
    }
    true
}
