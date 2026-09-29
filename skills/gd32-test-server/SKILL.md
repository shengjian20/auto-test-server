---
name: gd32-test-server
description: 控制载具测试服务器（GD32F470）：串口线协议经 MCP server 操控 CPLD/DMS IO/CAN0/W25Q Flash。当用户要求操作测试板外设（UART mux 切换、DMS 输出/输入、CAN 发送、Flash 读取）或查询板卡状态时触发。
---

# GD32F470 自动测试服务器控制

通过 MCP stdio server（`tools/mcp-server`）控制载具测试板外设。

## 启动

```bash
# 板子需已烧录 control-server 固件（UART6 线协议 115200）
/workspace/target/x86_64-unknown-linux-gnu/mcp-server --port /dev/ttyUSB0
# 权限不足时经 docker（板卡串口+DAPLink 透传）：
docker run --rm -i --privileged -v /media/raw/filespace/test/auto_test_server:/workspace \
  --device /dev/ttyUSB0 -v /dev/bus/usb:/dev/bus/usb auto-test-server:latest \
  /workspace/target/x86_64-unknown-linux-gnu/mcp-server --port /dev/ttyUSB0
```

stdio JSON-RPC 2.0（MCP 2024-11-05）：initialize → tools/list → tools/call。

## 工具速查

| 工具 | 作用 | 关键参数 |
|---|---|---|
| ping | 存活探测 | — |
| cpld_mux_get / cpld_mux_set | CPLD uart_mux 切换 | value: 0x80=MCU, 0x81-85=EXUART0-4 |
| out_set | DMS OUT 输出 | n=1-8, level=0/1 |
| in_read | DMS IN 读（hex 位图） | — |
| ctrl_set | DMS CTRL 输出 | n=1/2, level=0/1 |
| can0_send | CAN0 发标准帧 | id=0-7FF, data=hex 串 |
| can0_recv | CAN0 FIFO0 取帧 | — |
| flash_jedec | W25Q JEDEC ID | 期望 EF40 0018 |
| flash_read | W25Q 读 | addr=hex 高16位, len=1-128 |

## 已定案的板卡事实（勿重新猜测）

- 板：GD32F470VGT6，主 SRAM 448K@0x20000000 连续 + TCM 64K@0x10000000（仅 DBUS）
- UART6=PC_RS232_1（PE7/PE8 AF8）→ 宿主机 /dev/ttyUSB0（ATEN）
- RS485_1=USART1 PD5/PD6（网络名 UART4_TX/RX 系笔误），DIR1=PD4 **低=发送**
- SPI Flash=**W25Q128**（原理图网络名 W25Q256 系误标），SPI3 PE2/5/6+CS=PE4
- CAN0=PD0/PD1 AF9；CPLD=SPI2 PC10-12 AF6，CS=PA15，RST=PA4
- 调试：openocd 烧录（cfg 结尾必须 reset run，reset halt 会让板子假死）；
  改 memory.x 后必须 touch src 强制重链（增量构建不重链 vector table）
