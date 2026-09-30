import socket, time, zlib, struct, sys

IMG = sys.argv[1] if len(sys.argv) > 1 else 'target/ota-lbm.bin'
img = open(IMG, 'rb').read()
hdr = b'GDOTA001' + struct.pack('<II', len(img), zlib.crc32(img) & 0xFFFFFFFF)
pkt = hdr + img
print(f'image={IMG} {len(img)}B crc=0x{zlib.crc32(img)&0xFFFFFFFF:08X} packet={len(pkt)}B', flush=True)

class Line:
    def __init__(self):
        self.s = None
    def connect(self, tries=8):
        for i in range(tries):
            try:
                self.s = socket.create_connection(('172.22.0.50', 9000), timeout=4)
                self.s.settimeout(1.5)
                return True
            except OSError:
                time.sleep(1.5)
        return False
    def close(self):
        try: self.s.close()
        except Exception: pass
    def xact(self, cmd, wait=2.5):
        # 命令+换行单包发送（板侧单行解析）；读响应带重试
        for attempt in range(2):
            try:
                self.s.sendall((cmd + '\n').encode())
                end = time.time() + wait
                d = b''
                while time.time() < end:
                    try:
                        chunk = self.s.recv(512)
                    except socket.timeout:
                        continue
                    if not chunk:
                        break
                    d += chunk
                    if b'\n' in d:
                        return d.decode(errors='replace').strip()
                return d.decode(errors='replace').strip()
            except OSError:
                # 连接级故障：重连后重试一次
                self.close()
                if not self.connect(3):
                    return 'ERR conn'
                time.sleep(0.5)
        return 'ERR conn'

L = Line()
if not L.connect():
    print('CONN FAIL'); sys.exit(1)

r = L.xact('ping', 2.0)
if 'OK pong' not in r:
    print('PROBE FAIL:', r); sys.exit(1)
print('probe OK', flush=True)

n_sectors = (len(pkt) + 4095) // 4096
for sec in range(n_sectors):
    r = L.xact(f'flash se {sec*4096:06x}', 4.0)
    if 'OK' not in r:
        print(f'ERASE FAIL @sector{sec}:', r); sys.exit(1)
print(f'erase {n_sectors} sectors OK', flush=True)

CHUNK = 72
total = (len(pkt) + CHUNK - 1) // CHUNK
oks = 0
tail = b''
for i in range(0, len(pkt), CHUNK):
    line = f'flash wr {i:06x} ' + pkt[i:i+CHUNK].hex()
    L.s.sendall((line + '\n').encode())
    if (i // CHUNK) % 6 == 5:
        end = time.time() + 6
        got = b''
        while time.time() < end and got.count(b'OK') < 6:
            try:
                chunk = L.s.recv(512)
            except socket.timeout:
                continue
            if not chunk:
                break
            got += chunk
            end = max(end, time.time() + 0.6)
        oks += got.count(b'OK')
        if b'ERR' in got:
            print('WRITE ERR batch@', i, got[:160]); sys.exit(1)
        got = b''
# 收尾批（剩余 OK）
end = time.time() + 6
while oks < total and time.time() < end:
    try:
        chunk = L.s.recv(512)
    except socket.timeout:
        continue
    if not chunk:
        break
    tail += chunk
    oks += chunk.count(b'OK')
    end = max(end, time.time() + 0.6)
print(f'write OK {oks}/{total}', flush=True)

r = L.xact(f'flash crc 000000 {len(pkt)}', 4.0)
pcrc = f'{zlib.crc32(pkt) & 0xFFFFFFFF:08X}'
print('board crc:', r, flush=True)
crc_ok = pcrc in r
print(f'OTA_TCP_WRITE: {"PASS" if oks == total and crc_ok else "FAIL"}', flush=True)

# 6. 触发升级：软复位进 bootloader（槽位已有 lbm 镜像，CRC 校验搬运）
r = L.xact('ota boot', 3.0)
print('ota boot:', r, flush=True)
L.close()
