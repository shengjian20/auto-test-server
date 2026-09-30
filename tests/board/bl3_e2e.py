import serial, subprocess, time, sys

ser = serial.Serial('/dev/ttyUSB0', 115200, timeout=1)

# 1. 复位，抓 banner + UPGR? 提示（500ms 窗口内不发，让它超时走常规路径）
ser.reset_input_buffer()
subprocess.run(['probe-rs', 'reset', '--chip', 'GD32F470VG'], capture_output=True, timeout=20)
end = time.time() + 8
data = b''
while time.time() < end:
    chunk = ser.read(256)
    if chunk:
        data += chunk
        end = max(end, time.time() + 3)
d1 = data.decode(errors='replace')
print('--- 无触发路径 ---')
print(d1)
timeout_ok = 'upgrade done' not in d1 and 'bootloader v3' in d1

# 2. 复位前持续连发 UPGR（每 60ms 一次共 3.5s）——无论板子何时进入
#    2s 窗口，必然收到完整魔数；多发的魔数进升级模式行缓冲返回 ERR，
#    无害
ser.reset_input_buffer()
import threading
stop_spray = threading.Event()
def spray():
    while not stop_spray.is_set():
        ser.write(b'UPGR')
        time.sleep(0.06)
t = threading.Thread(target=spray, daemon=True)
t.start()
subprocess.run(['probe-rs', 'reset', '--chip', 'GD32F470VG'], capture_output=True, timeout=20)
time.sleep(3.0)   # spray 覆盖板子整个触发窗口（4-6s 的前 3s 内必然收齐）
stop_spray.set()
time.sleep(1.0)   # 等 upgrade mode 回显
r = ser.read(ser.in_waiting or 256).decode(errors='replace')
if 'upgrade mode' not in r:
    time.sleep(1.0)
    r += ser.read(ser.in_waiting or 256).decode(errors='replace')
print('after UPGR spray:', repr(r))
if 'upgrade mode' not in r:
    # 未触发：窗口仍在计时，等它超时跳转，后续协议测试标记 FAIL
    time.sleep(4)
else:
    # 已入升级模式：等 500ms 让 spray 尾字节落地，发 \n 清半行，
    # 再清一次输入缓冲（升级模式回显全部丢弃）
    time.sleep(0.5)
    ser.write(b'\n')
    time.sleep(0.3)
    ser.reset_input_buffer()
r = ser.read(ser.in_waiting or 64).decode(errors='replace')
print('--- 触发路径 ---')
print('after UPGR:', repr(r))
pass  # enter_ok 判定移至协议测试后

# 3. 升级模式协议测试：se 0 -> wr 0 55AA... -> crc 0 4
def xact(cmd, wait=1.0):
    ser.write((cmd + '\n').encode())
    end = time.time() + wait
    d = b''
    while time.time() < end:
        chunk = ser.read(128)
        if chunk:
            d += chunk
            end = max(end, time.time() + 0.3)
    return d.decode(errors='replace').strip()

ser.reset_input_buffer()
# 先发 \n 冲掉行缓冲里 spray 残留的 UPGRUPGR...（残行会污染首个命令）
xact('', wait=0.5)
r_se = xact('se 000000', wait=2.0)
r_wr = xact('wr 000000 55AA55AA', wait=1.0)
r_crc = xact('crc 000000 4', wait=1.0)
r_boot = xact('boot', wait=1.0)
print(f'se: {r_se!r} wr: {r_wr!r} crc: {r_crc!r} boot: {r_boot!r}')
import zlib
crc_expect = f'{zlib.crc32(bytes([0x55,0xAA,0x55,0xAA])) & 0xFFFFFFFF:08X}'
proto_ok = ('OK' in r_se and 'OK' in r_wr and crc_expect in r_crc.upper())
enter_ok = 'upgrade mode' in r or ('OK' in r_se and 'OK' in r_wr)

# 4. boot 软复位后走常规路径（搬运/跳转）——boot 响应里已含第二次
#    banner（r_boot），这里只补读 app 跳转段（若有）
end = time.time() + 8
data = b''
while time.time() < end:
    chunk = ser.read(256)
    if chunk:
        data += chunk
        end = max(end, time.time() + 3)
d4 = data.decode(errors='replace')
print('--- boot 后 ---')
print(d4[:400])
reboot_ok = ('bootloader v3' in (d4 + r_boot) or 'control-server' in (d4 + r_boot)
             or 'app OK' in (d4 + r_boot) or 'ota: app up-to-date' in (d4 + r_boot))

ser.close()
print()
print(f'TIMEOUT_PATH: {"PASS" if timeout_ok else "FAIL"}')
print(f'TRIGGER_PATH: {"PASS" if enter_ok else "FAIL"}')
print(f'PROTO_PATH: {"PASS" if proto_ok else "FAIL"}')
print(f'REBOOT_PATH: {"PASS" if reboot_ok else "FAIL"}')
print('BL3_E2E:', 'PASS' if (timeout_ok and enter_ok and proto_ok) else 'FAIL')
