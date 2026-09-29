//! control-server：阶段 4 前置——UART6 串行控制协议（文本行协议，与未来
//! TCP 版共用帧格式）
//!
//! 传输层：UART6 @ PE7/PE8 AF8 115200（已验收的缓冲回显同款传输参数），
//! 行协议按 \n 分帧（空闲判定由缓冲收集保证）。
//!
//! 协议：
//!   请求: ping | cpld mux get | cpld mux set <0x80-0x85> | out <n> <0|1>
//!         | in | ctrl <1|2> <0|1>
//!   响应: OK [values...] | ERR <msg>
//!
//! 外设覆盖（阶段 2 已验收 HAL）：cpld（uart_mux）、out×8（DMS SSR）、
//! in×8（DMS 光耦读）、ctrl×2。
#![no_std]
#![no_main]
#![deny(unsafe_code)]

use embassy_gd32::can::{Can, Frame};
use embassy_gd32::{cpld, Pin, Port, Rcc, Spi, Uart};
use panic_halt as _;

const PCLK1_HZ: u32 = 16_000_000;
const BAUD: u32 = 115_200;
const LINE_MAX: usize = 96;

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

fn parse_u8_hex(t: &[u8]) -> Option<u8> {
    if t.len() != 2 {
        return None;
    }
    let mut v: u8 = 0;
    for &c in t {
        v = (v << 4) + (c as char).to_digit(16)? as u8;
    }
    Some(v)
}

/// hex 串按字节对解析到 buf（"1a2b3c" -> [0x1a,0x2b,0x3c]），返回字节数。
/// 奇数长度/非法字符返回 None。
fn parse_hex_bytes(t: &[u8], buf: &mut [u8]) -> Option<usize> {
    if t.is_empty() || t.len() % 2 != 0 || t.len() > buf.len() * 2 {
        return None;
    }
    let mut n = 0usize;
    let mut i = 0usize;
    while i < t.len() {
        let hi = (t[i] as char).to_digit(16)? as u8;
        let lo = (t[i + 1] as char).to_digit(16)? as u8;
        buf[n] = (hi << 4) | lo;
        n += 1;
        i += 2;
    }
    Some(n)
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

#[cortex_m_rt::entry]
fn main() -> ! {
    let p = gd32f470::Peripherals::take().expect("peripherals already taken");

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

    put_str(&uart, "control-server ready\r\n");

    let mut line = [0u8; LINE_MAX];
    let mut n = 0usize;

    loop {
        match uart.read_byte() {
            Some(b'\n') | Some(b'\r') => {
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
                    } else if nt >= 2 && eq(toks[0], "can0") {
                        if nt >= 3 && eq(toks[1], "send") {
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
                            // （兼容 "11223344" 连写与 "11 22 33 44" 空格分隔）
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
            Some(ch) if ch != b'\r' => {
                if n < LINE_MAX {
                    line[n] = ch;
                    n += 1;
                }
            }
            _ => {}
        }
    }
}
