import errno
import os
import pty
import select
import subprocess
import sys
import time

master, slave = pty.openpty()
process = subprocess.Popen(sys.argv[1:], stdin=slave, stdout=slave, stderr=slave)
os.close(slave)
output = bytearray()
deadline = time.monotonic() + 10
try:
    while time.monotonic() < deadline:
        if select.select([master], [], [], 0.1)[0]:
            try:
                chunk = os.read(master, 65536)
            except OSError as error:
                if error.errno == errno.EIO:
                    break
                raise
            if not chunk:
                break
            output.extend(chunk)
        if process.poll() is not None:
            break
    process.wait(timeout=1)
finally:
    if process.poll() is None:
        process.kill()
        process.wait()
    os.close(master)
sys.stdout.buffer.write(output)
sys.exit(process.returncode)
