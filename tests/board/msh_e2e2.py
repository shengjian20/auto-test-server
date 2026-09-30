import serial, subprocess, time

ser = serial.Serial('/dev/ttyUSB0', 115200, timeout=1)

# 复位后完整收集（banner + prompt），不做 reset_input_buffer
subprocess.run(['probe-rs', 'reset', '--chip', 'GD32F470VG'], capture_output=True, timeout=20)
end = time.time() + 3
data = b''
while time.time() < end:
    chunk = ser.read(256)
    if chunk:
        data += chunk
        end = max(end, time.time() + 1)
print('banner:', repr(data.decode(errors='replace')[-80:]))

def xact(cmd, wait=2.0):
    ser.reset_input_buffer()          # 清残留（prompt 等）
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
print('help:', repr(r1[:260]))
r2 = xact('echo hello world')
print('echo:', repr(r2[:160]))
r3 = xact('delay 1000', wait=3.0)
print('delay:', repr(r3[:200]))
r4 = xact('nosuchcmd')
print('unknown:', repr(r4[:120]))
r5 = xact('reboot', wait=3.0)
print('reboot:', repr(r5[:260]))
ser.close()

import re as _re
m = _re.search(r'actual (\d+)us', r3)
actual = int(m.group(1)) if m else -1
prec_ok = 0 < actual < 1_100_000

help_ok = 'commands: help/echo/delay/reboot' in r1
echo_ok = 'hello world' in r2
delay_ok = 'delay 1000ms -> actual' in r3
unknown_ok = 'unknown cmd' in r4
reboot_ok = 'msh-demo v1' in r5

print()
print(f'HELP: {"PASS" if help_ok else "FAIL"}')
print(f'ECHO: {"PASS" if echo_ok else "FAIL"}')
print(f'DELAY: {"PASS" if delay_ok and prec_ok else "FAIL"} (actual={actual}us)')
print(f'UNKNOWN_CMD: {"PASS" if unknown_ok else "FAIL"}')
print(f'REBOOT: {"PASS" if reboot_ok else "FAIL"}')
print('MSH_E2E:', 'PASS' if all([help_ok, echo_ok, delay_ok, prec_ok, unknown_ok, reboot_ok]) else 'FAIL')
