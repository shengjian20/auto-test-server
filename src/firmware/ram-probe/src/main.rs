//! ram-probe：二分排查第一步 = uart-echo 结构克隆 + 改名 banner
#![no_std]
#![no_main]

use embassy_gd32::{Pin, Port, Rcc, Uart};
use panic_halt as _;

const PCLK1_HZ: u32 = 16_000_000;
const BAUD: u32 = 115_200;

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

    uart.write(b"ram-probe minimal\r\n");

    /// 单点探针（运行期地址读写，探针本职；地址由本文件常量表限定）
    #[allow(unsafe_code)]
    fn probe(addr: u32, v: u32) -> u32 {
        unsafe {
            (addr as *mut u32).write_volatile(v);
            (addr as *const u32).read_volatile()
        }
    }

    // 64K 步进探针 0x20000000..0x20070000
    let mut addr = 0x2000_0000u32;
    while addr < 0x2007_0000u32 {
        let rd = probe(addr, 0x1234_5678);
        const HEX: &[u8; 16] = b"0123456789ABCDEF";
        uart.write(b"0x");
        for shift in [28, 24, 20, 16, 12, 8, 4, 0] {
            uart.write_byte(HEX[((addr >> shift) & 0xF) as usize]);
        }
        uart.write(b": ");
        uart.write(if rd == 0x1234_5678 { b"OK" } else { b"BAD" });
        uart.write(b"\r\n");
        addr += 0x1_0000;
    }
    uart.write(b"probe done\r\n");
    loop {
        core::hint::spin_loop();
    }
}
