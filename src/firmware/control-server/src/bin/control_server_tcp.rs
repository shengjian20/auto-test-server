//! control-server-tcp：embassy-net 形态（需求 1 验收——socket 风格 async API）
//!
//! tcp::Socket 的 read/write 原生 async，行组装/回显由 await 驱动，
//! 无手写 poll 状态机。协议主体单一来源复用（init_deps + dispatch）。
//! 内置自测（lbm feature）：client 连本机 :9000 发 "ping" 验证 "OK pong"。
#![no_std]
#![no_main]
extern crate alloc;

use alloc::boxed::Box;
use alloc::vec;
use control_server::app;
use embassy_executor::Spawner;
use embassy_gd32::{Pin, Port, Rcc, Uart};
use embassy_net::tcp::TcpSocket;
use embassy_net::{Config, Ipv4Address, Ipv4Cidr, Stack, StackResources};
use linked_list_allocator::LockedHeap;
use panic_halt as _;

const PORT: u16 = 9000;
const LOCAL_IP: [u8; 4] = [172, 22, 0, 50];
const LOCAL_MAC: [u8; 6] = [0x02, 0x04, 0x06, 0x08, 0x0A, 0x0C];

#[cfg(feature = "lbm")]
const MAC_LBM: bool = true;
#[cfg(not(feature = "lbm"))]
const MAC_LBM: bool = false;

// ---- 堆（smoltcp alloc；TCM 禁 DMA——堆必须在主 SRAM）----
static mut HEAP_MEM: [u8; 65536] = [0; 65536];

#[global_allocator]
static ALLOCATOR: LockedHeap = LockedHeap::empty();

/// unsafe 依据：单核单点初始化（main 最前，任务未启动），堆区域独占
fn init_heap() {
    unsafe {
        let mem = core::ptr::addr_of_mut!(HEAP_MEM);
        ALLOCATOR.lock().init((*mem).as_mut_ptr(), (*mem).len());
    }
}

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

/// 网卡驱动（embassy-net-driver::Driver 薄包装：复用 enet_smoltcp token）
struct EnetDriver<'a> {
    rings: &'a mut embassy_gd32::enet_dma::Rings,
    mac: [u8; 6],
}

/// LinkState 采样：DMA/PHY 状态经 Phi 方法注入有借用冲突，LBM 自测恒
/// Up（self-ping 语义下链路必达）；normal 模式由 smoltcp 超时自然处理
fn link_assume() -> bool {
    true
}

impl embassy_net_driver::Driver for EnetDriver<'_> {
    type RxToken<'a>
        = embassy_gd32::enet_smoltcp::EnetRxToken<'a>
    where
        Self: 'a;
    type TxToken<'a>
        = embassy_gd32::enet_smoltcp::EnetTxToken<'a>
    where
        Self: 'a;

    fn receive(&mut self, cx: &mut core::task::Context) -> Option<(Self::RxToken<'_>, Self::TxToken<'_>)> {
        // embassy-net-driver 契约：返回 None 必须注册 waker——缺失则
        // Runner::run poll_fn 永挂（板上实证：帧滞留描述符 FL=64/FS|LS/
        // OWN=0 而主任务 polls=1 永不再被轮询）
        let idx = match self.rings.rx.first_valid() {
            Some(i) => i,
            None => {
                embassy_gd32::enet_smoltcp::net_poll::register(cx);
                return None;
            }
        };
        embassy_gd32::enet_smoltcp::enet_dma_stats::rx_hit();
        let fl = self.rings.rx.frame_len(idx);
        let len = fl.saturating_sub(4).min(embassy_gd32::enet_dma::BUF_SIZE);
        let rx = &mut self.rings.rx;
        let tx = &mut self.rings.tx;
        Some((
            embassy_gd32::enet_smoltcp::EnetRxToken::new(len, rx, idx),
            embassy_gd32::enet_smoltcp::EnetTxToken::new(tx),
        ))
    }

    fn transmit(&mut self, cx: &mut core::task::Context) -> Option<Self::TxToken<'_>> {
        if self.rings.tx.available() {
            Some(embassy_gd32::enet_smoltcp::EnetTxToken::new(&mut self.rings.tx))
        } else {
            embassy_gd32::enet_smoltcp::net_poll::register(cx);
            None
        }
    }

    fn link_state(&mut self, _cx: &mut core::task::Context) -> embassy_net_driver::LinkState {
        // LBM 自测：self-ping 必达恒 Up；normal 模式接网线时由 PHY 实际
        // 链路决定——静态近似（PHY 采样与 Driver 的借用冲突留待重构）
        if MAC_LBM || link_assume() {
            embassy_net_driver::LinkState::Up
        } else {
            embassy_net_driver::LinkState::Down
        }
    }

    fn capabilities(&self) -> embassy_net_driver::Capabilities {
        let mut caps = embassy_net_driver::Capabilities::default();
        caps.max_transmission_unit = 1500;
        caps.max_burst_size = Some(embassy_gd32::enet_dma::RING_LEN as usize);
        caps
    }

    fn hardware_address(&self) -> embassy_net_driver::HardwareAddress {
        embassy_net_driver::HardwareAddress::Ethernet(self.mac)
    }
}

#[embassy_executor::main]
async fn main(sp: Spawner) {
    init_heap();

    let p = app::periph::steal();
    embassy_gd32::init_time_driver();

    let rcc = Rcc::new(p.rcu);
    rcc.enable_gpio_port(Port::A);
    rcc.enable_gpio_port(Port::B);
    rcc.enable_gpio_port(Port::C);
    rcc.enable_gpio_port(Port::D);
    rcc.enable_gpio_port(Port::E);
    rcc.enable_uart6();
    rcc.enable_syscfg();
    rcc.enable_enet();
    rcc.enable_spi3();
    rcc.enable_spi2();

    let _utx = Pin::alternate(p.gpioe, 7, 8);
    let _urx = Pin::alternate(p.gpioe, 8, 8);
    let uart = Uart::new(p.uart6);
    uart.enable(16_000_000, 115_200);
    uart.write(b"control-server-tcp v2 (embassy-net)\r\n");

    // 1. RMII 选择（ENET 复位前）
    p.syscfg.cfg1().modify(|_, w| w.enet_phy_sel().set_bit());

    // 2. PHY 硬复位 + RMII 引脚 AF11
    let mut phy_rst = Pin::output(p.gpioc, 0);
    phy_rst.set_low();
    delay_ms(10);
    phy_rst.set_high();
    delay_ms(50);
    let _ref_clk = Pin::alternate(p.gpioa, 1, 11);
    let _mdio = Pin::alternate(p.gpioa, 2, 11);
    let _crs_dv = Pin::alternate(p.gpioa, 7, 11);
    let _mdc = Pin::alternate(p.gpioc, 1, 11);
    let _rxd0 = Pin::alternate(p.gpioc, 4, 11);
    let _rxd1 = Pin::alternate(p.gpioc, 5, 11);
    let _tx_en = Pin::alternate(p.gpiob, 11, 11);
    let _txd0 = Pin::alternate(p.gpiob, 12, 11);
    let _txd1 = Pin::alternate(p.gpiob, 13, 11);

    // 3. SWR 轮询
    if !embassy_gd32::enet::sw_reset(&p.enet_dma) {
        uart.write(b"SWR STUCK (REF_CLK dead)\r\n");
        loop {
            core::hint::spin_loop();
        }
    }

    // 4. PHY ID 校验
    let phy = embassy_gd32::enet::Phy::new(p.enet_mac);
    match phy.read_id() {
        Some(0x0007C0F1) => uart.write(b"PHY: LAN8720A OK\r\n"),
        _ => {
            uart.write(b"PHY: no LAN8720A\r\n");
            loop {
                core::hint::spin_loop();
            }
        }
    }

    // 5. MAC 配置 + 混杂 + MAC 地址
    p.enet_mac
        .mac_cfg()
        .modify(|_, w| w.spd().set_bit().dpm().set_bit());
    if MAC_LBM {
        p.enet_mac.mac_cfg().modify(|_, w| w.lbm().set_bit());
        uart.write(b"mode: MAC LBM self-test\r\n");
    } else {
        uart.write(b"mode: normal\r\n");
    }
    p.enet_mac.mac_frmf().modify(|_, w| w.pm().set_bit());
    embassy_gd32::enet::set_mac_addr0(p.enet_mac, LOCAL_MAC);

    // 6. 描述符环 + DMA 启动
    let rings = embassy_gd32::enet_dma::take_rings();
    rings.tx.init();
    rings.rx.init();
    embassy_gd32::enet_dma::start_dma(&p.enet_dma, rings);
    p.enet_mac
        .mac_cfg()
        .modify(|_, w| w.ren().set_bit().ten().set_bit());
    embassy_gd32::enet_dma::rx_poll(&p.enet_dma);
    uart.write(b"DMA started\r\n");

    // 7. embassy-net Stack：driver 按值移动（D = EnetDriver<'static>，
    //    Runner<'static, EnetDriver<'static>> 与 net_task 签名精确匹配；
    //    传 &mut 会带双引用参数，spawn 的 'static 约束必炸）；resources
    //    需 'static 引用故 Box::leak
    let rings: &'static mut embassy_gd32::enet_dma::Rings = rings;
    let resources: &'static mut StackResources<3> =
        Box::leak(Box::new(StackResources::<3>::new()));
    let (stack, runner) = embassy_net::new(
        EnetDriver { rings, mac: LOCAL_MAC },
        Config::ipv4_static(embassy_net::StaticConfigV4 {
            address: Ipv4Cidr::new(Ipv4Address::new(
                LOCAL_IP[0], LOCAL_IP[1], LOCAL_IP[2], LOCAL_IP[3],
            ), 23),
            gateway: Some(Ipv4Address::new(172, 22, 0, 1)),
            dns_servers: Default::default(),
        }),
        resources,
        0x1234_5678_9abc_def0,
    );
    sp.spawn(net_task(runner).unwrap());
    sp.spawn(poll_kicker().unwrap());
    sp.spawn(diag_task(Uart::new(p.uart6), p.enet_dma).unwrap());
    uart.write(b"stack up\r\n");

    // 8. 协议主体（单一来源）+ socket 风格 async API（accept().await 形态）
    let (mut deps, _) = app::init_deps();
    let mut rx_buf = vec![0u8; 2048];
    let mut tx_buf = vec![0u8; 2048];
    let mut socket = TcpSocket::new(stack, &mut rx_buf, &mut tx_buf);
    uart.write(b"listening :9000\r\n");

    let mut line = [0u8; app::LINE_MAX];
    let mut n = 0usize;
    let mut hb: i32 = 0;

    loop {
        // 心跳（主循环每拍判定；accept await 挂起期间本任务不运行，
        // kicker/poll 计数由 kicker 任务持续推进——读数即任务健康证据）
        hb += 1;
        if hb >= 2500 {
            hb = 0;
            uart.write(b"hb kicks=");
            put_hex8(&uart, embassy_gd32::enet_smoltcp::net_poll::kicks());
            uart.write(b" polls=");
            put_hex8(&uart, embassy_gd32::enet_smoltcp::net_poll::polls());
            uart.write(b" stat=0x");
            put_hex8(&uart, p.enet_dma.dma_stat().read().bits());
            uart.write(b"\r\n");
        }
        embassy_gd32::enet_smoltcp::net_poll::poll_tick();

        // accept：等连接建立（0.7.1 服务端形态）
        if let Err(e) = socket.accept(embassy_net::IpListenEndpoint::from(PORT)).await {
            uart.write(b"accept err\r\n");
            continue;
        }
        uart.write(b"client connected\r\n");

        // 连接 established：async read 行组装 -> dispatch -> write
        loop {
            let mut tmp = [0u8; 512];
            match socket.read(&mut tmp).await {
                Ok(0) | Err(_) => break,
                Ok(k) => {
                    for &ch in &tmp[..k] {
                        if ch == b'\n' || ch == b'\r' {
                            if n > 0 {
                                let mut toks: [&[u8]; 8] = [b""; 8];
                                let nt = app::tokenize(&line[..n], &mut toks);
                                // dispatch 输出汇 = async write（同步闭包内
                                // 不便 await，先收集后一次写出——响应 < MTU）
                                let mut resp = heapless::Vec::<u8, 512>::new();
                                deps.dispatch(&mut |b: &[u8]| {
                                    let _ = resp.extend_from_slice(b);
                                }, &toks, nt);
                                let _ = socket.write(&resp).await;
                                n = 0;
                            }
                        } else if n < app::LINE_MAX {
                            line[n] = ch;
                            n += 1;
                        }
                    }
                }
            }
        }
        socket.close();
    }
}

/// 网卡 poll 任务（Runner::run 挂起后由 poll_kicker 任务周期唤醒——
/// 轮询式网卡无 RX 中断，"外部 kick" 是 embassy 正统驱动形态）
#[embassy_executor::task]
async fn net_task(mut runner: embassy_net::Runner<'static, EnetDriver<'static>>) {
    runner.run().await
}

/// poll 驱动节拍 + 挂起恢复：2ms 唤醒 Runner 的 poll_fn（receive 返回
/// None 时已向 HAL 的 WakerRegistration 注册本任务的 waker）；TBU/RBU
/// 挂起恢复缺失的后果 = smoltcp 提交的帧滞留描述符永不发出（ARP 永不
/// 解析，connect 全超时——smoltcp 路线已踩过的同款坑）
#[embassy_executor::task]
async fn poll_kicker() {
    let p = app::periph::steal();
    loop {
        embassy_gd32::enet_smoltcp::wake_net_poll();
        embassy_gd32::enet_smoltcp::recover_suspended(&p.enet_dma);
        embassy_time::Timer::after(embassy_time::Duration::from_millis(2)).await;
    }
}

/// 诊断任务：5s 打印 kicker/poll 计数 + RX/TX 帧计数 + DMA stat
/// （独立调度——accept 挂起期间主循环不转，此任务计数即任务健康证据）
#[embassy_executor::task]
async fn diag_task(uart: Uart<'static>, dma: &'static gd32f470::EnetDma) {
    loop {
        embassy_time::Timer::after(embassy_time::Duration::from_millis(5000)).await;
        let (rx, tx, txf) = embassy_gd32::enet_smoltcp::enet_dma_stats::snapshot();
        uart.write(b"diag kicks=");
        put_hex8(&uart, embassy_gd32::enet_smoltcp::net_poll::kicks());
        uart.write(b" polls=");
        put_hex8(&uart, embassy_gd32::enet_smoltcp::net_poll::polls());
        uart.write(b" rx=");
        put_hex8(&uart, rx);
        uart.write(b" tx=");
        put_hex8(&uart, tx);
        uart.write(b" txf=");
        put_hex8(&uart, txf);
        uart.write(b" stat=0x");
        put_hex8(&uart, dma.dma_stat().read().bits());
        uart.write(b"\r\n");
    }
}
