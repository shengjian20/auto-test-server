//! SPI：主模式抽象（SPI3 = W25Q256/W25Q128 Flash 总线，PE2/4/5 AF5 + PE3 软件 CS）
//!
//! 事实链（三方吻合，无猜测）：
//! - carrier-box drv_spi.c：spi_bus3 = GPIOE + AF5 + PIN_2 起
//! - 原理图：W25Q256_SPI_* 网络连 PE2(CLK)/PE3(CS)/PE4(MISO)/PE5(MOSI)
//! - GD32F470 手册 AF 表：PE2=SPI3_SCK、PE3=SPI3_NSS、PE4=SPI3_MISO、PE5=SPI3_MOSI（AF5）
//!
//! 布局：SPI3 derivedFrom SPI0（spi0::RegisterBlock），时钟 RCU_APB2EN.SPI3EN
//! （APB2 复位分频=1，PCLK2=16MHz HSI）。安全 API：DATA/PSC 经 SVD 补丁为 Safe。
//! 片选用软件控制（PE3 GPIO），不走硬件 NSS——flash 单从机场景更直接。

use gd32f470::spi0;

/// SPI 主机实例
pub struct Spi<'a> {
    rb: &'a spi0::RegisterBlock,
}

impl<'a> Spi<'a> {
    /// 从寄存器块构造（未使能）
    pub fn new(rb: &'a spi0::RegisterBlock) -> Self {
        Self { rb }
    }

    /// 使能主模式：APB2 时钟 PCLK2、PSC 分频取不超 apb_hz/2 的最大档、
    /// CPOL=0/CPHA=0（W25Q SPI mode 0）、8 位帧（FF16=0）、软件 NSS
    pub fn enable_master(&self, pclk_hz: u32, target_baud: u32) {
        let rb = self.rb;
        rb.ctl0().modify(|_, w| w.spien().clear_bit());

        // PSC 3bit：波特率 = PCLK / 2^(psc+1)，选不超过目标的最低分频
        let mut psc: u8 = 0;
        while psc < 7 && (pclk_hz >> (psc + 1)) > target_baud {
            psc += 1;
        }

        // unsafe 依据（bits()）：SPI CTL0.PSC 3bit 分频档 0-7 全部为合法
        // 值（PCLK/2^(n+1) 逐档定义），psc 变量 0..=7 由上方循环边界保证
        rb.ctl0().modify(|_, w| unsafe {
            w.mstmod()
                .set_bit() // 主模式
                .psc()
                .bits(psc)
                .swnssen()
                .set_bit() // NSS 软件管理（SSI 置高防 MODF）
                .swnss()
                .set_bit()
        });

        rb.ctl0().modify(|_, w| w.spien().set_bit());
    }

    /// 全双工传输一字节：写 DATA 触发时钟，等 RBNE 读回
    pub fn transfer(&self, b: u8) -> u8 {
        while !self.rb.stat().read().tbe().bit_is_set() {
            core::hint::spin_loop();
        }
        // unsafe 依据（bits()）：SPI DATA 16b 全宽无保留位（手册 SPI 章节）
        self.rb.data().write(|w| unsafe { w.spi_data().bits(b as u16) });
        while !self.rb.stat().read().rbne().bit_is_set() {
            core::hint::spin_loop();
        }
        self.rb.data().read().spi_data().bits() as u8
    }

    /// 写缓冲（读侧丢弃）
    pub fn write(&self, buf: &[u8]) {
        for &b in buf {
            self.transfer(b);
        }
    }

    /// 读缓冲（写侧填 dummy）
    pub fn read(&self, buf: &mut [u8]) {
        for slot in buf.iter_mut() {
            *slot = self.transfer(0xFF);
        }
    }
}
