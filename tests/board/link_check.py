import serial, subprocess, time

ser = serial.Serial('/dev/ttyUSB0', 115200, timeout=1)
ser.reset_input_buffer()
subprocess.run(['probe-rs', 'reset', '--chip', 'GD32F470VG'], capture_output=True, timeout=30)
end = time.time() + 10
data = b''
while time.time() < end:
    chunk = ser.read(256)
    if chunk:
        data += chunk
        end = time.time() + 2.5
ser.close()
print(data.decode(errors='replace'))
