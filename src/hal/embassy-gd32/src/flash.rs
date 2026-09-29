//! 片上 FLASH 自编程驱动（GD32F470 FMC @0x40023C00）
//!
//! unsafe 收敛说明（项目约束：HAL 单点 unsafe + 逐点依据注释）：
//! - 解锁序列（KEY 写 0x45670123/0xCDEF89AB）：svd2rust 原生 PAC 对全宽
//!   KEY 字段只提供 unsafe `bits()`（无 writeConstraint，用户 2026-09-29
//!   拍板 PAC 禁止语义增强）。键值来自 GigaDevice 固件库 gd32f4xx_fmc.h
//!   UNLOCK_KEY0/1（硬编码芯片协议值，非任意值）
//! - CTL 位组合写（PG/SER/SN/PSZ/START）：同为原生 unsafe `bits()`，
//!   位域语义来自固件库 gd32f4xx_fmc.c 的官方时序
//! - 扇区映射按 1MB 型号：0-3 扇区 16KB，4 扇区 64KB，5-11 扇区 128KB
#![no_std]

use gd32f470::fmc;

/// 1MB 型号扇区基址表（扇区号 -> 起始地址）
pub const SECTOR_BASE: [u32; 12] = [
    0x0800_0000, 0x0800_4000, 0x0800_8000, 0x0800_C000, // 0-3: 16KB
    0x0801_0000,                                        // 4:   64KB
    0x0802_0000, 0x0804_0000, 0x0806_0000,              // 5-7: 128KB
    0x0808_0000, 0x080A_0000, 0x080C_0000, 0x080E_0000, // 8-11: 128KB
];

#[derive(Debug)]
pub enum FlashError {
    BusyTimeout,
    OpError,
    Unaligned,
    OutOfRange,
}

pub struct Flash {
    rb: &'static fmc::RegisterBlock,
}

impl Flash {
    /// unsafe 依据：FMC::PTR 静态外设地址（SVD 定案 0x40023C00）派生引用，
    /// Flash 实例由调用方独占（HAL 单点收敛，同 enet/time_driver 模式）
    pub fn new() -> Self {
        Self {
            rb: unsafe { &*gd32f470::Fmc::PTR },
        }
    }

    fn wait_ready(&self, spin_limit: u32) -> Result<(), FlashError> {
        let mut n = spin_limit;
        while self.rb.stat().read().busy().bit_is_set() {
            n -= 1;
            if n == 0 {
                return Err(FlashError::BusyTimeout);
            }
            core::hint::spin_loop();
        }
        Ok(())
    }

    /// 解锁 FMC（LOCK 位清除后才能写 CTL）
    pub fn unlock(&self) {
        if self.rb.ctl().read().lk().bit_is_set() {
            // unsafe 依据：解锁键值为芯片协议硬编码（gd32f4xx_fmc.h UNLOCK_KEY0/1）
            unsafe {
                self.rb.key().write(|w| w.key().bits(0x4567_0123));
                self.rb.key().write(|w| w.key().bits(0xCDEF_89AB));
            }
        }
    }

    pub fn lock(&self) {
        self.rb.ctl().modify(|_, w| w.lk().set_bit());
    }

    /// 清除全部历史状态标志（写 1 清），保证后续状态判定干净
    fn clear_flags(&self) {
        self.rb
            .stat()
            .modify(|_, w| w.end().clear_bit().operr().clear_bit().wperr().clear_bit().pgmerr().clear_bit().pgserr().clear_bit().rdderr().clear_bit());
    }

    /// 检查操作结果状态位
    fn check_result(&self) -> Result<(), FlashError> {
        let s = self.rb.stat().read();
        if s.pgmerr().bit_is_set() || s.wperr().bit_is_set() || s.pgserr().bit_is_set() || s.operr().bit_is_set() {
            return Err(FlashError::OpError);
        }
        Ok(())
    }

    /// 扇区擦除（阻塞轮询，FMC 操作期间 CPU 取指自动停等）
    pub fn erase_sector(&self, sector: u8) -> Result<(), FlashError> {
        if sector > 11 {
            return Err(FlashError::OutOfRange);
        }
        self.wait_ready(2_000_000)?;
        self.clear_flags();
        self.rb
            .ctl()
            .modify(|_, w| unsafe { w.ser().set_bit().sn().bits(sector) });
        // unsafe 依据：START 位触发擦除，固件库官方时序
        unsafe {
            self.rb.ctl().modify(|_, w| w.start().set_bit());
        }
        self.wait_ready(20_000_000)?;
        self.rb.ctl().modify(|_, w| w.ser().clear_bit());
        self.check_result()
    }

    /// 字编程（地址必须 4 字节对齐；PSZ=word）
    pub fn program_word(&self, addr: u32, word: u32) -> Result<(), FlashError> {
        if addr % 4 != 0 {
            return Err(FlashError::Unaligned);
        }
        if addr < SECTOR_BASE[0] || addr > SECTOR_BASE[11] + 128 * 1024 {
            return Err(FlashError::OutOfRange);
        }
        self.wait_ready(2_000_000)?;
        self.clear_flags();
        // unsafe 依据：PSZ=2（按字编程，固件库 CTL_PSZ_WORD）+ PG 置位
        unsafe {
            self.rb.ctl().modify(|_, w| w.psz().bits(2).pg().set_bit());
        }
        // volatile 写触发编程（flash 区域地址，编译器不得合并/省略）
        unsafe {
            core::ptr::write_volatile(addr as *mut u32, word);
        }
        self.wait_ready(2_000_000)?;
        self.rb.ctl().modify(|_, w| w.pg().clear_bit());
        self.check_result()?;
        // 读回校验
        let readback = unsafe { core::ptr::read_volatile(addr as *const u32) };
        if readback != word {
            return Err(FlashError::OpError);
        }
        Ok(())
    }

    /// 读回一个字（volatile；固件侧读 flash 内容的安全入口）
    /// unsafe 依据：仅地址范围检查后的一次 volatile 读，无别名写
    pub fn read_word(&self, addr: u32) -> Result<u32, FlashError> {
        if addr % 4 != 0 || addr < SECTOR_BASE[0] || addr > SECTOR_BASE[11] + 128 * 1024 {
            return Err(FlashError::OutOfRange);
        }
        Ok(unsafe { core::ptr::read_volatile(addr as *const u32) })
    }

    /// 批量编程
    pub fn program_buf(&self, addr: u32, data: &[u32]) -> Result<(), FlashError> {
        for (i, w) in data.iter().enumerate() {
            self.program_word(addr + (i as u32) * 4, *w)?;
        }
        Ok(())
    }
}
