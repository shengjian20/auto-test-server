//! flash-rw：阶段 2d 验收固件——W25Q128 扇区擦除/页编程/读回比对
//!
//! 硬件事实（阶段 2c 已定案）：SPI3 @ PE2(SCK)/PE5(MOSI)/PE6(MISO) AF5，
//! CS=PE4，JEDEC EF 40 18 = W25Q128（16MB，页 256B，扇区 4KB）。
//! 结果经 UART6 console（PE7/PE8 AF8 115200）输出。
//!
//! 测试地址 0x00F00000（16MB 尾部区域，避开低地址可能的表区）。
//! 单轮运行后挂起（防闪存磨损：扇区擦写寿命 10 万次，周期重跑会磨）；
//! PC 验收 = 先开串口 -> probe-rs reset -> 收结果。
#![no_std]
#![no_main]
#![deny(unsafe_code)]

use embassy_gd32::{Pin, Port, Rcc, Spi, Uart};
use panic_halt as _;

const PCLK_HZ: u32 = 16_000_000;
const TEST_ADDR: u32 = 0x00F0_0000;
const PAGE: usize = 256;

// W25Q 标准命令集（W25Q128 数据手册）
const CMD_WRITE_ENABLE: u8 = 0x06;
const CMD_READ_DATA: u8 = 0x03;
const CMD_PAGE_PROGRAM: u8 = 0x02;
const CMD_SECTOR_ERASE: u8 = 0x20;
const CMD_READ_STATUS: u8 = 0x05;

fn delay_ms(ms: u32) {
    for _ in 0..ms {
        for _ in 0..4000 {
            core::hint::spin_loop();
        }
    }
}

fn put_hex(uart: &Uart, v: u8) {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    uart.write_byte(HEX[(v >> 4) as usize]);
    uart.write_byte(HEX[(v & 0xF) as usize]);
}

fn put_u32(uart: &Uart, v: u32) {
    put_hex(uart, (v >> 24) as u8);
    put_hex(uart, (v >> 16) as u8);
    put_hex(uart, (v >> 8) as u8);
    put_hex(uart, v as u8);
}

// ---- W25Q 命令层 ----

fn write_enable(spi: &Spi, cs: &mut Pin) {
    cs.set_low();
    spi.write(&[CMD_WRITE_ENABLE]);
    cs.set_high();
}

fn wait_ready(spi: &Spi, cs: &mut Pin) {
    loop {
        cs.set_low();
        spi.write(&[CMD_READ_STATUS]);
        let st = spi.transfer(0xFF);
        cs.set_high();
        if st & 1 == 0 {
            return; // WIP=0
        }
        delay_ms(1);
    }
}

fn erase_sector(spi: &Spi, cs: &mut Pin, addr: u32) {
    write_enable(spi, cs);
    cs.set_low();
    spi.write(&[
        CMD_SECTOR_ERASE,
        (addr >> 16) as u8,
        (addr >> 8) as u8,
        addr as u8,
    ]);
    cs.set_high();
    wait_ready(spi, cs); // W25Q128 扇区擦 typ 30-60ms，max 400ms
}

fn page_program(spi: &Spi, cs: &mut Pin, addr: u32, data: &[u8]) {
    write_enable(spi, cs);
    cs.set_low();
    spi.write(&[
        CMD_PAGE_PROGRAM,
        (addr >> 16) as u8,
        (addr >> 8) as u8,
        addr as u8,
    ]);
    for &b in data {
        spi.transfer(b);
    }
    cs.set_high();
    wait_ready(spi, cs); // 页编程 typ 0.4-3ms
}

fn read_data(spi: &Spi, cs: &mut Pin, addr: u32, buf: &mut [u8]) {
    cs.set_low();
    spi.write(&[
        CMD_READ_DATA,
        (addr >> 16) as u8,
        (addr >> 8) as u8,
        addr as u8,
    ]);
    spi.read(buf);
    cs.set_high();
}

#[cortex_m_rt::entry]
fn main() -> ! {
    let p = gd32f470::Peripherals::take().expect("peripherals already taken");

    let rcc = Rcc::new(&p.rcu);
    rcc.enable_gpio_port(Port::E);
    rcc.enable_spi3();
    rcc.enable_uart6();

    // UART6 console：PE7=TX / PE8=RX（AF8）
    let _tx = Pin::alternate(&p.gpioe, 7, 8);
    let _rx = Pin::alternate(&p.gpioe, 8, 8);
    let uart = Uart::new(&p.uart6);
    uart.enable(PCLK_HZ, 115_200);

    // SPI3：SCK=PE2 / MOSI=PE5 / MISO=PE6（AF5），CS=PE4（软件 GPIO）
    let _sck = Pin::alternate(&p.gpioe, 2, 5);
    let _mosi = Pin::alternate(&p.gpioe, 5, 5);
    let _miso = Pin::alternate(&p.gpioe, 6, 5);
    let mut cs = Pin::output(&p.gpioe, 4);
    cs.set_high();

    let spi = Spi::new(&p.spi3);
    spi.enable_master(PCLK_HZ, 2_000_000);

    uart.write(b"flash-rw @0x");
    put_u32(&uart, TEST_ADDR);
    uart.write(b"\r\n");

    // 已知 pattern：地址低位异或掩码（无平铺重复，能测出移位类错误）
    let mut wr = [0u8; PAGE];
    for (i, b) in wr.iter_mut().enumerate() {
        *b = (i as u8) ^ 0xA5;
    }
    let mut rd = [0u8; PAGE];

    // 1. 擦除（4K 扇区）
    erase_sector(&spi, &mut cs, TEST_ADDR);
    uart.write(b"erase: done\r\n");

    // 1a. 擦除后验证全 0xFF（flash 擦除态）
    read_data(&spi, &mut cs, TEST_ADDR, &mut rd);
    let erased = rd.iter().all(|&b| b == 0xFF);
    uart.write(b"erased-blank: ");
    uart.write(if erased { b"OK" } else { b"BAD" });
    uart.write(b"\r\n");

    // 2. 页编程
    page_program(&spi, &mut cs, TEST_ADDR, &wr);
    uart.write(b"program: done\r\n");

    // 3. 读回比对
    read_data(&spi, &mut cs, TEST_ADDR, &mut rd);
    let mut mismatches = 0u32;
    let mut first_bad: usize = PAGE;
    for i in 0..PAGE {
        if rd[i] != wr[i] {
            mismatches += 1;
            if first_bad == PAGE {
                first_bad = i;
            }
        }
    }

    if mismatches == 0 {
        uart.write(b"verify: PASS (256/256)\r\n");
        uart.write(b"FLASH_RW: PASS\r\n");
    } else {
        uart.write(b"verify: FAIL mismatches=");
        put_u32(&uart, mismatches);
        uart.write(b" first@");
        put_u32(&uart, first_bad as u32);
        uart.write(b"\r\n");
        uart.write(b"FLASH_RW: FAIL\r\n");
    }

    // 单轮运行后挂起（防闪存磨损）
    loop {
        core::hint::spin_loop();
    }
}
