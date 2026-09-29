//! W25Q SPI NOR Flash 命令层（阶段 2d flash-rw 已验证命令集的 HAL 收编）
//!
//! 命令集（W25Q128 datasheet）：WREN=0x06 / RDSR=0x05 / READ=0x03 /
//! PAGE_PROGRAM=0x02 / SECTOR_ERASE(4K)=0x20 / JEDEC ID=0x9F
//! 时序参数：扇区擦 typ 30-60ms / 页编程 typ 0.4-3ms（wait_ready 轮询 WIP）
#![no_std]

use crate::gpio::Pin;
use crate::spi::Spi;

const CMD_WRITE_ENABLE: u8 = 0x06;
const CMD_READ_DATA: u8 = 0x03;
const CMD_PAGE_PROGRAM: u8 = 0x02;
const CMD_SECTOR_ERASE: u8 = 0x20;
const CMD_READ_STATUS: u8 = 0x05;
const CMD_JEDEC_ID: u8 = 0x9F;

/// W25Q 页大小（页编程不可跨页）
pub const PAGE_SIZE: usize = 256;
/// 扇区大小（最小擦除单元）
pub const SECTOR_SIZE: u32 = 4096;

/// W25Q 设备句柄（SPI 总线 + 软件 CS）
pub struct W25q<'a> {
    spi: &'a Spi<'a>,
    cs: &'a mut Pin<'a>,
}

impl<'a> W25q<'a> {
    pub fn new(spi: &'a Spi<'a>, cs: &'a mut Pin<'a>) -> Self {
        Self { spi, cs }
    }

    fn write_enable(&mut self) {
        self.cs.set_low();
        self.spi.write(&[CMD_WRITE_ENABLE]);
        self.cs.set_high();
    }

    /// 轮询 WIP（Write In Progress）位直到空闲
    pub fn wait_ready(&mut self) {
        loop {
            self.cs.set_low();
            self.spi.write(&[CMD_READ_STATUS]);
            let st = self.spi.transfer(0xFF);
            self.cs.set_high();
            if st & 1 == 0 {
                return; // WIP=0
            }
            // 1ms 忙等（16MHz）
            for _ in 0..4000 {
                core::hint::spin_loop();
            }
        }
    }

    /// 读 JEDEC ID（3 字节：厂商/类型/容量）
    pub fn jedec_id(&mut self) -> [u8; 3] {
        self.cs.set_low();
        self.spi.write(&[CMD_JEDEC_ID]);
        let mut id = [0u8; 3];
        for slot in id.iter_mut() {
            *slot = self.spi.transfer(0xFF);
        }
        self.cs.set_high();
        id
    }

    /// 读数据（任意长度，自动跨页）
    pub fn read(&mut self, addr: u32, buf: &mut [u8]) {
        self.cs.set_low();
        self.spi.write(&[
            CMD_READ_DATA,
            (addr >> 16) as u8,
            (addr >> 8) as u8,
            addr as u8,
        ]);
        for slot in buf.iter_mut() {
            *slot = self.spi.transfer(0xFF);
        }
        self.cs.set_high();
    }

    /// 扇区擦除（4K，阻塞等待完成）
    pub fn erase_sector(&mut self, addr: u32) {
        self.write_enable();
        self.cs.set_low();
        self.spi.write(&[
            CMD_SECTOR_ERASE,
            (addr >> 16) as u8,
            (addr >> 8) as u8,
            addr as u8,
        ]);
        self.cs.set_high();
        self.wait_ready();
    }

    /// 页编程（单页 ≤256B，不得跨页；阻塞等待完成）
    pub fn page_program(&mut self, addr: u32, data: &[u8]) {
        self.write_enable();
        self.cs.set_low();
        self.spi.write(&[
            CMD_PAGE_PROGRAM,
            (addr >> 16) as u8,
            (addr >> 8) as u8,
            addr as u8,
        ]);
        for &b in data {
            self.spi.transfer(b);
        }
        self.cs.set_high();
        self.wait_ready();
    }

    /// 任意长度写入（自动按页切分；目标区域需已擦除）
    pub fn write(&mut self, mut addr: u32, data: &[u8]) {
        let mut off = 0usize;
        while off < data.len() {
            let page_rem = PAGE_SIZE - (addr as usize % PAGE_SIZE);
            let n = core::cmp::min(page_rem, data.len() - off);
            self.page_program(addr, &data[off..off + n]);
            addr += n as u32;
            off += n;
        }
    }
}
