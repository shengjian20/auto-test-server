//! flash-identify：阶段 2 验收固件——SPI3 读 W25Q JEDEC ID，结果经 UART6 console 输出
//!
//! 事实链（carrier-box 地面真值，实跑配置 drv_spi.c + drv_spi_flash.c）：
//! spi_bus3 = SPI3 @ GPIOE AF5：SCK=PE2 / MOSI=PE5 / MISO=PE6；
//! flash 片选 CS = PE4（GPIO 输出）。
//! （原理图文本层列错位曾误导为 PE3=CS/PE4=MISO，导致 MISO 读到被配成
//! CS 输出的 PE4 -> JEDEC 全 0；以实跑固件配置为准。）
//!
//! 型号：原理图网络名叫 W25Q256，carrier-box .config 实跑 W25Q128BJ——
//! 以板上实测 JEDEC 为准。
//!
//! bring-up 形态：周期性输出（1s 间隔），避免依赖抓上电 banner；
//! "boot" 标记在 UART 初始化后立刻输出，用于区分早期挂起阶段。
#![no_std]
#![no_main]
#![deny(unsafe_code)]

use embassy_gd32::{Pin, Port, Rcc, Spi, Uart};
use panic_halt as _;

const PCLK_HZ: u32 = 16_000_000;

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

#[cortex_m_rt::entry]
fn main() -> ! {
    let p = gd32f470::Peripherals::take().expect("peripherals already taken");

    let rcc = Rcc::new(&p.rcu);
    rcc.enable_gpio_port(Port::E);
    rcc.enable_uart6();

    // UART6 引脚：PE7=TX / PE8=RX（AF8）——周期版重写时曾漏配导致静默
    // （UART6 外设寄存器全对但 PE7/PE8 停在复位态 AF0，TX 进悬空引脚）
    let _tx = Pin::alternate(&p.gpioe, 7, 8);
    let _rx = Pin::alternate(&p.gpioe, 8, 8);

    let uart = Uart::new(&p.uart6);
    uart.enable(PCLK_HZ, 115_200);
    uart.write(b"boot\r\n");

    // SPI3：SCK=PE2 / MOSI=PE5 / MISO=PE6（AF5），CS=PE4（软件 GPIO）
    rcc.enable_spi3();
    let _sck = Pin::alternate(&p.gpioe, 2, 5);
    let _mosi = Pin::alternate(&p.gpioe, 5, 5);
    let _miso = Pin::alternate(&p.gpioe, 6, 5);
    let mut cs = Pin::output(&p.gpioe, 4);
    cs.set_high();

    let spi = Spi::new(&p.spi3);
    spi.enable_master(PCLK_HZ, 2_000_000);
    uart.write(b"spi-init done\r\n");

    loop {
        // JEDEC ID（0x9F）：制造商 + 存储类型 + 容量
        cs.set_low();
        spi.write(&[0x9F]);
        let id0 = spi.transfer(0xFF);
        let id1 = spi.transfer(0xFF);
        let id2 = spi.transfer(0xFF);
        cs.set_high();

        uart.write(b"JEDEC: ");
        put_hex(&uart, id0);
        uart.write_byte(b' ');
        put_hex(&uart, id1);
        uart.write_byte(b' ');
        put_hex(&uart, id2);
        uart.write(b"\r\n");

        if id0 == 0xEF && id2 == 0x18 {
            uart.write(b"chip: W25Q128 (16MB)\r\n");
        } else if id0 == 0xEF && id2 == 0x19 {
            uart.write(b"chip: W25Q256 (32MB)\r\n");
        }

        delay_ms(1000);
    }
}
