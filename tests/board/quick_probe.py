import socket, time
for i in range(3):
    try:
        s = socket.create_connection(('172.22.0.50', 9000), timeout=3)
        s.settimeout(3)
        s.sendall(b'ping\n')
        time.sleep(0.5)
        d = s.recv(64)
        s.close()
        print(f'try{i}: {d}', flush=True)
        break
    except OSError as e:
        print(f'try{i}: {type(e).__name__}', flush=True)
        time.sleep(2)
