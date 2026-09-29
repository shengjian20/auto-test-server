//! CRC-32/ISO-HDLC（zlib 兼容）：初值 0xFFFFFFFF、反射多项式 0xEDB88320、
//! 终值异或 0xFFFFFFFF——与 PC 侧 Python `zlib.crc32` 数值一致，
//! 用于 OTA 镜像完整性校验（PC 生成镜像包时算 CRC，bootloader 独立复算）。
//! 查表法（256 项，编译期生成），每字节 1 次查表 + 异或。
#![no_std]

const POLY: u32 = 0xEDB8_8320;

/// 编译期生成反射 CRC 表
const fn build_table() -> [u32; 256] {
    let mut table = [0u32; 256];
    let mut i = 0usize;
    while i < 256 {
        let mut c = i as u32;
        let mut k = 0;
        while k < 8 {
            c = if c & 1 != 0 { POLY ^ (c >> 1) } else { c >> 1 };
            k += 1;
        }
        table[i] = c;
        i += 1;
    }
    table
}

static TABLE: [u32; 256] = build_table();

/// CRC-32 增量计算器（zlib.crc32 语义：crc32_update(crc32_init(), data) == zlib.crc32(data)）
pub struct Crc32 {
    state: u32,
}

impl Crc32 {
    pub fn init() -> Self {
        Self { state: 0xFFFF_FFFF }
    }

    pub fn update(&mut self, data: &[u8]) {
        let t = &TABLE;
        let mut s = self.state;
        for &b in data {
            s = t[((s ^ b as u32) & 0xFF) as usize] ^ (s >> 8);
        }
        self.state = s;
    }

    pub fn final_crc(self) -> u32 {
        self.state ^ 0xFFFF_FFFF
    }
}

/// 一次性计算
pub fn crc32(data: &[u8]) -> u32 {
    let mut c = Crc32::init();
    c.update(data);
    c.final_crc()
}
