#!/usr/bin/env python3
"""can-loopback 验收：先开串口 -> probe-rs reset 重放单轮输出 -> 收结果"""
import serial, subprocess, sys, time

s = serial.Serial("/dev/ttyUSB0", 115200, timeout=0.5)

r = subprocess.run(["probe-rs", "reset", "--chip", "GD32F470VG"],
                   capture_output=True, text=True, timeout=30)
print(f"reset rc={r.returncode}")

buf = b""
t0 = time.time()
while time.time() - t0 < 6.0 and b"CAN_LOOPBACK:" not in buf:
    buf += s.read(256)
s.close()
print("console:")
for line in buf.split(b"\r\n"):
    if line: print(" ", line.decode(errors="replace"))
print("RESULT:", "PASS" if b"CAN_LOOPBACK: PASS" in buf else ("FAIL" if b"CAN_LOOPBACK" in buf else "SILENT"))
