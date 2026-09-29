//! mcp-server：阶段 6——MCP stdio 服务器，agent 经此控制 GD32F470 测试服务器
//!
//! 架构：stdio（JSON-RPC 2.0，MCP 2024-11-05 子集：initialize/tools/*）
//!       -> 串口传输（UART6 线协议，/control-server 已验收）
//!       -> GD32F470 外设
//!
//! 工具集（与 control-server 协议一一映射）：
//!   ping / cpld_mux_get / cpld_mux_set / out_set / in_read / ctrl_set /
//!   can0_send / can0_recv / flash_jedec / flash_read
//!
//! 用法：mcp-server [--port /dev/ttyUSB0] [--baud 115200]
#![allow(unsafe_code)]

use serde_json::{json, Value};
use std::io::{BufRead, Write};

mod serial;
use serial::SerialLine;

fn main() {
    // 参数解析（--port / --baud）
    let mut port_path = String::from("/dev/ttyUSB0");
    let mut baud: u32 = 115_200;
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--port" => {
                if let Some(v) = args.next() {
                    port_path = v;
                }
            }
            "--baud" => {
                if let Some(v) = args.next() {
                    baud = v.parse().unwrap_or(115_200);
                }
            }
            _ => {}
        }
    }

    let mut ser = SerialLine::open(&port_path, baud)
        .unwrap_or_else(|e| {
            eprintln!("serial open {port_path}: {e}");
            std::process::exit(1);
        });
    // 握手：control-server 的 banner（可选）
    let _ = ser.flush_input();

    let stdin = std::io::stdin();
    let mut line = String::new();

    loop {
        line.clear();
        if stdin.read_line(&mut line).unwrap_or(0) == 0 {
            break; // EOF
        }
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let req: Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(_) => continue, // 非 JSON 行忽略（MCP 框架的 ping 等以 JSON 到达）
        };
        let id = req.get("id").cloned();
        let method = req.get("method").and_then(|m| m.as_str()).unwrap_or("");

        let response = match method {
            "initialize" => json!({
                "jsonrpc": "2.0", "id": id,
                "result": {
                    "protocolVersion": "2024-11-05",
                    "capabilities": {"tools": {}},
                    "serverInfo": {"name": "gd32-test-server", "version": "0.1.0"}
                }
            }),
            "notifications/initialized" => continue,
            "tools/list" => json!({
                "jsonrpc": "2.0", "id": id,
                "result": {"tools": tools_schema()}
            }),
            "tools/call" => {
                let params = req.get("params").cloned().unwrap_or(json!({}));
                let name = params.get("name").and_then(|n| n.as_str()).unwrap_or("");
                let args = params.get("arguments").cloned().unwrap_or(json!({}));
                let text = dispatch_tool(&mut ser, name, &args);
                json!({
                    "jsonrpc": "2.0", "id": id,
                    "result": {"content": [{"type": "text", "text": text}], "isError": false}
                })
            }
            "ping" => json!({"jsonrpc": "2.0", "id": id, "result": {}}),
            _ => match id {
                Some(id) => json!({
                    "jsonrpc": "2.0", "id": id,
                    "error": {"code": -32601, "message": "method not found"}
                }),
                None => continue,
            },
        };
        println!("{}", response);
        let _ = std::io::stdout().flush();
    }
}

/// 工具执行分发：MCP 参数 -> 线协议命令 -> control-server 响应文本
fn dispatch_tool(ser: &mut SerialLine, name: &str, args: &Value) -> String {
    let cmd = match name {
        "ping" => "ping".to_string(),
        "cpld_mux_get" => "cpld mux get".to_string(),
        "cpld_mux_set" => {
            let v = args.get("value").and_then(|v| v.as_u64()).unwrap_or(0x80);
            format!("cpld mux set {v:02x}")
        }
        "out_set" => {
            let n = args.get("n").and_then(|v| v.as_u64()).unwrap_or(1);
            let level = args.get("level").and_then(|v| v.as_u64()).unwrap_or(0);
            format!("out {n} {level}")
        }
        "in_read" => "in".to_string(),
        "ctrl_set" => {
            let n = args.get("n").and_then(|v| v.as_u64()).unwrap_or(1);
            let level = args.get("level").and_then(|v| v.as_u64()).unwrap_or(0);
            format!("ctrl {n} {level}")
        }
        "can0_send" => {
            let id = args.get("id").and_then(|v| v.as_u64()).unwrap_or(0);
            let data = args
                .get("data")
                .and_then(|v| v.as_str())
                .unwrap_or("00");
            format!("can0 send {id:x} {data}")
        }
        "can0_recv" => "can0 recv".to_string(),
        "flash_jedec" => "flash jedec".to_string(),
        "flash_read" => {
            let addr = args.get("addr").and_then(|v| v.as_str()).unwrap_or("0000");
            let len = args.get("len").and_then(|v| v.as_u64()).unwrap_or(8);
            format!("flash read {addr} {len}")
        }
        _ => return format!("ERR unknown tool: {name}"),
    };

    match ser.transact(&cmd) {
        Ok(resp) => resp,
        Err(e) => format!("ERR serial: {e}"),
    }
}

/// MCP tools/list 的 schema（每个工具的名称/描述/参数）
fn tools_schema() -> Value {
    json!([
        {"name": "ping", "description": "存活探测（返回 OK pong）", "inputSchema": {"type": "object", "properties": {}}},
        {"name": "cpld_mux_get", "description": "读 CPLD uart_mux 当前值（0x80=MCU, 0x81-85=EXUART0-4）", "inputSchema": {"type": "object", "properties": {}}},
        {"name": "cpld_mux_set", "description": "设置 CPLD uart_mux", "inputSchema": {"type": "object", "properties": {"value": {"type": "integer", "minimum": 128, "maximum": 133}}, "required": ["value"]}},
        {"name": "out_set", "description": "DMS OUT 输出（n=1-8 对应 OUT1-8，level 0/1）", "inputSchema": {"type": "object", "properties": {"n": {"type": "integer", "minimum": 1, "maximum": 8}, "level": {"type": "integer", "minimum": 0, "maximum": 1}}, "required": ["n", "level"]}},
        {"name": "in_read", "description": "读 DMS IN1-8 状态（十六进制位图）", "inputSchema": {"type": "object", "properties": {}}},
        {"name": "ctrl_set", "description": "DMS CTRL 输出（n=1/2，level 0/1）", "inputSchema": {"type": "object", "properties": {"n": {"type": "integer", "minimum": 1, "maximum": 2}, "level": {"type": "integer", "minimum": 0, "maximum": 1}}, "required": ["n", "level"]}},
        {"name": "can0_send", "description": "CAN0 发送标准帧（id 0-7FF，data 为 hex 串如 1122334455667788）", "inputSchema": {"type": "object", "properties": {"id": {"type": "integer"}, "data": {"type": "string"}}, "required": ["id", "data"]}},
        {"name": "can0_recv", "description": "CAN0 FIFO0 取帧（空则返回 OK empty）", "inputSchema": {"type": "object", "properties": {}}},
        {"name": "flash_jedec", "description": "W25Q JEDEC ID（期望 EF40 0018 = W25Q128）", "inputSchema": {"type": "object", "properties": {}}},
        {"name": "flash_read", "description": "W25Q 读数据（addr 4 位 hex 高 16 位，len 1-128）", "inputSchema": {"type": "object", "properties": {"addr": {"type": "string"}, "len": {"type": "integer", "minimum": 1, "maximum": 128}}, "required": ["addr", "len"]}},
    ])
}
