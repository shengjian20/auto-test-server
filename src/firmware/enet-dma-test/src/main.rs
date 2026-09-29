//! enet-dma-test：MAC LBM 手工寄存器序列版（与 openocd 手工实验完全同源，
//! 用于判定"手工序列 vs HAL 初始化"差异是否为问题根因）
#![no_std]
#![no_main]

use embassy_gd32::{Pin, Port, Rcc, Uart};
use panic_halt as _;

const PCLK_HZ: u32 = 16_000_000;

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

/// 手工寄存器写（openocd mww 等价；地址/值由本文件常量限定）
#[allow(unsafe_code)]
fn hw_write(addr: u32, v: u32) {
    unsafe {
        (addr as *mut u32).write_volatile(v);
    }
}

#[allow(unsafe_code)]
fn hw_read(addr: u32) -> u32 {
    unsafe { (addr as *const u32).read_volatile() }
}

#[cortex_m_rt::entry]
fn main() -> ! {
    let p = gd32f470::Peripherals::take().expect("peripherals already taken");

    let rcc = Rcc::new(&p.rcu);
    rcc.enable_gpio_port(Port::E);
    rcc.enable_uart6();
    let _utx = Pin::alternate(&p.gpioe, 7, 8);
    let _urx = Pin::alternate(&p.gpioe, 8, 8);
    let uart = Uart::new(&p.uart6);
    uart.enable(PCLK_HZ, 115_200);
    uart.write(b"enet-dma manual-seq\r\n");

    // ==== openocd 手工序列 1:1 复刻（mww -> hw_write）====
    // 1. 时钟
    hw_write(0x4002_3830, 0x0210_0110); // AHB1: ENET*4 + GPIOA/C/E
    uart.write_byte(b'1');
    hw_write(0x4002_3844, 0x0001_0100); // APB2: SYSCFG + GPIO?
    uart.write_byte(b'2');
    hw_write(0x4002_3840, 0x4000_0000); // APB1: UART6
    uart.write_byte(b'3');
    // 2. SYSCFG_CFG1 RMII (bit23)
    hw_write(0x4002_1008, 0x0080_0000);
    uart.write_byte(b'4');
    // 3. PHY 复位脚 PC0
    hw_write(0x4002_1000, 0x0000_0001); // PC0 输出
    hw_write(0x4002_1004, 0x0000_0001); // PC0 高
    // 4. SWR
    hw_write(0x4002_9000, 0x0000_0001);
    uart.write_byte(b'5');
    for _ in 0..10 {
        delay_ms(10);
        uart.write_byte(b'.');
    }
    let bctl = hw_read(0x4002_9000);
    uart.write(b" BCTL=0x");
    put_hex8(&uart, bctl);
    uart.write(b"\r\n");
    // 5. DPSL=31
    hw_write(0x4002_9000, 0x0000_1F00);
    // 6. MAC_CFG: LBM|DPM|SPD
    hw_write(0x4002_8000, 0x0000_D800);
    // 7. 混杂
    hw_write(0x4002_8004, 0x0000_0001);
    // 8. MAC 地址
    hw_write(0x4002_8040, 0x0806_0402);
    hw_write(0x4002_8044, 0x0000_0C0A);
    // 9. TX 描述符 @0x20000418
    hw_write(0x2000_0418, 0xC130_0000);
    hw_write(0x2000_041C, 0x0000_0040);
    hw_write(0x2000_0420, 0x2000_2000);
    hw_write(0x2000_0424, 0x0000_0000);
    // 10. TX buf 首字
    hw_write(0x2000_2000, 0x5555_5555);
    // 11. RX 描述符 @0x20002230
    hw_write(0x2000_2230, 0x8000_0000);
    hw_write(0x2000_2234, 0x0000_45F4);
    hw_write(0x2000_2238, 0x2000_2280);
    hw_write(0x2000_223C, 0x0000_0000);
    // 12. 表地址
    hw_write(0x4002_900C, 0x2000_2230);
    hw_write(0x4002_9010, 0x2000_0418);
    // 13. MAC 收发使能
    hw_write(0x4002_8000, 0x0000_D80C);
    // 14. DMA_CTL: RSFD|TSFD|SRE|STE
    hw_write(0x4002_9018, 0x0220_2002);
    delay_ms(500);

    // 结果报告
    let stat = hw_read(0x4002_9014);
    let rdesc = hw_read(0x2000_2230);
    let tdesc = hw_read(0x2000_0418);
    let rbuf0 = hw_read(0x2000_2280);
    uart.write(b"BCTL=0x");
    put_hex8(&uart, bctl);
    uart.write(b" STAT=0x");
    put_hex8(&uart, stat);
    uart.write(b"\r\nRDESC0=0x");
    put_hex8(&uart, rdesc);
    uart.write(b" TDESC0=0x");
    put_hex8(&uart, tdesc);
    uart.write(b"\r\nRBUF=0x");
    put_hex8(&uart, rbuf0);
    uart.write(b"\r\n");
    uart.write(if rdesc & 0x8000_0000 == 0 {
        b"MANUAL_LBM: RX GOT FRAME\r\n"
    } else {
        b"MANUAL_LBM: NO RX\r\n"
    });

    loop {
        core::hint::spin_loop();
    }
}
