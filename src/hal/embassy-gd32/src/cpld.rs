//! CPLD 协议驱动：SPI2 总线上的 Lattice CPLD 寄存器访问（V1.0 协议）
//!
//! 事实链（carrier-box 实跑配置 core_cpld.c + drv_cpld.c + drv_cpld.h）：
//! - 总线：SPI2 @ PC10(SCK)/PC11(MISO)/PC12(MOSI) AF6，Mode0 MSB，16 位帧，
//!   500kHz（保守值），CS=PA15（软件），RST=PA4（硬复位：低 10ms -> 高）
//! - 帧结构（两段式，CS 全程保持）：cmd 帧内嵌应答字 0x55AA，随后 data 帧
//! - 命令集（高 4 位）：
//!   0x1<<12 SET_EXIO  写 GPO（低 6 位 dout）        [写]
//!   0x2<<12 GET_EXIO  读 GPI                        [读]
//!   0x3<<12 SET_UART  写 uart_mux（低 3 位）        [写]
//!   0x4<<12 GET_UART  读 uart_mux                   [读]
//!   EXUART 切换目标值：0x80=MCU_UART，0x81-0x85=EXUART0-4

use crate::gpio::Pin;
use crate::spi::Spi;

pub const CMD_SET_EXIO: u16 = 0x1 << 12;
pub const CMD_GET_EXIO: u16 = 0x2 << 12;
pub const CMD_SET_UART: u16 = 0x3 << 12;
pub const CMD_GET_UART: u16 = 0x4 << 12;

/// uart_mux 目标值（协议文档 V1.0 + carrier-box drv_cpld.h）
pub const TO_MCU_UART: u16 = 0x80;
pub const TO_EXUART0: u16 = 0x81;
pub const TO_EXUART1: u16 = 0x82;
pub const TO_EXUART2: u16 = 0x83;
pub const TO_EXUART3: u16 = 0x84;
pub const TO_EXUART4: u16 = 0x85;

/// 应答魔数（cmd 帧读回字段，协议 V1.0）
const RESP_MAGIC: u16 = 0x55AA;

pub enum CpldError {
    /// 0x55AA 应答校验失败（CPLD 未复位/未就绪/总线异常）
    BadResponse(u16),
}

/// CPLD 设备：SPI 传输 + 软件 CS（Pin 类型复用，零 unsafe）
pub struct Cpld<'a> {
    spi: &'a Spi<'a>,
    cs: &'a mut Pin<'a>,
}

impl<'a> Cpld<'a> {
    /// 构造（SPI 已 enable_master、CS 已配输出且空闲高）
    pub fn new(spi: &'a Spi<'a>, cs: &'a mut Pin<'a>) -> Self {
        Self { spi, cs }
    }

    /// 硬复位 CPLD（core_cpld.c 同款时序：低 10ms -> 高）
    pub fn hard_reset(cs: &mut Pin<'a>) {
        cs.set_low();
        // ~10ms 忙等（16MHz HSI）
        for _ in 0..10 {
            for _ in 0..16_000 {
                core::hint::spin_loop();
            }
        }
        cs.set_high();
    }

    /// 协议传输原语：cmd 帧内嵌 0x55AA 应答（全双工，carrier-box msg1
    /// send+recv 同 16 时钟的时序——应答与命令同相位回读，不是额外字节），
    /// 随后 data 帧按命令方向读或写。CS 全程保持。
    fn transfer(&mut self, cmd: u16, data: Option<u16>) -> Result<u16, CpldError> {
        self.cs.set_low();
        let r1 = self.spi.transfer((cmd >> 8) as u8);
        let r2 = self.spi.transfer((cmd & 0xFF) as u8);
        let resp = (u16::from(r1) << 8) | u16::from(r2);

        let out = match data {
            Some(d) => {
                self.spi.transfer((d >> 8) as u8);
                self.spi.transfer((d & 0xFF) as u8);
                0
            }
            None => {
                let dhi = self.spi.transfer(0xFF);
                let dlo = self.spi.transfer(0xFF);
                (u16::from(dhi) << 8) | u16::from(dlo)
            }
        };
        self.cs.set_high();

        if resp != RESP_MAGIC {
            return Err(CpldError::BadResponse(resp));
        }
        Ok(out)
    }

    /// 写 GPO（SET_EXIO，低 6 位）
    pub fn set_exio(&mut self, dout: u16) -> Result<(), CpldError> {
        self.transfer(CMD_SET_EXIO, Some(dout & 0x3F))?;
        Ok(())
    }

    /// 读 GPI（GET_EXIO）
    pub fn get_exio(&mut self) -> Result<u16, CpldError> {
        self.transfer(CMD_GET_EXIO, None)
    }

    /// 切换 uart_mux（SET_UART）
    pub fn set_uart_mux(&mut self, target: u16) -> Result<(), CpldError> {
        self.transfer(CMD_SET_UART, Some(target))?;
        Ok(())
    }

    /// 读 uart_mux 当前值（GET_UART）
    pub fn get_uart_mux(&mut self) -> Result<u16, CpldError> {
        self.transfer(CMD_GET_UART, None)
    }
}
