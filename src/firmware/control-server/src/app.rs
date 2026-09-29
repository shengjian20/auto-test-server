//! control-server 应用主体（lib 共享源码——由 control-server @0x08000000 与
//! control-server-ota @0x08008000 两个 bin 变体复用；banner 由各 bin 注入）
//!
//! 协议 v4（在 v3 基础上新增写路径）：
//!   ping | cpld mux get|set | out <n> <0|1> | in | ctrl <1|2> <0|1>
//!   flash jedec
//!   flash read <addr6hex> <len_dec(1-128)>
//!   flash se   <addr6hex>                      —— W25Q 4K 扇区擦除
//!   flash wr   <addr6hex> <hexbytes..>         —— W25Q 写（须已擦除）
//!   flash crc  <addr6hex> <len_dec(1-32768)>   —— W25Q 区间 CRC-32
//!   ota boot                                   —— 软复位进 bootloader
//!   can0 send <id> <db..> | can0 recv
//! 响应: OK [values...] | ERR <msg>
#![deny(unsafe_code)]

use cortex_m::peripheral::SCB;
use embassy_gd32::can::{Can, Frame};
use embassy_gd32::crc32::Crc32;
use embassy_gd32::w25q::{W25q, PAGE_SIZE};
use embassy_gd32::{cpld, Pin, Port, Rcc, Spi, Uart};

const PCLK1_HZ: u32 = 16_000_000;
const BAUD: u32 = 115_200;
const LINE_MAX: usize = 192;

fn delay_ms(ms: u32) {
    for _ in 0..ms {
        for _ in 0..4000 {
            core::hint::spin_loop();
        }
    }
}

fn put_str(uart: &Uart, s: &str) {
    uart.write(s.as_bytes());
}

fn put_u8(uart: &Uart, v: u8) {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    uart.write_byte(HEX[(v >> 4) as usize]);
    uart.write_byte(HEX[(v & 0xF) as usize]);
}

fn put_hex4(uart: &Uart, v: u16) {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    for shift in [12, 8, 4, 0] {
        uart.write_byte(HEX[((v >> shift) & 0xF) as usize]);
    }
}

fn put_hex8(uart: &Uart, v: u32) {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    for shift in [28, 24, 20, 16, 12, 8, 4, 0] {
        uart.write_byte(HEX[((v >> shift) & 0xF) as usize]);
    }
}

/// 令牌化（空格分隔，原地）
fn tokenize<'a>(line: &'a [u8], out: &mut [&'a [u8]; 8]) -> usize {
    let mut n = 0;
    let mut i = 0;
    while i < line.len() && n < 8 {
        while i < line.len() && line[i] == b' ' {
            i += 1;
        }
        if i >= line.len() {
            break;
        }
        let start = i;
        while i < line.len() && line[i] != b' ' {
            i += 1;
        }
        out[n] = &line[start..i];
        n += 1;
    }
    n
}

fn eq(t: &[u8], s: &str) -> bool {
    t == s.as_bytes()
}

fn parse_u8(t: &[u8]) -> Option<u8> {
    if t.len() == 2 {
        let hi = (t[0] as char).to_digit(16)?;
        let lo = (t[1] as char).to_digit(16)?;
        return Some((hi * 16 + lo) as u8);
    }
    if t.len() == 1 {
        return Some((t[0] as char).to_digit(10)? as u8);
    }
    None
}

fn parse_u16_hex(t: &[u8]) -> Option<u16> {
    if t.len() < 3 || t.len() > 4 {
        return None;
    }
    let mut v: u16 = 0;
    for &c in t {
        v = (v << 4) + (c as char).to_digit(16)? as u16;
    }
    Some(v)
}

/// W25Q 24 位地址（1-6 个 hex 字符）
fn parse_u32_hex(t: &[u8]) -> Option<u32> {
    if t.is_empty() || t.len() > 6 {
        return None;
    }
    let mut v: u32 = 0;
    for &c in t {
        v = (v << 4) + (c as char).to_digit(16)? as u32;
    }
    Some(v)
}

/// 十进制 u32（1-5 位，用于 len）
fn parse_dec_u32(t: &[u8]) -> Option<u32> {
    if t.is_empty() || t.len() > 5 {
        return None;
    }
    let mut v: u32 = 0;
    for &c in t {
        v = v * 10 + (c as char).to_digit(10)? as u32;
    }
    Some(v)
}

/// hex 串按字节对解析（兼容连写形态），返回写入字节数
fn parse_hex_bytes(t: &[u8], buf: &mut [u8]) -> Option<usize> {
    if t.is_empty() || t.len() % 2 != 0 || t.len() > buf.len() * 2 {
        return None;
    }
    for i in (0..t.len()).step_by(2) {
        let hi = (t[i] as char).to_digit(16)?;
        let lo = (t[i + 1] as char).to_digit(16)?;
        buf[i / 2] = (hi * 16 + lo) as u8;
    }
    Some(t.len() / 2)
}

/// 外设静态访问（bootloader 跳转链专用）：
/// take() 的 DEVICE_PERIPHERALS 标志在 RAM 不清零的跳转链上残留
/// （bootloader 已 take），二次 take 返回 None -> expect panic ->
/// panic_halt 静默（板上实证：PC 停在 HardFault_ 0x0800D3DC 自环）。
/// 改用 PAC 类型别名的 PTR 静态地址直取——安全化封装，固件零 unsafe。
mod periph {
    use gd32f470::{Can0, EnetDma, EnetMac, Gpioa, Gpiob, Gpioc, Gpiod, Gpioe, Rcu, Spi2, Spi3, Timer1, Uart6};

    /// 字段为 PAC 值类型（Periph<RB, A> 零大小句柄，Copy 语义）——
    /// 调用点 `&p.gpioe` 自动形成 `&Periph`，匹配 HAL 的 GpioRef bound
    pub struct Peripherals {
        pub rcu: Rcu,
        pub gpioa: Gpioa,
        pub gpiob: Gpiob,
        pub gpioc: Gpioc,
        pub gpiod: Gpiod,
        pub gpioe: Gpioe,
        pub uart6: Uart6,
        pub spi2: Spi2,
        pub spi3: Spi3,
        pub can0: Can0,
        pub timer1: Timer1,
        pub enet_mac: EnetMac,
        pub enet_dma: EnetDma,
    }

    /// unsafe 收敛点：静态外设地址派生引用（PAC Periph::PTR 同源语义），
    /// 应用生命周期内外设独占（单一任务，无别名写）
    #[allow(unsafe_code)]
    pub fn steal() -> Peripherals {
        unsafe {
            Peripherals {
                rcu: Rcu::steal(),
                gpioa: Gpioa::steal(),
                gpiob: Gpiob::steal(),
                gpioc: Gpioc::steal(),
                gpiod: Gpiod::steal(),
                gpioe: Gpioe::steal(),
                uart6: Uart6::steal(),
                spi2: Spi2::steal(),
                spi3: Spi3::steal(),
                can0: Can0::steal(),
                timer1: Timer1::steal(),
                enet_mac: EnetMac::steal(),
                enet_dma: EnetDma::steal(),
            }
        }
    }
}

/// 应用主体（banner 由 bin 变体注入，控制权永不返回）
pub async fn run(banner: &'static str) -> ! {
    let p = periph::steal();

    // 时间驱动初始化（TIMER1@1MHz）——缺失则 Ticker 闹钟永不触发
    embassy_gd32::init_time_driver();

    let rcc = Rcc::new(&p.rcu);
    rcc.enable_gpio_port(Port::A);
    rcc.enable_gpio_port(Port::B);
    rcc.enable_gpio_port(Port::C);
    rcc.enable_gpio_port(Port::D);
    rcc.enable_gpio_port(Port::E);
    rcc.enable_spi2();
    rcc.enable_uart6();

    // UART6 console
    let _utx = Pin::alternate(&p.gpioe, 7, 8);
    let _urx = Pin::alternate(&p.gpioe, 8, 8);
    let uart = Uart::new(&p.uart6);
    uart.enable(PCLK1_HZ, BAUD);
    // 切换中断驱动接收（RXNE ISR -> 环；此后 read_byte 不可用）
    embassy_gd32::usart::uart6_ring_enable(&p.uart6);

    // CPLD（SPI2 + CS=PA15 + RST=PA4）
    let _sck = Pin::alternate(&p.gpioc, 10, 6);
    let _miso = Pin::alternate(&p.gpioc, 11, 6);
    let _mosi = Pin::alternate(&p.gpioc, 12, 6);
    let mut cs = Pin::output(&p.gpioa, 15);
    cs.set_high();
    let mut rst = Pin::output(&p.gpioa, 4);
    rst.set_high();
    cpld::Cpld::hard_reset(&mut rst);
    delay_ms(5);
    let spi = Spi::new(&p.spi2);
    spi.enable_master(PCLK1_HZ, 500_000);
    let mut cpld_dev = cpld::Cpld::new(&spi, &mut cs);

    // CAN0（PD0=RX / PD1=TX AF9，500k，正常模式）
    rcc.enable_gpio_port(Port::D);
    rcc.enable_can0();
    let _crx = Pin::alternate(&p.gpiod, 0, 9);
    let _ctx = Pin::alternate(&p.gpiod, 1, 9);
    let can0 = Can::new(&p.can0, &p.can0);
    if can0.init_loopback(&embassy_gd32::can::BitTiming::kbps500()).is_err() {
        put_str(&uart, "can0 init failed (continuing without CAN)\r\n");
    }

    // SPI3 Flash（W25Q128：SCK=PE2/MOSI=PE5/MISO=PE6 AF5，CS=PE4）
    rcc.enable_spi3();
    let _fsck = Pin::alternate(&p.gpioe, 2, 5);
    let _fmosi = Pin::alternate(&p.gpioe, 5, 5);
    let _fmiso = Pin::alternate(&p.gpioe, 6, 5);
    let mut fcs = Pin::output(&p.gpioe, 4);
    fcs.set_high();
    let fspi = Spi::new(&p.spi3);
    fspi.enable_master(PCLK1_HZ, 2_000_000);
    let mut w25q = W25q::new(&fspi, &mut fcs);

    // DMS OUT×8（OUT1-7=PE9-15，OUT8=PB10）、CTRL1-2=PB3/PB4
    let mut outs_e: [Pin; 7] = [
        Pin::output(&p.gpioe, 9),
        Pin::output(&p.gpioe, 10),
        Pin::output(&p.gpioe, 11),
        Pin::output(&p.gpioe, 12),
        Pin::output(&p.gpioe, 13),
        Pin::output(&p.gpioe, 14),
        Pin::output(&p.gpioe, 15),
    ];
    let mut out8 = Pin::output(&p.gpiob, 10);
    let mut ctrl1 = Pin::output(&p.gpiob, 3);
    let mut ctrl2 = Pin::output(&p.gpiob, 4);
    // DMS IN×8（IN1-6=PD10-15，IN7-8=PC6/7）
    let ins_d: [Pin; 6] = [
        Pin::input(&p.gpiod, 10),
        Pin::input(&p.gpiod, 11),
        Pin::input(&p.gpiod, 12),
        Pin::input(&p.gpiod, 13),
        Pin::input(&p.gpiod, 14),
        Pin::input(&p.gpiod, 15),
    ];
    let ins_c: [Pin; 2] = [Pin::input(&p.gpioc, 6), Pin::input(&p.gpioc, 7)];

    put_str(&uart, banner);

    let mut line = [0u8; LINE_MAX];
    let mut n = 0usize;
    let mut ticker = embassy_time::Ticker::every(embassy_time::Duration::from_millis(10));

    loop {
        ticker.next().await;
        // 每拍整拍排空接收环（ISR 持续入队；10ms@115200 最多累积 ~117 字节，
        // 单字节/拍模式在 OTA 大流量写入时会 RX 环溢出）
        while let Some(ch) = embassy_gd32::usart::uart6_ring_pop() {
            match ch {
                b'\n' | b'\r' => {
                if n > 0 {
                    let mut copy = [0u8; LINE_MAX];
                    copy[..n].copy_from_slice(&line[..n]);
                    let mut toks: [&[u8]; 8] = [b""; 8];
                    let nt = tokenize(&copy[..n], &mut toks);
                    // 命令分发
                    if nt >= 1 && eq(toks[0], "ping") {
                        put_str(&uart, "OK pong\r\n");
                    } else if nt >= 2 && eq(toks[0], "cpld") && eq(toks[1], "mux") {
                        if nt >= 3 && eq(toks[2], "get") {
                            match cpld_dev.get_uart_mux() {
                                Ok(v) => {
                                    put_str(&uart, "OK 0x");
                                    put_u8(&uart, v as u8);
                                    put_str(&uart, "\r\n");
                                }
                                Err(_) => put_str(&uart, "ERR cpld-bus\r\n"),
                            }
                        } else if nt >= 4 && eq(toks[2], "set") {
                            match parse_u8(toks[3]) {
                                Some(t) if (0x80..=0x85).contains(&t) => {
                                    match cpld_dev.set_uart_mux(t as u16) {
                                        Ok(_) => put_str(&uart, "OK\r\n"),
                                        Err(_) => put_str(&uart, "ERR cpld-bus\r\n"),
                                    }
                                }
                                _ => put_str(&uart, "ERR arg (0x80-0x85)\r\n"),
                            }
                        } else {
                            put_str(&uart, "ERR usage: cpld mux get|set <val>\r\n");
                        }
                    } else if nt >= 2 && eq(toks[0], "out") {
                        // out <n(1-8)> <0|1>（3 token）
                        if nt >= 3 {
                            let idx = match parse_u8(toks[1]) {
                                Some(v) if (1..=8).contains(&v) => (v - 1) as usize,
                                _ => {
                                    put_str(&uart, "ERR arg n(1-8)\r\n");
                                    n = 0;
                                    continue;
                                }
                            };
                            let lvl = eq(toks[2], "1");
                            if idx < 7 {
                                if lvl {
                                    outs_e[idx].set_high();
                                } else {
                                    outs_e[idx].set_low();
                                }
                            } else if lvl {
                                out8.set_high();
                            } else {
                                out8.set_low();
                            }
                            put_str(&uart, "OK\r\n");
                        } else {
                            put_str(&uart, "ERR usage: out <n> <0|1>\r\n");
                        }
                    } else if nt >= 1 && eq(toks[0], "in") {
                        let mut in_byte = 0u8;
                        for (i, pin) in ins_d.iter().enumerate() {
                            if pin.input_level() {
                                in_byte |= 1 << i;
                            }
                        }
                        for (i, pin) in ins_c.iter().enumerate() {
                            if pin.input_level() {
                                in_byte |= 1 << (6 + i);
                            }
                        }
                        put_str(&uart, "OK 0x");
                        put_u8(&uart, in_byte);
                        put_str(&uart, "\r\n");
                    } else if nt >= 2 && eq(toks[0], "ctrl") {
                        // ctrl <1|2> <0|1>（3 token）
                        if nt >= 3 {
                            let which = parse_u8(toks[1]);
                            let lvl = eq(toks[2], "1");
                            match which {
                                Some(1) => {
                                    if lvl {
                                        ctrl1.set_high();
                                    } else {
                                        ctrl1.set_low();
                                    }
                                    put_str(&uart, "OK\r\n");
                                }
                                Some(2) => {
                                    if lvl {
                                        ctrl2.set_high();
                                    } else {
                                        ctrl2.set_low();
                                    }
                                    put_str(&uart, "OK\r\n");
                                }
                                _ => put_str(&uart, "ERR arg ctrl(1|2)\r\n"),
                            }
                        } else {
                            put_str(&uart, "ERR usage: ctrl <1|2> <0|1>\r\n");
                        }
                    } else if nt >= 2 && eq(toks[0], "flash") {
                        if nt >= 2 && eq(toks[1], "jedec") {
                            let id = w25q.jedec_id();
                            put_str(&uart, "OK ");
                            put_hex4(&uart, u16::from(id[0]) << 8 | u16::from(id[1]));
                            uart.write_byte(b' ');
                            put_hex4(&uart, u16::from(id[2]));
                            uart.write(b"\r\n");
                        } else if nt >= 4 && eq(toks[1], "read") {
                            // flash read <addr6hex> <len_dec(1-128)>
                            let addr = match parse_u32_hex(toks[2]) {
                                Some(v) => v,
                                None => {
                                    put_str(&uart, "ERR addr\r\n");
                                    n = 0;
                                    continue;
                                }
                            };
                            let cnt = match parse_u8(toks[3]) {
                                Some(v) if (1..=128).contains(&v) => v as usize,
                                _ => {
                                    put_str(&uart, "ERR len (1-128)\r\n");
                                    n = 0;
                                    continue;
                                }
                            };
                            let mut buf = [0u8; 128];
                            w25q.read(addr, &mut buf[..cnt]);
                            put_str(&uart, "OK");
                            for &b in buf[..cnt].iter() {
                                uart.write_byte(b' ');
                                put_u8(&uart, b);
                            }
                            put_str(&uart, "\r\n");
                        } else if nt >= 3 && eq(toks[1], "se") {
                            // flash se <addr6hex>——4K 扇区擦除
                            let addr = match parse_u32_hex(toks[2]) {
                                Some(v) if v % 4096 == 0 => v,
                                _ => {
                                    put_str(&uart, "ERR addr (4K aligned)\r\n");
                                    n = 0;
                                    continue;
                                }
                            };
                            w25q.erase_sector(addr);
                            put_str(&uart, "OK\r\n");
                        } else if nt >= 4 && eq(toks[1], "wr") {
                            // flash wr <addr6hex> <hexbytes..>（须已擦除；≤32B/命令）
                            let addr = match parse_u32_hex(toks[2]) {
                                Some(v) if (v as usize) < 16 * 1024 * 1024 => v,
                                _ => {
                                    put_str(&uart, "ERR addr\r\n");
                                    n = 0;
                                    continue;
                                }
                            };
                            let mut data = [0u8; 80];
                            let mut len = 0usize;
                            let mut parse_err = false;
                            for t in toks[3..nt].iter() {
                                match parse_hex_bytes(t, &mut data[len..]) {
                                    Some(k) => len += k,
                                    None => {
                                        parse_err = true;
                                        break;
                                    }
                                }
                            }
                            if parse_err || len == 0 {
                                put_str(&uart, "ERR data hex\r\n");
                                n = 0;
                                continue;
                            }
                            if addr as usize + len > 16 * 1024 * 1024 {
                                put_str(&uart, "ERR range\r\n");
                                n = 0;
                                continue;
                            }
                            w25q.write(addr, &data[..len]);
                            put_str(&uart, "OK ");
                            put_u8(&uart, len as u8);
                            put_str(&uart, "\r\n");
                        } else if nt >= 4 && eq(toks[1], "crc") {
                            // flash crc <addr6hex> <len_dec(1-32768)>——区间 CRC-32
                            let addr = match parse_u32_hex(toks[2]) {
                                Some(v) => v,
                                _ => {
                                    put_str(&uart, "ERR addr\r\n");
                                    n = 0;
                                    continue;
                                }
                            };
                            let len = match parse_dec_u32(toks[3]) {
                                Some(v) if (1..=32768).contains(&v) => v,
                                _ => {
                                    put_str(&uart, "ERR len (1-32768)\r\n");
                                    n = 0;
                                    continue;
                                }
                            };
                            if addr as u64 + len as u64 > 16 * 1024 * 1024 {
                                put_str(&uart, "ERR range\r\n");
                                n = 0;
                                continue;
                            }
                            let mut c = Crc32::init();
                            let mut buf = [0u8; PAGE_SIZE];
                            let mut off = 0u32;
                            while off < len {
                                let k = core::cmp::min(PAGE_SIZE as u32, len - off) as usize;
                                w25q.read(addr + off, &mut buf[..k]);
                                c.update(&buf[..k]);
                                off += k as u32;
                            }
                            put_str(&uart, "OK ");
                            put_hex8(&uart, c.final_crc());
                            put_str(&uart, "\r\n");
                        } else {
                            put_str(&uart, "ERR usage: flash jedec|read|se|wr|crc\r\n");
                        }
                    } else if nt >= 2 && eq(toks[0], "ota") && eq(toks[1], "boot") {
                        // 软复位进 bootloader（升级流程触发点）
                        put_str(&uart, "OK rebooting\r\n");
                        delay_ms(100); // 等 UART TC
                        SCB::sys_reset();
                    } else if nt >= 2 && eq(toks[0], "can0") {
                        if eq(toks[1], "recv") {
                            match can0.recv() {
                                Some(f) => {
                                    put_str(&uart, "OK 0x");
                                    put_hex4(&uart, f.id);
                                    uart.write_byte(b' ');
                                    put_u8(&uart, f.len);
                                    for &b in f.data[..f.len as usize].iter() {
                                        uart.write_byte(b' ');
                                        put_u8(&uart, b);
                                    }
                                    put_str(&uart, "\r\n");
                                }
                                None => put_str(&uart, "OK empty\r\n"),
                            }
                        } else if nt >= 3 && eq(toks[1], "send") {
                            // can0 send <id> <db0> [db1..db7]（全部 hex）
                            let id = match parse_u16_hex(toks[2]) {
                                Some(v) if v <= 0x7FF => v,
                                _ => {
                                    put_str(&uart, "ERR id (0-7FF)\r\n");
                                    n = 0;
                                    continue;
                                }
                            };
                            // 数据 = 各 token 的 hex 串按字节对解析合并
                            let mut data = [0u8; 8];
                            let mut len = 0usize;
                            let mut parse_err = false;
                            for t in toks[3..nt].iter() {
                                let need = (t.len() + 1) / 2;
                                if len + need > 8 {
                                    put_str(&uart, "ERR data >8 bytes\r\n");
                                    parse_err = true;
                                    break;
                                }
                                match parse_hex_bytes(t, &mut data[len..]) {
                                    Some(k) => len += k,
                                    None => {
                                        parse_err = true;
                                        break;
                                    }
                                }
                            }
                            if parse_err {
                                n = 0;
                                continue;
                            }
                            if len == 0 {
                                put_str(&uart, "ERR need >=1 data byte\r\n");
                                n = 0;
                                continue;
                            }
                            let frame = Frame { id, len: len as u8, data };
                            if can0.send(&frame) {
                                put_str(&uart, "OK sent\r\n");
                            } else {
                                put_str(&uart, "ERR send-timeout\r\n");
                            }
                        } else {
                            put_str(&uart, "ERR usage: can0 send <id> <bytes..>\r\n");
                        }
                    } else {
                        put_str(&uart, "ERR unknown cmd\r\n");
                    }
                    n = 0;
                }
            }
                ch if ch != b'\r' => {
                    if n < LINE_MAX {
                        line[n] = ch;
                        n += 1;
                    }
                }
                _ => {}
            }
        }
    }
}
