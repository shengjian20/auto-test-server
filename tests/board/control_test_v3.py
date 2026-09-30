#!/usr/bin/env python3
"""control-server v3 验收：flash jedec/read + can0 recv 新命令 + 原命令回归"""
import serial, sys, time

s = serial.Serial("/dev/ttyUSB0", 115200, timeout=0.5)
time.sleep(0.3)

def cmd(line, wait=0.4):
    s.write(line.encode() + b"\n")
    s.flush()
    time.sleep(wait)
    buf = b""
    t0 = time.time()
    while time.time() - t0 < wait and b"\n" not in buf:
        buf += s.read(256)
    return buf.decode(errors="replace").strip()

ok = True

# 新：flash jedec（期望 EF 40 18 = W25Q128，阶段 2c 已定案）
r = cmd("flash jedec")
print(f"flash jedec -> {r!r}")
if "EF40 0018" not in r: ok = False

# 新：flash read（地址 0 读 8 字节，W25Q 出厂全 FF）
r = cmd("flash read 0000 8")
print(f"flash read 0000 8 -> {r!r}")
if "OK 63 61 72 72 69 65 72 5F" not in r: ok = False  # "carrier_"

# 新：can0 recv（空 FIFO 预期 OK empty）
r = cmd("can0 recv")
print(f"can0 recv -> {r!r}")
if "OK" not in r: ok = False

# 回归：ping / cpld mux / out / in / ctrl
r = cmd("ping")
if "OK pong" not in r: ok = False
cmd("cpld mux set 81")
r = cmd("cpld mux get")
if "01" not in r: ok = False
cmd("cpld mux set 80")
r = cmd("out 1 1")
if "OK" not in r: ok = False
r = cmd("in")
if "OK 0x" not in r: ok = False
r = cmd("ctrl 2 1")
if "OK" not in r: ok = False
print("regress (ping/cpld/out/in/ctrl) -> OK")

print("CONTROL_V3:", "PASS" if ok else "FAIL")
sys.exit(0 if ok else 1)
