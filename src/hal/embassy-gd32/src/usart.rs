//! USART/UART：串行口抽象（同步轮询 API 先行，async+DMA 后续阶段引入）
//!
//! 引脚映射（权威来源：carrier-box drv_usart.c 实跑配置 + target.md 接口标注，
//! 二者交叉验证）：
//! - PC_RS232_1 = UART6 @ APB1，TX=PE7 / RX=PE8（AF8，carrier-box 实跑配置）
//! - RS485_1    = USART1 @ APB1，TX=PA2 / RX=PA3（AF7）+ DE/RE 方向脚（后半段）
//! - CPLD_UART  = 经 CPLD uart_mux 切换（与 CPLD 驱动一起做）
//!
//! 寄存器事实（GD32 命名，PAC uart3::RegisterBlock 布局，UART6 复用）：
//! STAT0@0x00(TBE=bit7/RBNE=bit5)、DATA@0x04(9bit)、BAUD@0x08(INTDIV[15:4]+
//! FRADIV[3:0]，组合值=PCLK/baud)、CTL0@0x0C(UEN/TEN/REN)。BAUD/DATA 字段
//! 经 SVD writeConstraint 补丁为 Safe writer。

use gd32f470::uart3;

/// 串口实例（寄存器块借用）。独占性由 PAC Peripherals::take 单次语义间接保证。
pub struct Uart<'a> {
    rb: &'a uart3::RegisterBlock,
}

impl<'a> Uart<'a> {
    /// 从寄存器块构造（未使能；先备好 GPIO/时钟再调 enable）
    pub fn new(rb: &'a uart3::RegisterBlock) -> Self {
        Self { rb }
    }

    /// 使能外设：115200-N8-1（oversample16，BAUD=PCLK/baud）
    pub fn enable(&self, pclk_hz: u32, baud: u32) {
        let rb = self.rb;
        // 配置期间关外设（UEN=0 时才可写 BAUD 等）
        rb.ctl0().modify(|_, w| w.uen().clear_bit());

        let reg = (pclk_hz / baud) as u16;
        rb.baud().write(|w| {
            w.intdiv().set(reg >> 4).fradiv().set((reg & 0xF) as u8)
        });

        // CTL0：8 位字长（WL=0 复位默认）、无校验（PCEN=0）、使能 TX/RX/外设
        rb.ctl0()
            .modify(|_, w| w.ten().set_bit().ren().set_bit().uen().set_bit());
        // CTL2 复位默认即 1 停止位、无流控
    }

    /// 阻塞写单字节（等 STAT0.TBE）
    pub fn write_byte(&self, b: u8) {
        while !self.rb.stat0().read().tbe().bit_is_set() {
            core::hint::spin_loop();
        }
        self.rb.data().write(|w| w.data().set(b as u16));
    }

    /// 阻塞写缓冲
    pub fn write(&self, buf: &[u8]) {
        for &b in buf {
            self.write_byte(b);
        }
        // 等移位完成（TC），保证返回后字节全部上线
        while !self.rb.stat0().read().tc().bit_is_set() {
            core::hint::spin_loop();
        }
    }

    /// 非阻塞读：RBNE 置位时返回接收字节（读 DATA 自动清 RBNE）
    pub fn read_byte(&self) -> Option<u8> {
        if self.rb.stat0().read().rbne().bit_is_set() {
            Some(self.rb.data().read().data().bits() as u8)
        } else {
            None
        }
    }
}
