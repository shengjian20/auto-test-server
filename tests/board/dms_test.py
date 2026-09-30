#!/usr/bin/env python3
"""dms-io-test 验收：先开串口 -> reset -> 抓走位说明 + IN 状态流"""
import serial, subprocess, sys, time

s = serial.Serial("/dev/ttyUSB0", 115200, timeout=0.5)
r = subprocess.run(["probe-rs", "reset", "--chip", "GD32F470VG"],
                   capture_output=True, text=True, timeout=30)
buf = b""
t0 = time.time()
while time.time() - t0 < 8.0:
    buf += s.read(256)
s.close()
print("console:")
for line in buf.split(b"\r\n"):
    if line: print(" ", line.decode(errors="replace"))
print("RESULT:", "RUNNING" if (b"dms-io-test" in buf and b"IN=0x" in buf) else "SILENT")
