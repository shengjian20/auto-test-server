//! bootloader v2：W25Q 升级通道——镜像头校验（magic+size+CRC32）+
//! 应用区搬运 + 跳转（阶段 5 OTA 的引导侧）
//!
//! 升级协议（W25Q 槽位布局 @0x000000）：
//!   [0..8)   magic "GDOTA001"
//!   [8..12)  size  u32 LE（应用镜像字节数，不含头）
//!   [12..16) crc32 u32 LE（应用镜像的 CRC-32/ISO-HDLC，zlib 兼容）
//!   [16..)   应用镜像（向量表 + .text + .data init，链接 @0x08008000）
//!
//! 启动决策树：
//!   W25Q 头有效 且 应用区 CRC 不匹配 -> 擦扇区2-3（应用区 32K）->
//!   搬运 -> 复算 CRC -> 一致则跳转（升级完成）；CRC 仍不符则停机报错
//!   W25Q 头有效 且 应用区 CRC 匹配 -> 直接跳转（无升级）
//!   W25Q 头无效 -> 按应用区向量表合法性跳转（无升级需求的常规启动）
//!
//! unsafe 收敛：跳转/VTOR/MSP 为裸金属本质操作（逐点注释，沿 v1）；
//! 搬运的 flash 编程全部经 HAL flash 模块安全 API（FMC 已板上验收）。
#![no_std]
#![no_main]

use embassy_gd32::crc32::Crc32;
use embassy_gd32::flash::{Flash, SECTOR_BASE};
use embassy_gd32::w25q::W25q;
use embassy_gd32::{Pin, Port, Rcc, Spi, Uart};
use panic_halt as _;

const APP_ADDR: u32 = 0x0800_8000;
/// 应用区容量（扇区 2-3：0x08008000..0x08010000，32K）
const APP_MAX: u32 = 0x0000_8000;
const RAM_BASE: u32 = 0x2000_0000;
const RAM_END: u32 = 0x2007_0000;
const MAGIC: &[u8; 8] = b"GDOTA001";
const HDR_SIZE: u32 = 16;

const W25Q_SLOT: u32 = 0x0000_0000;

fn delay_ms(ms: u32) {
    for _ in 0..ms {
        for _ in 0..4000 {
            core::hint::spin_loop();
        }
    }
}

fn put_hex8(uart: &Uart, v: u32) {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    for shift in [28, 24, 20, 16, 12, 8, 4, 0] {
        uart.write_byte(HEX[((v >> shift) & 0xF) as usize]);
    }
}

#[allow(unsafe_code)]
fn read_u32(addr: u32) -> u32 {
    unsafe { core::ptr::read_volatile(addr as *const u32) }
}

/// 读 W25Q 槽位头并校验 magic/size/CRC（size<=APP_MAX 且 CRC 全像复算一致）
fn read_slot_header(w25q: &mut W25q, uart: &Uart) -> Option<(u32, u32)> {
    let mut hdr = [0u8; 16];
    w25q.read(W25Q_SLOT, &mut hdr);
    if &hdr[0..8] != MAGIC {
        uart.write(b"ota: no magic\r\n");
        return None;
    }
    let size = u32::from_le_bytes([hdr[8], hdr[9], hdr[10], hdr[11]]);
    let crc = u32::from_le_bytes([hdr[12], hdr[13], hdr[14], hdr[15]]);
    if size == 0 || size > APP_MAX {
        uart.write(b"ota: bad size\r\n");
        return None;
    }
    // 全像 CRC 复算（分块流式，256B 步进）
    let mut c = Crc32::init();
    let mut buf = [0u8; 256];
    let mut off = HDR_SIZE;
    let end = HDR_SIZE + size;
    while off < end {
        let n = core::cmp::min(256, (end - off) as usize);
        w25q.read(W25Q_SLOT + off, &mut buf[..n]);
        c.update(&buf[..n]);
        off += n as u32;
    }
    let calc = c.final_crc();
    if calc != crc {
        uart.write(b"ota: crc mismatch slot=0x");
        put_hex8(&uart, crc);
        uart.write(b" calc=0x");
        put_hex8(&uart, calc);
        uart.write(b"\r\n");
        return None;
    }
    Some((size, crc))
}

/// 计算应用区（0x08008000 起 size 字节）的 CRC
fn app_region_crc(size: u32) -> u32 {
    let mut c = Crc32::init();
    let mut off = 0u32;
    while off < size {
        let w = read_u32(APP_ADDR + off);
        let bytes = w.to_le_bytes();
        let n = core::cmp::min(4, (size - off) as usize);
        c.update(&bytes[..n]);
        off += 4;
    }
    c.final_crc()
}

#[cortex_m_rt::entry]
fn main() -> ! {
    let p = gd32f470::Peripherals::take().expect("peripherals already taken");

    let rcc = Rcc::new(&p.rcu);
    rcc.enable_gpio_port(Port::E);
    rcc.enable_uart6(); // UART6 APB1 时钟门——缺失则寄存器写入静默丢弃（console 全静默的根因，与 TIMER1 时钟同款坑）
    let _utx = Pin::alternate(&p.gpioe, 7, 8);
    let _urx = Pin::alternate(&p.gpioe, 8, 8);
    let uart = Uart::new(&p.uart6);
    uart.enable(16_000_000, 115_200);
    uart.write(b"bootloader v3 (serial-trigger OTA)\r\n");

    // SPI3 Flash 总线（W25Q128）
    rcc.enable_spi3();
    let _fsck = Pin::alternate(&p.gpioe, 2, 5);
    let _fmosi = Pin::alternate(&p.gpioe, 5, 5);
    let _fmiso = Pin::alternate(&p.gpioe, 6, 5);
    let mut fcs = Pin::output(&p.gpioe, 4);
    fcs.set_high();
    let fspi = Spi::new(&p.spi3);
    fspi.enable_master(16_000_000, 2_000_000);

    let mut w25q = W25q::new(fspi, fcs);

    // 串口触发窗口（500ms）：收 "UPGR" 魔数 -> 升级模式（行协议收镜像，
    // 命令形态与 control-server 的 flash se/wr/crc 一致——PC 侧工具链
    // 零改动复用）；超时无触发 -> 常规路径（W25Q 校验搬运/直接跳转）
    uart.write(b"UPGR?\r\n");
    let mut trigger = [0u8; 4];
    let mut got = 0usize;
    // 触发窗口：确定性递减计数（spin_loop 忙等会被 O1 优化空转——教训
    // #15）。guard 按实测校准：read_byte 每轮 volatile 读 STAT0+可选读
    // DATA ≈ 20+ cycles/迭代，128M 计数 ≈ 4-6s 实际窗口（PC 侧 spray
    // 连发 UPGR，收齐 4 字节立即触发）
    let mut guard: u64 = 128_000_000;
    while guard > 0 && got < 4 {
        if let Some(b) = uart.read_byte() {
            trigger[got] = b;
            got += 1;
        } else {
            guard = guard.wrapping_sub(24);
        }
    }
    if got == 4 && &trigger == b"UPGR" {
        uart.write(b"upgrade mode\r\n");
        serial_upgrade(&uart, &mut w25q);
        // 升级模式以 boot 命令结束（软复位回常规路径校验搬运）
        uart.write(b"upgrade done, rebooting\r\n");
        delay_ms(100);
        cortex_m::peripheral::SCB::sys_reset();
    }

    let slot = read_slot_header(&mut w25q, &uart);

    if let Some((size, crc)) = slot {
        // 槽位有效：比对应用区 CRC，决定是否搬运
        let app_crc = app_region_crc(size);
        if app_crc == crc {
            uart.write(b"ota: app up-to-date\r\n");
        } else {
            uart.write(b"ota: flashing app (");
            put_hex8(&uart, size);
            uart.write(b" bytes)\r\n");
            let flash = Flash::new();
            flash.unlock();
            // 应用区 = 扇区 2、3（0x08008000/0x0800C000，各 16K）
            let mut erased = true;
            for s in 2..=3u8 {
                if flash.erase_sector(s).is_err() {
                    erased = false;
                    break;
                }
            }
            if !erased {
                uart.write(b"ota: erase FAIL\r\n");
                error_halt(&uart);
            }
            // 分块搬运：W25Q 读 256B -> FMC 字编程
            let mut buf = [0u8; 256];
            let mut off = 0u32;
            while off < size {
                let n = core::cmp::min(256, (size - off) as usize);
                w25q.read(W25Q_SLOT + HDR_SIZE + off, &mut buf[..n]);
                // 尾块补 FF 对齐到字
                let words = (n + 3) / 4;
                for wi in 0..words {
                    let mut w = [0xFFu8; 4];
                    for bi in 0..4 {
                        let idx = wi * 4 + bi;
                        if idx < n {
                            w[bi] = buf[idx];
                        }
                    }
                    let word = u32::from_le_bytes(w);
                    if flash.program_word(APP_ADDR + off + (wi as u32) * 4, word).is_err() {
                        uart.write(b"ota: program FAIL\r\n");
                        error_halt(&uart);
                    }
                }
                off += n as u32;
            }
            flash.lock();
            let check = app_region_crc(size);
            if check != crc {
                uart.write(b"ota: post-copy crc FAIL\r\n");
                error_halt(&uart);
            }
            uart.write(b"ota: copy verified\r\n");
        }
    }

    // 应用合法性检查（无升级/升级完成后共用）
    let sp = read_u32(APP_ADDR);
    let rv = read_u32(APP_ADDR + 4);
    if sp < RAM_BASE || sp > RAM_END || rv & 1 == 0 {
        uart.write(b"app invalid: SP=0x");
        put_hex8(&uart, sp);
        uart.write(b" RST=0x");
        put_hex8(&uart, rv);
        uart.write(b"\r\n");
        error_halt(&uart);
    }

    uart.write(b"app OK: SP=0x");
    put_hex8(&uart, sp);
    uart.write(b" RST=0x");
    put_hex8(&uart, rv & !1);
    uart.write(b" -> jumping\r\n");
    delay_ms(100);

    // bx 消费目标的 LSB 作为 T 位（thumb 状态）——必须保留原始 thumb 位。
    // 曾误写 rv & !1（清位）-> bx 偶地址 -> INVSTATE -> HardFault -> 应用
    // 向量表的 HardFault handler 自环（VTOR 已先重定向）-> 全静默（板上
    // 实证：异常帧 fault PC=0x080028F2 即 bx 指令）
    jump(sp, rv)
}

/// 升级模式：行协议收镜像到 W25Q（命令形态与 control-server flash
/// se/wr/crc 一致，PC 侧工具链零改动复用）。收 "boot" 行后返回（调用方
/// 软复位走常规校验搬运路径）。
fn serial_upgrade(uart: &Uart, w25q: &mut W25q) {
    const LINE_MAX: usize = 440; // 80B 数据 = 160 hex 字符 + 命令头
    let mut line = [0u8; LINE_MAX];
    let mut n = 0usize;

    // 极简 hex 解析（bootloader 内联，不依赖 control-server）
    fn hex_val(c: u8) -> Option<u8> {
        match c {
            b'0'..=b'9' => Some(c - b'0'),
            b'a'..=b'f' => Some(c - b'a' + 10),
            b'A'..=b'F' => Some(c - b'A' + 10),
            _ => None,
        }
    }
    fn parse_hex_bytes(t: &[u8], buf: &mut [u8]) -> Option<usize> {
        if t.is_empty() || t.len() % 2 != 0 || t.len() > buf.len() * 2 {
            return None;
        }
        for i in (0..t.len()).step_by(2) {
            buf[i / 2] = (hex_val(t[i])? << 4) | hex_val(t[i + 1])?;
        }
        Some(t.len() / 2)
    }
    fn parse_u32_hex(t: &[u8]) -> Option<u32> {
        if t.is_empty() || t.len() > 6 {
            return None;
        }
        let mut v: u32 = 0;
        for &c in t {
            v = (v << 4) + hex_val(c)? as u32;
        }
        Some(v)
    }

    loop {
        match uart.read_byte() {
            Some(b'\n') | Some(b'\r') => {
                if n == 0 {
                    continue;
                }
                let mut toks: [&[u8]; 12] = [b""; 12];
                // 内联 tokenize（bootloader 独立 workspace，不依赖 control-server）
                let mut nt = 0usize;
                let mut i = 0usize;
                while i < n && nt < 12 {
                    while i < n && line[i] == b' ' { i += 1; }
                    if i >= n { break; }
                    let start = i;
                    while i < n && line[i] != b' ' { i += 1; }
                    toks[nt] = &line[start..i];
                    nt += 1;
                }
                // 命令回显（诊断：PC 侧按序比对，同时暴露 spray 污染）
                uart.write(b"[");
                uart.write(&line[..n]);
                uart.write(b"]\r\n");
                let resp = if nt == 2 && toks[0] == b"se" {
                    if let Some(addr) = parse_u32_hex(toks[1]) {
                        if addr % 4096 == 0 {
                            w25q.erase_sector(addr);
                            "OK\r\n"
                        } else {
                            "ERR align\r\n"
                        }
                    } else {
                        "ERR addr\r\n"
                    }
                } else if nt >= 3 && toks[0] == b"wr" {
                    let mut data = [0u8; 128];
                    if let (Some(addr), Some(k)) = (parse_u32_hex(toks[1]), parse_hex_bytes(toks[2], &mut data)) {
                        // 多 token hex 合并（wr <addr> <hex..>）
                        let mut total = k;
                        for t in toks[3..nt].iter() {
                            if let Some(m) = parse_hex_bytes(t, &mut data[total..]) {
                                total += m;
                            } else {
                                total = 0;
                                break;
                            }
                        }
                        if total > 0 {
                            w25q.write(addr, &data[..total]);
                            "OK\r\n"
                        } else {
                            "ERR data\r\n"
                        }
                    } else {
                        "ERR addr\r\n"
                    }
                } else if nt == 3 && toks[0] == b"crc" {
                    use embassy_gd32::crc32::Crc32;
                    if let (Some(addr), Some(len)) = (parse_u32_hex(toks[1]), parse_u32_hex(toks[2]).map(|v| v as usize)) {
                        if (1..=32768).contains(&len) {
                            let mut c = Crc32::init();
                            let mut buf = [0u8; 256];
                            let mut off = 0usize;
                            while off < len {
                                let k = core::cmp::min(256, len - off);
                                w25q.read(addr + off as u32, &mut buf[..k]);
                                c.update(&buf[..k]);
                                off += k;
                            }
                            uart.write(b"OK ");
                            put_hex8(uart, c.final_crc());
                            "\r\n"
                        } else {
                            "ERR len\r\n"
                        }
                    } else {
                        "ERR arg\r\n"
                    }
                } else if nt >= 1 && toks[0] == b"boot" {
                    return; // 调用方软复位
                } else {
                    "ERR cmd\r\n"
                };
                uart.write(resp.as_bytes());
                n = 0;
            }
            Some(ch) if ch != b'\r' => {
                if n < LINE_MAX {
                    line[n] = ch;
                    n += 1;
                }
            }
            Some(_) => {}
            None => {}
        }
    }
}

/// 错误停机：console 提示 + 停机循环（LED_1 常亮=低电平，可目视区分）
fn error_halt(uart: &Uart) -> ! {
    uart.write(b"bootloader: HALT\r\n");
    loop {
        core::hint::spin_loop();
    }
}

/// 跳转到应用（unsafe 收敛点：裸金属跳转的本质操作，沿 v1 已验证序列）。
///
/// unsafe 依据：合法性检查已验证 SP 在主 SRAM 区、reset 向量带 thumb 位；
/// 跳转前关中断 + VTOR 重定向 + MSP 重载，序列来自 cortex-m-rt 官方
/// 跳转范式。跳转后控制权完全移交应用。
#[allow(unsafe_code)]
fn jump(sp: u32, reset: u32) -> ! {
    unsafe {
        // 1. 关中断（PRIMASK=1），避免跳转过程中断打在旧向量上
        core::arch::asm!("cpsid i");
        // 2. VTOR 重定向到应用向量表
        (0xE000_ED08u32 as *mut u32).write_volatile(APP_ADDR);
        core::arch::asm!("dsb", "isb");
        // 3. MSP 重载为应用初始 SP
        core::arch::asm!(
            "msr msp, {sp}",
            sp = in(reg) sp,
        );
        // 4. 恢复中断（PRIMASK=0）——跳转不是复位，硬件不会自动清 PRIMASK；
        //    应用（embassy Ticker/UART6 RXNE 环）依赖中断，PRIMASK 残留 =
        //    应用首个 await 永挂（实测踩坑：OTA 应用跳转后 ping 无响应、
        //    全静默）
        core::arch::asm!("cpsie i");
        // 5. 清流水线后跳转（BX 带 thumb 位）
        core::arch::asm!("dsb", "isb", "bx {r}", r = in(reg) reset, options(noreturn));
    }
}
