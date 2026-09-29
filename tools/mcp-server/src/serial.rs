//! 串口传输封装：UART6 线协议的命令/应答事务

use serialport::SerialPort;
use std::io::{self, Read, Write};
use std::time::{Duration, Instant};

pub struct SerialLine {
    port: Box<dyn SerialPort>,
}

impl SerialLine {
    pub fn open(path: &str, baud: u32) -> serialport::Result<Self> {
        let port = serialport::new(path, baud)
            .timeout(Duration::from_millis(500))
            .open()?;
        Ok(Self { port })
    }

    /// 清空输入缓冲（丢弃残留 banner 等）
    pub fn flush_input(&mut self) -> io::Result<()> {
        self.port
            .clear(serialport::ClearBuffer::Input)
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e.to_string()))
    }

    /// 事务：发命令行 -> 收响应行（\n 结尾，带总超时）
    pub fn transact(&mut self, cmd: &str) -> Result<String, String> {
        self.port
            .write_all(cmd.as_bytes())
            .and_then(|_| self.port.write_all(b"\n"))
            .and_then(|_| self.port.flush())
            .map_err(|e| format!("write: {e}"))?;

        let mut buf = String::new();
        let start = Instant::now();
        let timeout = Duration::from_millis(1500);
        // 逐字节读到 \n（TBU 场景 send-timeout 可能 1s+）
        let mut byte = [0u8; 1];
        while start.elapsed() < timeout {
            match self.port.read(&mut byte) {
                Ok(1) => {
                    if byte[0] == b'\n' {
                        return Ok(buf.trim_end().to_string());
                    }
                    buf.push(byte[0] as char);
                }
                Ok(_) => {} // 0 字节 = 超时 tick，继续等总窗口
                Err(e) if e.kind() == io::ErrorKind::TimedOut => {}
                Err(e) => return Err(format!("read: {e}")),
            }
            if buf.contains("ERR") && start.elapsed() > Duration::from_millis(200) {
                // 错误响应即刻返回（缩短失败路径延迟）
                return Ok(buf.trim_end().to_string());
            }
        }
        if buf.is_empty() {
            Err("timeout: no response".into())
        } else {
            Ok(buf.trim_end().to_string())
        }
    }
}

// ---- TCP 传输（control-server-tcp :9000，协议同一来源）----

use std::net::TcpStream;

pub struct TcpLine {
    stream: TcpStream,
}

impl TcpLine {
    pub fn open(addr: &str) -> io::Result<Self> {
        let stream = TcpStream::connect(addr)?;
        stream.set_read_timeout(Some(Duration::from_millis(200)))?;
        // NODELAY：命令行单包发送——Nagle 会把 cmd 与 "\n" 合并延迟达
        // 40ms，配合板侧按拍轮询造成分段撕裂/事务超时抖动
        stream.set_nodelay(true)?;
        Ok(Self { stream })
    }

    pub fn flush_input(&mut self) -> io::Result<()> {
        // TCP 无输入缓冲清空概念：丢弃既达数据（非阻塞读至空）
        self.stream.set_nonblocking(true)?;
        let mut sink = [0u8; 512];
        while let Ok(n @ 1..) = self.stream.read(&mut sink) {
            let _ = n;
        }
        self.stream.set_nonblocking(false)?;
        Ok(())
    }

    pub fn transact(&mut self, cmd: &str) -> Result<String, String> {
        // 命令行含换行单包发送（分次 write 在 Nagle 下拆段，板侧残尾
        // 拼接虽可续齐，单包更稳）
        let mut pkt = String::with_capacity(cmd.len() + 1);
        pkt.push_str(cmd);
        pkt.push('\n');
        self.stream
            .write_all(pkt.as_bytes())
            .and_then(|_| self.stream.flush())
            .map_err(|e| format!("write: {e}"))?;

        let mut buf = String::new();
        let start = Instant::now();
        let timeout = Duration::from_millis(1500);
        let mut byte = [0u8; 1];
        while start.elapsed() < timeout {
            match self.stream.read(&mut byte) {
                Ok(1) => {
                    if byte[0] == b'\n' {
                        return Ok(buf.trim_end().to_string());
                    }
                    buf.push(byte[0] as char);
                }
                Ok(_) => {}
                Err(e) if e.kind() == io::ErrorKind::WouldBlock || e.kind() == io::ErrorKind::TimedOut => {}
                Err(e) => return Err(format!("read: {e}")),
            }
        }
        if buf.is_empty() {
            Err("timeout: no response".into())
        } else {
            Ok(buf.trim_end().to_string())
        }
    }
}

/// 传输抽象（串口/TCP 双形态；协议同一来源，dispatch_tool 不感知差异）
pub enum AnyLine {
    Serial(SerialLine),
    Tcp(TcpLine),
}

impl AnyLine {
    pub fn flush_input(&mut self) -> io::Result<()> {
        match self {
            AnyLine::Serial(s) => s.flush_input(),
            AnyLine::Tcp(t) => t.flush_input(),
        }
    }

    pub fn transact(&mut self, cmd: &str) -> Result<String, String> {
        match self {
            AnyLine::Serial(s) => s.transact(cmd),
            AnyLine::Tcp(t) => t.transact(cmd),
        }
    }
}
