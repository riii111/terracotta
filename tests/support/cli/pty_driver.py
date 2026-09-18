
import os
import pty
import select
import signal
import fcntl
import struct
import sys
import termios
import time

_, binary, root, columns_arg, rows_arg, scenario = sys.argv[:6]
columns = int(columns_arg)
rows = int(rows_arg)
command = sys.argv[6:]
pid, fd = pty.fork()
if pid == 0:
    os.environ["TERM"] = "xterm-256color"
    os.execv(
        "/bin/sh",
        [
            "sh",
            "-c",
            'stty rows "$1" cols "$2"; cd "$3"; shift 3; exec "$@"',
            "sh",
            rows_arg,
            columns_arg,
            root,
            binary,
            *command,
        ],
    )

output = bytearray()
observed = []


class Screen:
    def __init__(self, columns, rows):
        self.columns = columns
        self.rows = rows
        self.cells = [[" "] * columns for _ in range(rows)]
        self.row = 0
        self.column = 0
        self.saved = (0, 0)
        self.pending = bytearray()

    def resize(self, columns, rows):
        cells = [[" "] * columns for _ in range(rows)]
        for row in range(min(self.rows, rows)):
            for column in range(min(self.columns, columns)):
                cells[row][column] = self.cells[row][column]
        self.columns = columns
        self.rows = rows
        self.cells = cells
        self.row = min(self.row, rows - 1)
        self.column = min(self.column, columns - 1)

    def feed(self, data):
        self.pending.extend(data)
        while self.pending:
            if self.pending[0] == 0x1b:
                if len(self.pending) < 2:
                    return
                if self.pending[1] == ord("["):
                    final = next(
                        (index for index, value in enumerate(self.pending[2:], 2)
                         if 0x40 <= value <= 0x7e),
                        None,
                    )
                    if final is None:
                        return
                    sequence = bytes(self.pending[2:final])
                    command = chr(self.pending[final])
                    del self.pending[:final + 1]
                    self.csi(sequence.decode("ascii", "ignore"), command)
                    continue
                del self.pending[:2]
                continue

            value = self.pending[0]
            if value == 0x0d:
                del self.pending[:1]
                self.column = 0
                continue
            if value == 0x0a:
                del self.pending[:1]
                self.row = min(self.row + 1, self.rows - 1)
                continue
            if value == 0x08:
                del self.pending[:1]
                self.column = max(self.column - 1, 0)
                continue
            if value < 0x20 or value == 0x7f:
                del self.pending[:1]
                continue

            character = None
            for length in range(1, min(4, len(self.pending)) + 1):
                try:
                    character = bytes(self.pending[:length]).decode("utf-8")
                    break
                except UnicodeDecodeError as error:
                    if error.reason == "unexpected end of data" and length == len(self.pending):
                        return
            if character is None:
                character = "�"
                length = 1
            del self.pending[:length]
            self.put(character)

    def put(self, character):
        if self.row >= self.rows:
            return
        if self.column >= self.columns:
            self.column = 0
            self.row = min(self.row + 1, self.rows - 1)
        self.cells[self.row][self.column] = character
        self.column = min(self.column + 1, self.columns)

    def csi(self, parameters, command):
        private = parameters.startswith("?")
        if private:
            parameters = parameters[1:]
        values = []
        for value in parameters.split(";") if parameters else []:
            try:
                values.append(int(value) if value else 1)
            except ValueError:
                values.append(1)

        if command in ("H", "f"):
            self.row = max((values[0] if values else 1) - 1, 0)
            self.column = max((values[1] if len(values) > 1 else 1) - 1, 0)
        elif command == "G":
            self.column = max((values[0] if values else 1) - 1, 0)
        elif command == "d":
            self.row = max((values[0] if values else 1) - 1, 0)
        elif command == "A":
            self.row = max(self.row - (values[0] if values else 1), 0)
        elif command == "B":
            self.row = min(self.row + (values[0] if values else 1), self.rows - 1)
        elif command == "C":
            self.column = min(self.column + (values[0] if values else 1), self.columns)
        elif command == "D":
            self.column = max(self.column - (values[0] if values else 1), 0)
        elif command == "J":
            if not values or values[0] == 2:
                self.cells = [[" "] * self.columns for _ in range(self.rows)]
        elif command == "K":
            mode = values[0] if values else 0
            start = 0 if mode == 2 else self.column if mode == 0 else 0
            end = self.columns if mode in (0, 2) else self.column + 1
            for column in range(start, min(end, self.columns)):
                self.cells[self.row][column] = " "
        elif command == "s":
            self.saved = (self.row, self.column)
        elif command == "u":
            self.row, self.column = self.saved

    def text(self):
        return "\n".join("".join(row) for row in self.cells)


screen = Screen(columns, rows)


def child_status():
    waited, status = os.waitpid(pid, os.WNOHANG)
    if waited == 0:
        return None
    return os.waitstatus_to_exitcode(status)


def read_available():
    ready, _, _ = select.select([fd], [], [], 0.1)
    if not ready:
        return
    try:
        chunk = os.read(fd, 8192)
        output.extend(chunk)
        screen.feed(chunk)
    except OSError:
        pass


def wait_screen(predicate, name, description, timeout=20):
    before = screen.text()
    deadline = time.time() + timeout
    while time.time() < deadline:
        current = screen.text()
        if current != before and predicate(current):
            observed.append(name)
            return
        read_available()
        current = screen.text()
        if current != before and predicate(current):
            observed.append(name)
            return
        if child_status() is not None:
            break
    raise RuntimeError(
        f"missing {name}: {description!r}; head={bytes(output)[:4000]!r}; tail={bytes(output)[-1200:]!r}"
    )


def wait_new(marker, name, timeout=20):
    wait_screen(lambda current: marker in current, name, marker, timeout)


def wait_parts(markers, name, timeout=20):
    wait_screen(lambda current: all(marker in current for marker in markers), name, markers, timeout)


def wait_any(markers, name, timeout=20):
    wait_screen(lambda current: any(marker in current for marker in markers), name, markers, timeout)


def wait_file(path, name, timeout=20):
    deadline = time.time() + timeout
    while time.time() < deadline:
        if os.path.isfile(path) and os.path.getsize(path) > 0:
            observed.append(name)
            return
        read_available()
    raise RuntimeError(f"missing {name}: {path!r}; head={bytes(output)[:4000]!r}")


def wait_exit(timeout=20):
    deadline = time.time() + timeout
    while time.time() < deadline:
        status = child_status()
        if status is not None:
            return drain_after_exit(status)
        read_available()
    raise RuntimeError(
        f"child did not exit; head={bytes(output)[:4000]!r}; tail={bytes(output)[-1200:]!r}"
    )


def send_key(key):
    os.write(fd, key)
    time.sleep(0.1)


def drain_after_exit(status):
    while True:
        ready, _, _ = select.select([fd], [], [], 0.2)
        if not ready:
            return status
        try:
            chunk = os.read(fd, 8192)
        except OSError:
            return status
        if not chunk:
            return status
        output.extend(chunk)
        screen.feed(chunk)


def resize(columns, rows):
    fcntl.ioctl(fd, termios.TIOCSWINSZ, struct.pack("HHHH", rows, columns, 0, 0))
    screen.resize(columns, rows)
    try:
        os.killpg(pid, signal.SIGWINCH)
    except OSError:
        os.kill(pid, signal.SIGWINCH)


def kill_child():
    try:
        os.killpg(pid, signal.SIGKILL)
    except OSError:
        try:
            os.kill(pid, signal.SIGKILL)
        except OSError:
            pass
    try:
        os.waitpid(pid, 0)
    except ChildProcessError:
        pass


try:
    if scenario == "workflow":
        wait_new("Needs review: 0 / 1", "list")
        send_key(b"f")
        wait_parts(["Showing", "0/1"], "filter_empty")
        send_key(b"f")
        wait_new("Filter: All", "filter_all")
        send_key(b"/")
        send_key(b"/")
        wait_new("Type to search", "search_input")
        send_key(b"z")
        wait_new("No matching resources", "search_empty")
        send_key(b"\x7f")
        send_key(b"\x7f")
        send_key(b"\r")
        wait_new("terraform_data.api", "search_cleared")
        send_key(b"\r")
        wait_new("Resource 1/1", "detail")
        send_key(b"j")
        send_key(b"\r")
        wait_new("collaps", "expanded")
        send_key(b"y")
        wait_any(["Copied", "Copy failed"], "copy")
        send_key(b"\x1b")
        wait_new("Filter:", "back")
        send_key(b"q")
        exit_code = wait_exit()
    elif scenario == "diagnostic_success":
        wait_new("Diagnostics: 1 (w)", "diagnostic_notice")
        send_key(b"w")
        wait_parts(["Diagnostics", "Warning 1/1", "synthetic plan warning"], "diagnostics")
        send_key(b"\x1b")
        wait_new("Diagnostics: 1 (w)", "diagnostics_back")
        send_key(b"q")
        exit_code = wait_exit()
    elif scenario == "failure":
        wait_new("Failed", "failed")
        send_key(b"y")
        wait_any(["Copied diagnostic", "Copy failed: clipboard unavailable."], "failure_copy")
        send_key(b"q")
        exit_code = wait_exit()
    elif scenario == "git_failure":
        wait_new("incomplete", "git_failure")
        send_key(b"q")
        exit_code = wait_exit()
    elif scenario == "interrupt":
        wait_file(os.environ["TERRACOTTA_FAKE_PID_PATH"], "terraform_started")
        send_key(b"\x03")
        wait_new("Cancelling...", "cancelling")
        exit_code = wait_exit()
    elif scenario == "git_interrupt":
        wait_file(os.environ["TERRACOTTA_FAKE_GIT_PID_PATH"], "git_started")
        send_key(b"\x03")
        wait_new("Cancelling...", "cancelling")
        exit_code = wait_exit()
    elif scenario == "config_interrupt":
        wait_file(os.environ["TERRACOTTA_FAKE_GIT_PID_PATH"], "git_started")
        send_key(b"\x03")
        exit_code = wait_exit()
    elif scenario == "narrow":
        wait_new("Terminal too small", "narrow")
        resize(100, 24)
        wait_new("Needs review: 0 / 1", "resized")
        send_key(b"q")
        exit_code = wait_exit()
    elif scenario == "empty":
        wait_parts(["Needs review: 0 / 0", "No res"], "empty")
        send_key(b"y")
        send_key(b"\r")
        send_key(b"q")
        exit_code = wait_exit()
    elif scenario == "compare_ref":
        wait_parts(["merge-base(main)", "Needs review: 1 / 1"], "compare_ref")
        send_key(b"q")
        exit_code = wait_exit()
    elif scenario == "panic":
        exit_code = wait_exit()
    else:
        raise RuntimeError(f"unknown scenario: {scenario}")
    print(f"exit={exit_code}")
    print(f"restored={str(b'\x1b[?1049l' in output).lower()}")
    print(f"cursor_restored={str(b'\x1b[?25h' in output).lower()}")
    print("observed=" + ",".join(observed))
except BaseException as error:
    kill_child()
    print(f"driver_error={error!r}")
    print(bytes(output)[-1200:].decode("utf-8", "replace"))
    raise
