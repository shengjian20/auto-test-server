//! enet-link-test：阶段 3 第二个闸门——PHY 复位/自协商/链路状态轮询
//!
//! 链路前提：RJ45 需接入交换机/路由器（自协商对端在线才会上线）。
//! 无论 Up/Down 均为有效证据：
//! - Up  + 速度双工解析 -> MDIO 数据路径 + 自协商引擎完全工作
//! - Down + BMSR 可读    -> MDIO 工作正常，仅对端缺失
//! 结果经 UART6 console 周期输出（1s 间隔，5 轮后挂起）。
#![no_std]
#![no_main]
#![deny(unsafe_code)]

use embassy_gd32::enet::{LinkState, Phy};
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

fn link_str(ls: LinkState) -> &'static str {
    match ls {
        LinkState::Down => "DOWN",
        LinkState::Up10Half => "UP 10HALF",
        LinkState::Up10Full => "UP 10FULL",
        LinkState::Up100Half => "UP 100HALF",
        LinkState::Up100Full => "UP 100FULL",
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
    rcc.enable_syscfg();
    rcc.enable_enet();

    let _utx = Pin::alternate(&p.gpioe, 7, 8);
    let _urx = Pin::alternate(&p.gpioe, 8, 8);
    let uart = Uart::new(&p.uart6);
    uart.enable(PCLK_HZ, 115_200);
    uart.write(b"enet-link-test\r\n");

    // RMII 选择须在 ENET 复位前
    p.syscfg.cfg1().modify(|_, w| w.enet_phy_sel().set_bit());

    // PHY 硬复位（PC0 = ETH_nRST）
    let mut phy_rst = Pin::output(&p.gpioc, 0);
    phy_rst.set_low();
    delay_ms(10);
    phy_rst.set_high();
    delay_ms(50);

    // RMII 引脚组（AF11）
    let _ref_clk = Pin::alternate(&p.gpioa, 1, 11);
    let _mdio = Pin::alternate(&p.gpioa, 2, 11);
    let _crs_dv = Pin::alternate(&p.gpioa, 7, 11);
    let _mdc = Pin::alternate(&p.gpioc, 1, 11);
    let _rxd0 = Pin::alternate(&p.gpioc, 4, 11);
    let _rxd1 = Pin::alternate(&p.gpioc, 5, 11);
    let _tx_en = Pin::alternate(&p.gpiob, 11, 11);
    let _txd0 = Pin::alternate(&p.gpiob, 12, 11);
    let _txd1 = Pin::alternate(&p.gpiob, 13, 11);

    // ENET 软复位（REF_CLK 存活时自动清零）
    if !embassy_gd32::enet::sw_reset(&p.enet_dma) {
        uart.write(b"SWR STUCK\r\n");
        loop {
            core::hint::spin_loop();
        }
    }
    uart.write(b"SWR cleared\r\n");

    let phy = Phy::new(&p.enet_mac);

    // ID 校验（0x0007C0F1 = LAN8720A）
    match phy.read_id() {
        Some(id @ 0x0007C0F1) => {
            uart.write(b"PHY ID: 0x0007C0F1 (LAN8720A) OK\r\n");
            let _ = id;
        }
        Some(id) => {
            uart.write(b"PHY ID unexpected\r\n");
            let _ = id;
        }
        None => {
            uart.write(b"MDIO no response\r\n");
            loop {
                core::hint::spin_loop();
            }
        }
    }

    // 复位 + 启动自协商
    if !phy.reset() {
        uart.write(b"PHY reset fail\r\n");
        loop {
            core::hint::spin_loop();
        }
    }
    if !phy.start_autoneg() {
        uart.write(b"autoneg start fail\r\n");
        loop {
            core::hint::spin_loop();
        }
    }
    uart.write(b"autoneg started\r\n");

    // 轮询 5 轮（每轮 1s）
    for round in 0..5u8 {
        match phy.link_state() {
            Some(ls) => {
                uart.write(b"link: ");
                uart.write(link_str(ls).as_bytes());
                uart.write(b"\r\n");
                if ls != LinkState::Down {
                    uart.write(b"LINK_UP\r\n");
                }
            }
            None => {
                uart.write(b"link: MDIO ERR\r\n");
            }
        }
        delay_ms(1000);
    }

    uart.write(b"done\r\n");
    loop {
        core::hint::spin_loop();
    }
}
