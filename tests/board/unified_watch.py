import serial, subprocess, time

ser = serial.Serial('/dev/ttyUSB0', 115200, timeout=1)
ser.reset_input_buffer()
# 同容器内 openocd 复位（无跨容器 USB 竞争）
r = subprocess.run(['openocd', '-f', '/probe/reset_only.cfg'],
                   capture_output=True, timeout=25)
print('openocd rc=', r.returncode, flush=True)
end = time.time() + 12
data = b''
while time.time() < end:
    chunk = ser.read(256)
    if chunk:
        data += chunk
        end = max(end, time.time() + 3)
ser.close()
print(data.decode(errors='replace')[:500])
