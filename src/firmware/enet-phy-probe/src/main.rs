//! enet-phy-probe：阶段 3 第一个闸门——MDIO 扫描读 LAN8720A PHY ID
//!
//! 验证链（每个环节独立可判）：
//! 1. SYSCFG_CFG1.ENETPHYSEL=1（RMII 模式选择，须在 ENET 复位前完成）
//! 2. RCU 时钟：GPIOA/B/C + SYSCFGEN(APB2) + ENETTXE/ENETRXE/ENETPTPE/ENETE(AHB1)
//! 3. ETH_nRST=PC0 复位 LAN8720A（低 10ms -> 高 -> 等 50ms PHY 启动）
//! 4. RMII 引脚组（全 AF11）：PA1=REF_CLK / PA2=MDIO / PA7=CRS_DV /
//!    PC1=MDC / PC4=RXD0 / PC5=RXD1 / PB11=TX_EN / PB12=TXD0 / PB13=TXD1
//! 5. ENET_DMA_BCTL.SWR 软复位——REF_CLK(50MHz) 存活时 SWR 自动清零，
//!    卡 1 = REF_CLK 死线（PHY 晶振/供电/走线问题的直接证据）
//! 6. MDIO 扫描 32 个 PHY 地址：读 REG2/REG3（PHY ID1/ID2）。
//!    LAN8720A 期望 ID1=0x0007（SMSC）
//! 结果经 UART6 console（PE7/PE8 AF8 115200）输出，单轮后挂起。
#![no_std]
#![no_main]
#![deny(unsafe_code)]

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

fn put_hex4(uart: &Uart, v: u16) {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    for shift in [12, 8, 4, 0] {
        uart.write_byte(HEX[((v >> shift) & 0xF) as usize]);
    }
}

#[cortex_m_rt::entry]
fn main() -> ! {
    let p = gd32f470::Peripherals::take().expect("peripherals already taken");

    let rcc = Rcc::new(&p.rcu);
    rcc.enable_gpio_port(Port::A);
    rcc.enable_gpio_port(Port::B);
    rcc.enable_gpio_port(Port::C);
    rcc.enable_gpio_port(Port::E);
    rcc.enable_uart6();

    let _utx = Pin::alternate(&p.gpioe, 7, 8);
    let _urx = Pin::alternate(&p.gpioe, 8, 8);
    let uart = Uart::new(&p.uart6);
    uart.enable(PCLK_HZ, 115_200);
    uart.write(b"enet-phy-probe\r\n");

    // 1. SYSCFG 时钟（APB2）+ RMII 选择。必须在 ENET 复位前设置
    rcc.enable_syscfg();
    p.syscfg.cfg1().modify(|_, w| w.enet_phy_sel().set_bit());
    uart.write(b"syscfg: RMII selected\r\n");

    // 2. ENET 时钟（AHB1：MAC/DMA + TX/RX/PTP）
    rcc.enable_enet();

    // 3. PHY 硬复位（PC0 = ETH_nRST，低有效）
    let mut phy_rst = Pin::output(&p.gpioc, 0);
    phy_rst.set_low();
    delay_ms(10);
    phy_rst.set_high();
    delay_ms(50); // LAN8720A 复位后 ~50ms 就绪

    // 4. RMII 引脚组（全 AF11）
    let _ref_clk = Pin::alternate(&p.gpioa, 1, 11);
    let _mdio = Pin::alternate(&p.gpioa, 2, 11);
    let _crs_dv = Pin::alternate(&p.gpioa, 7, 11);
    let _mdc = Pin::alternate(&p.gpioc, 1, 11);
    let _rxd0 = Pin::alternate(&p.gpioc, 4, 11);
    let _rxd1 = Pin::alternate(&p.gpioc, 5, 11);
    let _tx_en = Pin::alternate(&p.gpiob, 11, 11);
    let _txd0 = Pin::alternate(&p.gpiob, 12, 11);
    let _txd1 = Pin::alternate(&p.gpiob, 13, 11);

    // 5. ENET 软复位：SWR 置位后硬件在 REF_CLK 就绪时自动清零
    p.enet_dma
        .dma_bctl()
        .modify(|_, w| w.swr().set_bit());
    let mut guard = 2_000_000u32;
    while p.enet_dma.dma_bctl().read().swr().bit_is_set() {
        guard -= 1;
        if guard == 0 {
            uart.write(b"SWR: STUCK (REF_CLK dead?)\r\n");
            loop {
                core::hint::spin_loop();
            }
        }
    }
    uart.write(b"SWR: cleared (REF_CLK alive)\r\n");

    // 6. MDIO 时钟：MDC = HCLK/(42+2*2^CLR)，CLR=0 -> ~364kHz @16MHz
    p.enet_mac.mac_phy_ctl().modify(|_, w| w.clr().set(0));

    // MDIO 读原语（闭内联，避免借用问题）
    macro_rules! mdio_read {
        ($phy:expr, $reg:expr) => {{
            p.enet_mac
                .mac_phy_ctl()
                .modify(|_, w| {
                    w.pa().set($phy).pr().set($reg).pw().clear_bit()
                });
            p.enet_mac.mac_phy_ctl().modify(|_, w| w.pb().set_bit());
            let mut g = 2_000_000u32;
            while p.enet_mac.mac_phy_ctl().read().pb().bit_is_set() {
                g -= 1;
                if g == 0 {
                    break;
                }
            }
            p.enet_mac.mac_phy_data().read().pd().bits()
        }};
    }

    uart.write(b"PHY scan (REG2/REG3, x=dead addr):\r\n");
    let mut found: Option<(u8, u16, u16)> = None;
    for phy in 0u8..32 {
        let id1 = mdio_read!(phy, 2);
        let id2 = mdio_read!(phy, 3);
        // 无 PHY 的地址典型表现为全 0 或全 F
        if id1 == 0x0000 || id1 == 0xFFFF {
            continue;
        }
        put_str_addr(&uart, phy);
        uart.write(b": ID=0x");
        put_hex4(&uart, id1);
        uart.write_byte(b' ');
        put_hex4(&uart, id2);
        if id1 == 0x0007 {
            uart.write(b"  <-- LAN8720A");
            found = Some((phy, id1, id2));
        }
        uart.write(b"\r\n");
    }

    match found {
        Some((phy, id1, id2)) => {
            uart.write(b"PHY_FOUND: addr=");
            put_str_addr(&uart, phy);
            uart.write(b" id=0x");
            put_hex4(&uart, id1);
            put_hex4(&uart, id2);
            uart.write(b"\r\n");
            uart.write(b"PHY_PROBE: PASS\r\n");
        }
        None => {
            uart.write(b"PHY_PROBE: NO LAN8720A FOUND\r\n");
        }
    }

    loop {
        core::hint::spin_loop();
    }
}

fn put_str_addr(uart: &Uart, phy: u8) {
    uart.write(b"phy[");
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    if phy >= 0x10 {
        uart.write_byte(HEX[(phy >> 4) as usize]);
    } else {
        uart.write_byte(b'0');
    }
    uart.write_byte(HEX[(phy & 0xF) as usize]);
    uart.write_byte(b']');
}
