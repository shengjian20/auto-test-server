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
