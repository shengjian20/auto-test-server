import serial, subprocess, time

ser = serial.Serial('/dev/ttyUSB0', 115200, timeout=1)
ser.reset_input_buffer()
# 同容器：openocd 复位 + pyserial 读（无 USB 竞争）
r = subprocess.run(['openocd', '-f', '/probe/reset_only.cfg'],
                   capture_output=True, timeout=25)
end = time.time() + 3
data = b''
while time.time() < end:
    chunk = ser.read(256)
    if chunk:
        data += chunk
        end = max(end, time.time() + 1)
ser.reset_input_buffer()

def xact(cmd, wait=1.5):
    ser.write((cmd + '\n').encode())
    end = time.time() + wait
    d = b''
    while time.time() < end:
        chunk = ser.read(256)
        if chunk:
            d += chunk
            end = max(end, time.time() + 0.5)
    return d.decode(errors='replace')

r1 = xact('help')
print('help:', repr(r1[:150]))
r2 = xact('echo streaming test')
print('echo:', repr(r2[:120]))
r3 = xact('delay 500', wait=2.0)
print('delay:', repr(r3[:150]))
ser.close()

ok = 'commands: help/echo/delay/reboot' in r1 and 'streaming test' in r2 and 'actual' in r3
print('MSH_INTERACTIVE:', 'PASS' if ok else 'FAIL')
