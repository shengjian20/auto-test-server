//! cpld-test：阶段 2f 验收固件——CPLD 协议回路（UART mux 读写往返 + EXIO 写应答）
//!
//! 事实链（carrier-box 实跑配置 core_cpld.c/drv_cpld.c/drv_cpld.h）：
//! - SPI2 @ PC10(SCK)/PC11(MISO)/PC12(MOSI) AF6，Mode0 MSB，16 位帧 500kHz
//! - CS=PA15（软件），RST=PA4（硬复位：低 10ms -> 高）
//! - 协议 V1.0：cmd 帧内嵌 0x55AA 应答；SET 写锁存低 3 位，GET 返回
//!   {13'd0, uart_sel[2:0]}
//! 验收序列：
//!   A. GET_UART（应答魔数 + 读复位值）
//!   B. SET_UART(EXUART0=0x81) -> GET_UART == 0x0001（写读往返）
//!   C. SET_UART(MCU=0x80) -> GET_UART == 0x0000（恢复默认）
//!   D. SET_EXIO(0x15)（写命令应答校验）
//! 结果经 UART6 console 输出，单轮后挂起。
#![no_std]
#![no_main]
#![deny(unsafe_code)]

use embassy_gd32::{cpld, Pin, Port, Rcc, Spi, Uart};
use panic_halt as _;

const PCLK1_HZ: u32 = 16_000_000;

fn delay_ms(ms: u32) {
    for _ in 0..ms {
        for _ in 0..4000 {
            core::hint::spin_loop();
        }
    }
}

fn put_hex16(uart: &Uart, v: u16) {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    for shift in [12, 8, 4, 0] {
        uart.write_byte(HEX[((v >> shift) & 0xF) as usize]);
    }
}

fn put_str(uart: &Uart, s: &str) {
    uart.write(s.as_bytes());
}

#[cortex_m_rt::entry]
fn main() -> ! {
    let p = gd32f470::Peripherals::take().expect("peripherals already taken");

    let rcc = Rcc::new(&p.rcu);
    rcc.enable_gpio_port(Port::A); // CS=PA15 / RST=PA4
    rcc.enable_gpio_port(Port::C); // SPI2 引脚
    rcc.enable_gpio_port(Port::E); // UART6 引脚
    rcc.enable_spi2();
    rcc.enable_uart6();

    // SPI2 引脚：PC10=SCK / PC11=MISO / PC12=MOSI（AF6）
    let _sck = Pin::alternate(&p.gpioc, 10, 6);
    let _miso = Pin::alternate(&p.gpioc, 11, 6);
    let _mosi = Pin::alternate(&p.gpioc, 12, 6);

    // CS=PA15 / RST=PA4（GPIO 输出）
    let mut cs = Pin::output(&p.gpioa, 15);
    cs.set_high();
    let mut rst = Pin::output(&p.gpioa, 4);
    rst.set_high();

    // UART6 console：PE7/PE8（AF8）
    let _utx = Pin::alternate(&p.gpioe, 7, 8);
    let _urx = Pin::alternate(&p.gpioe, 8, 8);
    let uart = Uart::new(&p.uart6);
    uart.enable(PCLK1_HZ, 115_200);
    put_str(&uart, "cpld-test\r\n");

    // CPLD 硬复位 + SPI 使能
    cpld::Cpld::hard_reset(&mut rst);
    delay_ms(5);
    let spi = Spi::new(&p.spi2);
    spi.enable_master(PCLK1_HZ, 500_000);
    let mut cpld = cpld::Cpld::new(spi, cs);

    let mut result = "PASS";

    // A. GET_UART 复位值（校验 0x55AA 应答魔数存在性）
    match cpld.get_uart_mux() {
        Ok(v) => {
            put_str(&uart, "A get_uart(rst): 0x");
            put_hex16(&uart, v);
            put_str(&uart, "\r\n");
        }
        Err(cpld::CpldError::BadResponse(r)) => {
            put_str(&uart, "A get_uart: BADRESP 0x");
            put_hex16(&uart, r);
            put_str(&uart, "\r\n");
            result = "FAIL";
        }
    }

    // B. SET_UART(EXUART0) -> GET == 1（写读往返）
    {
        if cpld.set_uart_mux(cpld::TO_EXUART0).is_err() {
            put_str(&uart, "B set EXUART0: ERR\r\n");
            result = "FAIL";
        } else {
            delay_ms(2);
            match cpld.get_uart_mux() {
                Ok(0x0001) => put_str(&uart, "B set/get EXUART0: MATCH\r\n"),
                Ok(v) => {
                    put_str(&uart, "B get: 0x");
                    put_hex16(&uart, v);
                    put_str(&uart, " (expect 0x0001) MISMATCH\r\n");
                    result = "FAIL";
                }
                Err(_) => {
                    put_str(&uart, "B get: ERR\r\n");
                    result = "FAIL";
                }
            }
        }
    }

    // C. 恢复 MCU_UART -> GET == 0
    {
        if cpld.set_uart_mux(cpld::TO_MCU_UART).is_err() {
            put_str(&uart, "C set MCU: ERR\r\n");
            result = "FAIL";
        } else {
            delay_ms(2);
            match cpld.get_uart_mux() {
                Ok(0x0000) => put_str(&uart, "C restore MCU: MATCH\r\n"),
                Ok(v) => {
                    put_str(&uart, "C get: 0x");
                    put_hex16(&uart, v);
                    put_str(&uart, " (expect 0x0000) MISMATCH\r\n");
                    result = "FAIL";
                }
                Err(_) => {
                    put_str(&uart, "C get: ERR\r\n");
                    result = "FAIL";
                }
            }
        }
    }

    // D. SET_EXIO 写命令应答校验（GPO pattern 0x15，外部可观测）
    {
        if cpld.set_exio(0x15).is_err() {
            put_str(&uart, "D set_exio: ERR\r\n");
            result = "FAIL";
        } else {
            put_str(&uart, "D set_exio(0x15): resp OK\r\n");
        }
    }

    if result == "PASS" {
        put_str(&uart, "CPLD_TEST: PASS\r\n");
    } else {
        put_str(&uart, "CPLD_TEST: FAIL\r\n");
    }

    loop {
        core::hint::spin_loop();
    }
}
