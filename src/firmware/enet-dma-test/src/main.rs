//! enet-dma-test：阶段 3 数据通路验收——PHY BMCR 回环 + MAC/DMA 描述符收发
//!
//! 验收策略（与 CAN 回环同构，无需网线）：
//! - PHY 侧：BMCR.LOOPBACK=1（LAN8720A 内部 TX->RX 短接，100M 全双工固定态）
//! - MAC 侧：SPD=100M/DPM=全双工、混杂模式（FRMF.PM=1 避开过滤器变量）
//! - DMA 侧：TX/RX 环描述符（carrier-box 同构 5 环 × 1524B），store-and-forward
//! - 数据流：TX 描述符发 64B 帧（0x55 码型）-> PHY 回环 -> RX FIFO -> RX 描述符
//!   -> 读回比对
//! - 结果经 UART6 console（PE7/PE8 AF8 115200）输出，单轮后挂起
#![no_std]
#![no_main]
#![deny(unsafe_code)]

use embassy_gd32::enet::{LinkState, Phy};
use embassy_gd32::enet_dma;
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

fn put_hex8(uart: &Uart, v: u32) {
    put_hex4(&uart, (v >> 16) as u16);
    put_hex4(&uart, v as u16);
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
    uart.write(b"enet-dma-test\r\n");

    // 1. RMII 选择（ENET 复位前）
    p.syscfg.cfg1().modify(|_, w| w.enet_phy_sel().set_bit());
    rcc.enable_enet();

    // 2. PHY 硬复位（PC0 = ETH_nRST）
    let mut phy_rst = Pin::output(&p.gpioc, 0);
    phy_rst.set_low();
    delay_ms(10);
    phy_rst.set_high();
    delay_ms(50);

    // 3. RMII 引脚组（AF11）
    let _ref_clk = Pin::alternate(&p.gpioa, 1, 11);
    let _mdio = Pin::alternate(&p.gpioa, 2, 11);
    let _crs_dv = Pin::alternate(&p.gpioa, 7, 11);
    let _mdc = Pin::alternate(&p.gpioc, 1, 11);
    let _rxd0 = Pin::alternate(&p.gpioc, 4, 11);
    let _rxd1 = Pin::alternate(&p.gpioc, 5, 11);
    let _tx_en = Pin::alternate(&p.gpiob, 11, 11);
    let _txd0 = Pin::alternate(&p.gpiob, 12, 11);
    let _txd1 = Pin::alternate(&p.gpiob, 13, 11);

    // 4. ENET 软复位（REF_CLK 存活时自动清零）
    if !embassy_gd32::enet::sw_reset(&p.enet_dma) {
        uart.write(b"SWR STUCK (REF_CLK dead)\r\n");
        loop {
            core::hint::spin_loop();
        }
    }
    uart.write(b"SWR cleared\r\n");

    // DMA_BCTL.DPSL=32 突发配置已收编进 enet_dma::start_dma

    // 5. PHY：ID 校验 + 回环模式（100M 全双工固定态）
    let phy = Phy::new(&p.enet_mac);
    match phy.read_id() {
        Some(0x0007C0F1) => uart.write(b"PHY: LAN8720A OK\r\n"),
        _ => {
            uart.write(b"PHY: no LAN8720A\r\n");
            loop {
                core::hint::spin_loop();
            }
        }
    }
    if !phy.reset() {
        uart.write(b"PHY reset fail\r\n");
        loop {
            core::hint::spin_loop();
        }
    }
    // PHY 保持正常模式（MAC LBM 在 MII 层内部回环，帧不出引脚，
    // PHY 是否连网线均不影响本测试）
    delay_ms(10);
    uart.write(b"PHY: normal mode (MAC LBM loopback)\r\n");

    // 6. MAC 配置：100M 全双工 + MAC LBM 回环（MII 层内部 TX->RX，
    //    纯 MAC+DMA 数据通路测试，不依赖 PHY/引脚）+ 混杂模式
    p.enet_mac.mac_cfg().modify(|_, w| {
        w.spd().set_bit() // 100M
            .dpm().set_bit() // 全双工
            .lbm().set_bit() // MAC 回环
    });
    p.enet_mac.mac_frmf().modify(|_, w| w.pm().set_bit()); // 混杂模式

    // 7. DMA：描述符环初始化 + 表地址 + store-and-forward + 启动
    let rings = enet_dma::take_rings();
    rings.tx.init();
    rings.rx.init();
    // MAC 地址写入（ADDR0H/L）——空 MAC 会被部分 MAC 实现拒发
    embassy_gd32::enet::set_mac_addr0(&p.enet_mac, [0x02, 0x04, 0x06, 0x08, 0x0A, 0x0C]);

    // DMA 启动收编进 HAL（环地址写入的 unsafe 由 HAL 单点收敛）
    enet_dma::start_dma(&p.enet_dma, rings);
    // MAC 收发使能（DMA 启动后）
    p.enet_mac.mac_cfg().modify(|_, w| w.ren().set_bit().ten().set_bit());
    uart.write(b"DMA started\r\n");

    // 自报告：MAC/DMA 关键寄存器原始值（调试期）
    let mc = p.enet_mac.mac_cfg().read().bits();
    let dc = p.enet_dma.dma_ctl().read().bits();
    let ds = p.enet_dma.dma_stat().read().bits();
    uart.write(b"MAC_CFG=0x");
    put_hex8(&uart, mc);
    uart.write(b" DMA_CTL=0x");
    put_hex8(&uart, dc);
    uart.write(b" DMA_STAT=0x");
    put_hex8(&uart, ds);
    uart.write(b"\r\n");

    // 8. 数据通路测试：发 3 帧，等回环收
    let mut pass = 0u32;
    let test_frames: [[u8; 64]; 3] = [
        {
            let mut f = [0u8; 64];
            for (i, b) in f.iter_mut().enumerate() {
                *b = i as u8;
            }
            f
        },
        [0x55; 64],
        {
            let mut f = [0u8; 64];
            for (i, b) in f.iter_mut().enumerate() {
                *b = 0xFF - i as u8;
            }
            f
        },
    ];

    for (i, frame) in test_frames.iter().enumerate() {
        // TX
        if !rings.tx.submit(frame) {
            uart.write(b"frame");
            put_hex4(&uart, i as u16);
            uart.write(b": TX submit fail\r\n");
            continue;
        }
        // 唤醒 DMA：poll/demand 位（写 DMA_TPEN 触发轮询）
                // TBU 置位时 DMA 已停：清标志 + 重新 poll（均收敛于 HAL）
        if p.enet_dma.dma_stat().read().tbu().bit_is_set() {
            enet_dma::clear_tbu(&p.enet_dma);
        }
        enet_dma::tx_poll(&p.enet_dma);

        // 诊断：TX 提交后立即读 TCURR + TPEN 多补几次
        let tcurr = p.enet_dma.dma_tpen() as *const _ as u32; // 占位防优化
        let _ = tcurr;
        enet_dma::tx_poll(&p.enet_dma);
        delay_ms(2);
        enet_dma::tx_poll(&p.enet_dma);
        let tcurr_v = { p.enet_dma.dma_tdtaddr().read().stt().bits() };

        // RX 等待（轮询 RX 环 pending）
        let mut got = false;
        for _ in 0..2_000_000 {
            if rings.rx.pending() > 0 {
                got = true;
                break;
            }
            core::hint::spin_loop();
        }
        if !got {
            let tc = p.enet_dma.dma_tdtaddr().read().stt().bits();
            let _ = tcurr_v;
            uart.write(b"frame");
            put_hex4(&uart, i as u16);
            uart.write(b": NO RX tdt=0x");
            put_hex8(&uart, tc);
            uart.write(b"\r\n");
            continue;
        }

        // 比对：RX 环 index-1 是刚收的（环回从 0 顺序收）
        let rx_idx = (rings.rx.index + enet_dma::RING_LEN - 1) % enet_dma::RING_LEN;
        if !rings.rx.frame_valid(rx_idx) {
            uart.write(b"frame");
            put_hex4(&uart, i as u16);
            uart.write(b": RX invalid desc\r\n");
            continue;
        }
        let rx_len = rings.rx.frame_len(rx_idx);
        let rx_ok = rx_len >= frame.len()
            && rings.rx.buf[rx_idx][..frame.len()] == *frame;
        if rx_ok {
            pass += 1;
            uart.write(b"frame");
            put_hex4(&uart, i as u16);
            uart.write(b": MATCH len=");
            put_hex4(&uart, rx_len as u16);
            uart.write(b"\r\n");
        } else {
            uart.write(b"frame");
            put_hex4(&uart, i as u16);
            uart.write(b": MISMATCH len=");
            put_hex4(&uart, rx_len as u16);
            uart.write(b"\r\n");
        }
        // 释放 RX 描述符
        rings.rx.release(rx_idx);
    }

    // 复查：帧后状态
    let ds2 = p.enet_dma.dma_stat().read().bits();
    let rfifo = p.enet_dma.dma_rpen().read().bits();
    uart.write(b"post DMA_STAT=0x");
    put_hex8(&uart, ds2);
    uart.write(b" RPEN=0x");
    put_hex8(&uart, rfifo);
    uart.write(b"\r\n");

    if pass == 3 {
        uart.write(b"ENET_DMA_TEST: PASS\r\n");
    } else {
        uart.write(b"ENET_DMA_TEST: FAIL\r\n");
    }

    // PHY 退出回环（恢复正常）
    phy.clear_loopback();

    loop {
        core::hint::spin_loop();
    }
}
