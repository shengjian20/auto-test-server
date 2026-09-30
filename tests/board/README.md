# 板上验收脚本（PC 侧执行）

依赖前提：
- 宿主机 Python3 + pyserial（`pip install pyserial`）
- 板卡经 USB 连接（probe-rs 识别 GD32F470VG）
- 固件已烧录（各脚本对应的固件见 `src/firmware/`，烧录用
  `probe-rs download --chip GD32F470VG --verify <bin>` 或 openocd）
- 对应串口设备 `/dev/ttyUSB0`（ATEN，UART6 console）在线

## 脚本清单（按验收阶段）

| 脚本 | 固件 | 验收目标 | 证据 |
|---|---|---|---|
| `echo_test.py` | uart-echo | UART6 缓冲回显（256B 连发不丢字节） | ECHO_TEST: PASS |
| `control_test_v3.py` | control-server | 全外设文本行协议（ping/cpld/out/in/ctrl/flash/can0） | CONTROL_V2: PASS |
| `cpld_test.py` | cpld-test | CPLD 协议（uart_mux 读写往返 + 0x55AA 应答魔数） | CPLD_TEST: PASS |
| `dms_test.py` | dms-io-test | DMS OUT×8/IN×8/CTRL×2 状态流 | console 状态流 |
| `can_loopback_test.py` | can-loopback | bxCAN0 回环三帧（标准/最大 ID/零长） | CAN_LOOPBACK: PASS |
| `tcp_selftest2.py` | tcp-echo | smoltcp 全栈 LBM 自测（ARP+SYN+DATA 256B） | TCP_SELFTEST: PASS |
| `link_check.py` | enet-link-test | PHY ID + 自协商 + 链路状态 | console link=UP/DOWN |
| `bl3_e2e.py` | bootloader v3 | 串口触发升级四路径（超时/触发/协议/软复位闭环） | BL3_E2E: PASS |
| `ota_tcp_write2.py` | control-server-tcp | OTA 镜像经 TCP 写 W25Q（擦/写/CRC 三段校验） | OTA_TCP_WRITE: PASS |

## 通用模式

所有脚本遵循"先开串口 → probe-rs reset 重放 banner → 收结果"的
既定时序（UART banner 只在上电时发一次——教训 #6）。

## 已知环境约束

- 串口设备权限：宿主机用户需在 dialout 组，或经 docker 容器
  （`--device /dev/ttyUSB0` + root）
- rsproxy 抖动：容器构建挂载持久 CARGO_HOME（`.cargo-home/`）
- USB 串口设备号可能互换：以 VID:PID 为准（ATEN=067b:23a3，
  CH340=1a86:7523），udev 别名或重新枚举后核对
