#!/usr/bin/env python3
"""uart-echo 验收：UART6 (PC_RS232_1) 经 /dev/ttyUSB0 的 PC 侧环回测试"""
import serial, sys, time

PORT = "/dev/ttyUSB1"
BAUD = 115200

try:
    s = serial.Serial(PORT, BAUD, timeout=2)
except PermissionError as e:
    print(f"SKIP: no permission on {PORT}: {e}")
    sys.exit(2)

# 1. banner 检查（固件 reset 后发 "uart-echo ready\r\n"；若刚烧录可能已错过，发个换行不回显无妨）
s.reset_input_buffer()
banner = s.read(64)
print(f"banner: {banner!r}")

# 2. 回显比对：多轮，含边界字节
patterns = [bytes(range(256)), b"hello-carrier\r\n", b"\x00\xff\x55\xaa" * 16]
ok = True
for i, pat in enumerate(patterns):
    s.reset_input_buffer()
    s.write(pat)
    s.flush()
    rx = s.read(len(pat) + 8)
    if rx == pat:
        print(f"pattern{i}: PASS ({len(pat)} bytes)")
    else:
        print(f"pattern{i}: FAIL tx={len(pat)} rx={len(rx)} {rx[:32]!r}")
        ok = False
    time.sleep(0.05)

print("ECHO_TEST:", "PASS" if ok else "FAIL")
sys.exit(0 if ok else 1)
