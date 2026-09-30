import serial, subprocess, time

# 1. reset（短超时，挂起即抛）
subprocess.run(['probe-rs', 'reset', '--chip', 'GD32F470VG'],
               capture_output=True, timeout=20)
ser = serial.Serial('/dev/ttyUSB0', 115200, timeout=1)
ser.reset_input_buffer()
# 复位后固件自报告 + 自测全程 25s
end = time.time() + 25
data = b''
while time.time() < end:
    chunk = ser.read(256)
    if chunk:
        data += chunk
        end = max(end, time.time() + 4)
ser.close()
out = data.decode(errors='replace')
print(out)
ok = 'TCP_SELFTEST: PASS (256/256)' in out
print('VERDICT:', 'PASS' if ok else 'FAIL')
