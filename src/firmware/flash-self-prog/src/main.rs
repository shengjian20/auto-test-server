//! flash-self-prog：片上 FLASH 自编程验收（阶段 5 W25Q 升级通道前提）
//!
//! 流程：解锁 FMC -> 擦除扇区 11（0x080E0000，128K，远离 BL/应用区）->
//! 空白校验（全 FF）-> 64 字图案编程（带读回校验）-> 读回比对 ->
//! 上锁 -> 结果经 UART6 console 报告。单轮运行防闪存磨损
//! （验收重放靠"先开串口再复位"）。
#![no_std]
#![no_main]
#![deny(unsafe_code)]

use embassy_gd32::flash::{Flash, SECTOR_BASE};
use embassy_gd32::{Pin, Port, Rcc, Uart};
use panic_halt as _;

const PCLK1_HZ: u32 = 16_000_000;
const BAUD: u32 = 115_200;

fn put_str(uart: &Uart, s: &str) {
    uart.write(s.as_bytes());
}

fn put_hex8(uart: &Uart, v: u32) {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    for i in 0..8 {
        uart.write_byte(HEX[((v >> (28 - i * 4)) & 0xF) as usize]);
    }
}

#[cortex_m_rt::entry]
fn main() -> ! {
    let p = gd32f470::Peripherals::take().expect("peripherals already taken");

    let rcc = Rcc::new(&p.rcu);
    rcc.enable_gpio_port(Port::E);
    rcc.enable_uart6();

    let _tx = Pin::alternate(&p.gpioe, 7, 8);
    let _rx = Pin::alternate(&p.gpioe, 8, 8);

    let uart = Uart::new(&p.uart6);
    uart.enable(PCLK1_HZ, BAUD);

    put_str(&uart, "flash-self-prog v1\r\n");

    const TEST_SECTOR: u8 = 11;
    const TEST_ADDR: u32 = SECTOR_BASE[11];
    const WORDS: usize = 64;

    let flash = Flash::new();
    let mut pass = 0u8;
    let mut fail = 0u8;

    // 1. 解锁 + 扇区擦除
    flash.unlock();
    match flash.erase_sector(TEST_SECTOR) {
        Ok(_) => {
            put_str(&uart, "ERASE OK\r\n");
            pass += 1;
        }
        Err(_) => {
            put_str(&uart, "ERASE FAIL\r\n");
            fail += 1;
        }
    }

    // 2. 空白校验（全 FF）
    let mut blank_ok = true;
    for i in 0..WORDS {
        match flash.read_word(TEST_ADDR + (i as u32) * 4) {
            Ok(v) if v == 0xFFFF_FFFF => {}
            _ => blank_ok = false,
        }
    }
    if blank_ok {
        put_str(&uart, "BLANK OK\r\n");
        pass += 1;
    } else {
        put_str(&uart, "BLANK FAIL\r\n");
        fail += 1;
    }

    // 3. 图案编程（0xA55A0000 ^ 序号扩展，逐字读回校验在 HAL 内）
    let mut prog_ok = true;
    for i in 0..WORDS {
        let w = 0xA55A_0000 ^ ((i as u32).wrapping_mul(0x0101_0101));
        if flash.program_word(TEST_ADDR + (i as u32) * 4, w).is_err() {
            prog_ok = false;
            put_str(&uart, "PROG ERR @");
            put_hex8(&uart, i as u32);
            put_str(&uart, "\r\n");
            break;
        }
    }
    if prog_ok {
        put_str(&uart, "PROG OK (64 words)\r\n");
        pass += 1;
    } else {
        fail += 1;
    }

    // 4. 读回比对
    let mut match_ok = true;
    for i in 0..WORDS {
        let expect = 0xA55A_0000 ^ ((i as u32).wrapping_mul(0x0101_0101));
        match flash.read_word(TEST_ADDR + (i as u32) * 4) {
            Ok(v) if v == expect => {}
            Ok(v) => {
                put_str(&uart, "MISMATCH @");
                put_hex8(&uart, i as u32);
                put_str(&uart, " got ");
                put_hex8(&uart, v);
                put_str(&uart, "\r\n");
                match_ok = false;
                break;
            }
            Err(_) => {
                match_ok = false;
                break;
            }
        }
    }
    if match_ok {
        put_str(&uart, "MATCH OK (64/64)\r\n");
        pass += 1;
    } else {
        fail += 1;
    }

    flash.lock();

    put_str(&uart, "RESULT: ");
    if fail == 0 && pass == 4 {
        put_str(&uart, "FLASH_SELFPROG PASS\r\n");
    } else {
        put_str(&uart, "FLASH_SELFPROG FAIL\r\n");
    }

    loop {
        core::hint::spin_loop();
    }
}
