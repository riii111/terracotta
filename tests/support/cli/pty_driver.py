
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
        f"missing {name}: {description!r}; screen={screen.text()!r}; head={bytes(output)[:4000]!r}; tail={bytes(output)[-1200:]!r}"
    )


def wait_new(marker, name, timeout=20):
    wait_screen(lambda current: marker in current, name, marker, timeout)


def observe_current_or_wait(marker, name, timeout=20):
    if marker in screen.text():
        observed.append(name)
        return
    wait_new(marker, name, timeout)


def wait_parts(markers, name, timeout=20):
    wait_screen(lambda current: all(marker in current for marker in markers), name, markers, timeout)


def wait_file(path, name, timeout=20):
    deadline = time.time() + timeout
    while time.time() < deadline:
        if os.path.isfile(path) and os.path.getsize(path) > 0:
            observed.append(name)
            return
        read_available()
    raise RuntimeError(f"missing {name}: {path!r}; head={bytes(output)[:4000]!r}")


def plan_status_is_above_plan_text(current):
    markers = ("Unique targets (replace once):", "terraform_data.api")
    positions = [current.find(marker) for marker in markers]
    if any(position < 0 for position in positions):
        return False
    return positions[0] < positions[1]


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


def assert_screen_unchanged(name, timeout=1):
    before = screen.text()
    deadline = time.time() + timeout
    while time.time() < deadline:
        read_available()
        if screen.text() != before:
            raise RuntimeError(f"screen changed while {name}")
        if child_status() is not None:
            raise RuntimeError(f"child exited while {name}")
        time.sleep(0.05)
    observed.append(name)


def send_text(text):
    for character in text:
        send_key(character.encode())


def quit_with_enter():
    send_key(b"q")
    wait_new("Quit Terracotta?", "quit_confirmation")
    send_key(b"\r")
    return wait_exit()


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
    if scenario in ("full_text", "user_output", "cli_args", "detailed"):
        wait_parts(["Plan:", "terraform_data.api"], "plan_text", timeout=30)
        observe_current_or_wait("3/", "plan_position")
        exit_code = quit_with_enter()
    elif scenario.startswith("env_"):
        if scenario == "env_child_interrupt":
            exit_code = wait_exit()
        elif scenario in ("env_partial", "env_cancel"):
            wait_parts(["Ready: 1/2", "Running", "z-slow"], "partial_results")
            send_key(b"\r")
            wait_new("terraform_data.api", "ready_review_while_running")
            send_key(b"a")
            send_key(b"\x1b")
            wait_new("0 Overview", "back_to_environments")
            if scenario == "env_cancel":
                wait_file(os.environ["TERRACOTTA_FAKE_PID_PATH"], "active_process")
                send_key(b"q")
                wait_new("Stop acquiring", "cancel_confirmation")
                send_key(b"\x1b")
                wait_new("0 Overview", "continue_acquisition")
                send_key(b"q")
                wait_new("Stop acquiring", "cancel_again")
                send_key(b"\r")
                exit_code = wait_exit()
            else:
                open(os.path.join(root, "z-slow/release-plan"), "w").close()
                wait_new("Ready: 2/2", "all_ready")
                send_key(b"q")
                exit_code = wait_exit()
        elif scenario == "env_example":
            wait_parts(["Ready: 2/3", "Error", "~ 20"], "example_comparison")
            send_key(b"]")
            send_key(b"r")
            wait_parts(["Ready: 3/3", "~ 200"], "example_retry")
            send_key(b"q")
            exit_code = wait_exit()
        elif scenario == "env_real":
            wait_parts(["Ready: 1/3", "Running", "Compared: dev"], "real_partial_results")
            send_key(b"v")
            wait_new("terraform_data.api", "real_review_while_running")
            send_key(b"\x1b")
            wait_new("Total", "real_back_to_matrix")
            open(os.environ["TERRACOTTA_REAL_PLAN_GATE"], "w").close()
            wait_parts(["Ready: 2/3", "Error", "Compared: dev, stg"], "real_comparison_with_error")
            send_key(b"]")
            send_key(b"\r")
            wait_new("required variable", "real_error_diagnostic")
            send_key(b"\x1b")
            wait_new("Total", "real_error_dialog_closed")
            with open(os.path.join(root, "prod/retry.auto.tfvars"), "w") as repair:
                repair.write('release = "new"\n')
            send_key(b"r")
            wait_parts(["Ready: 3/3", "prod · terraform"], "real_selected_retry_success")
            send_key(b"v")
            wait_parts(["terraform_data.api", "prod"], "real_retried_plan_review")
            send_key(b"0")
            wait_new("Total", "real_complete_comparison")
            send_key(b"q")
            exit_code = wait_exit()
        elif scenario == "env_default_matrix":
            wait_parts(["Ready: 3/3", "terraform_data.server[*]"], "default_matrix")
            observed.append("default_matrix")
            send_key(b"q")
            exit_code = wait_exit()
        elif scenario == "env_matrix":
            wait_parts(["Ready: 3/3", "~ 200"], "matrix_ready")
            send_key(b"/")
            wait_new("Filter:", "matrix_filter")
            send_text("[198]")
            send_key(b"\r")
            send_key(b" ")
            send_key(b"j")
            send_key(b"]")
            send_key(b"\r")
            wait_parts(["server[198]", "Esc overview"], "matrix_raw_member")
            send_key(b"3")
            wait_new("c-prod", "matrix_digit_environment")
            send_key(b"\x1b")
            wait_parts(["Filter: /[198]", "Total", "~200"], "restored_matrix_selection")
            send_key(b"q")
            exit_code = wait_exit()
        elif scenario == "env_many":
            wait_new("Ready: 12/12", "many_ready")
            for _ in range(11):
                send_key(b"]")
            wait_new("env-11 · terraform", "twelfth_column")
            send_key(b"\r")
            wait_parts(["terraform_data.api", "env-11"], "twelfth_environment")
            send_key(b"[")
            wait_new("env-10", "eleventh_environment")
            send_key(b"0")
            wait_new("env-11 · terraform", "restored_last_column")
            send_key(b"q")
            exit_code = wait_exit()
        elif scenario == "env_show_failure":
            wait_parts(["Ready: 1/2", "Error"], "failed_environment")
            send_key(b"]")
            wait_parts(["show output could not be parsed", "synthetic plan warning"], "warning_and_failure")
            send_key(b"q")
            exit_code = wait_exit()
        elif scenario == "env_retry":
            wait_parts(["Ready: 1/2", "Error"], "failed_environment")
            send_key(b"]")
            wait_new("Missing required variable", "error_diagnostic")
            send_key(b"r")
            wait_new("Ready: 2/2", "retry_success")
            send_key(b"q")
            exit_code = wait_exit()
        else:
            expected = "Ready: 1/2" if scenario in ("env_init_failure", "env_excluded", "env_reinit_failure") else "Ready: 2/2"
            observe_current_or_wait(expected, "final_environment_results")
            if scenario in ("env_init_failure", "env_reinit_failure"):
                send_key(b"]")
                observe_current_or_wait("Error", "failed_environment")
            if scenario == "env_detailed":
                send_key(b"\r")
                wait_new("chosen-production", "selected_workspace")
            send_key(b"q")
            exit_code = wait_exit()
    elif scenario == "filter_navigation":
        wait_parts(["Plan:", "terraform_data.api"], "plan_text", timeout=30)
        observe_current_or_wait("3/", "plan_position")
        send_key(b"/")
        wait_new("/ ", "filter_input")
        send_text("api")
        wait_new(" matches", "filter_matches")
        send_key(b"\r")
        wait_new("y copy all", "filter_confirmed")
        send_key(b"?")
        wait_new("Help", "filter_help")
        send_key(b"?")
        wait_new("y copy all", "filter_help_closed")
        send_key(b"c")
        wait_new("Execution directory", "filter_context")
        send_key(b"\x1b")
        wait_new("y copy all", "filter_context_closed")
        send_key(b"y")
        wait_screen(
            lambda current: "Copied." in current or "Copy failed." in current,
            "filter_copy",
            "copy notice after filtering",
        )
        send_key(b"a")
        assert_screen_unchanged("plan_entry_apply_blocked")
        send_key(b"q")
        wait_new("Quit Terracotta?", "filter_quit_confirmation")
        send_key(b"\x1b")
        wait_new("y copy all", "filter_quit_cancelled")
        send_key(b"n")
        send_key(b"N")
        send_key(b"\x1b")
        wait_screen(
            lambda current: "Plan | Filter" not in current
            and "terraform_data.api" in current,
            "filter_cleared",
            "full review after Escape",
        )
        exit_code = quit_with_enter()
    elif scenario == "overview_navigation":
        wait_parts(["Plan:", "terraform_data.api"], "plan_text", timeout=30)
        send_key(b"s")
        wait_parts(["Overview", "terraform_data.server[*]", "Repeated: 2"], "overview_opened")
        send_key(b"j")
        send_key(b" ")
        wait_new('terraform_data.server["one"]', "overview_expanded")
        send_key(b"\r")
        wait_new("terraform_data.server[\"one\"]", "overview_raw_block")
        send_key(b"/")
        wait_new("/ ", "overview_raw_filter_input")
        send_text("api")
        wait_new(" matches", "overview_raw_filter_matches")
        send_key(b"\x1b")
        wait_new("/ filter", "overview_raw_filter_cancelled")
        send_key(b"/")
        wait_new("/ ", "overview_raw_filter_input_again")
        send_text("api")
        wait_new(" matches", "overview_raw_filter_matches_again")
        send_key(b"\r")
        wait_new("y copy all", "overview_raw_filter_confirmed")
        send_key(b"\x1b")
        wait_new("Overview", "overview_restored")
        send_key(b"/")
        wait_new("Filter: /", "overview_filter_input")
        send_text("two")
        send_key(b"\r")
        wait_new("display only", "overview_filtered")
        send_key(b"v")
        wait_parts(["Plan:", "terraform_data.api"], "overview_full_plan")
        exit_code = quit_with_enter()
    elif scenario == "default_overview":
        wait_parts(["Overview", "terraform_data.server[*]"], "default_overview")
        send_key(b"q")
        send_key(b"\r")
        exit_code = wait_exit()
    elif scenario == "default_ci":
        wait_new("Usage: terracotta", "default_help")
        exit_code = wait_exit()
    elif scenario == "unsupported_default":
        observed.append("unsupported_default")
        exit_code = wait_exit()
    elif scenario == "demo":
        wait_new("Opening the single-environment Overview...", "demo_tui", timeout=120)
        wait_parts(["Change Address", "terraform_data.api"], "demo_overview")
        send_key(b"q")
        send_key(b"\r")
        exit_code = wait_exit()
    elif scenario == "basic_workflow":
        wait_parts(["Plan:", "terraform_data.api"], "plan_text", timeout=30)
        wait_new("a apply", "plan_ready")
        send_key(b"a")
        wait_new("Apply this reviewed plan?", "apply_confirmation")
        wait_new("Type ", "apply_target_prompt")
        send_text(os.path.basename(os.path.normpath(root)))
        send_key(b"\r")
        wait_parts(["Apply complete", "terraform_data.api"], "apply_result", timeout=60)
        send_key(b"y")
        exit_code = quit_with_enter()
    elif scenario == "no_changes":
        observed.append("no_changes")
        exit_code = wait_exit()
    elif scenario in (
        "apply_success",
        "apply_failure",
        "apply_interrupt",
        "apply_log_view",
        "apply_mapping",
    ):
        wait_parts(["Plan:", "terraform_data.api"], "plan_text", timeout=30)
        if scenario == "apply_success":
            send_key(b"/")
            wait_new("/ ", "apply_filter_input")
            send_text("not-present")
            observe_current_or_wait("No matches", "apply_filter_no_matches")
            send_key(b"\r")
            wait_new("y copy all", "apply_filter_confirmed")
        send_key(b"?")
        wait_new("Help", "plan_help")
        send_key(b"?")
        wait_new("a apply", "plan_help_closed")
        send_key(b"a")
        wait_new("Apply this reviewed plan?", "apply_confirmation")
        send_key(b"?")
        wait_new("Apply help", "apply_help")
        send_key(b"?")
        wait_new("Apply this reviewed plan?", "apply_help_closed")
        send_key(b"\t")
        wait_new("Execution directory", "apply_context")
        send_key(b"\x1b")
        wait_new("Apply this reviewed plan?", "apply_context_closed")
        send_text("yes")
        send_key(b"\r")
        wait_new("Applying...", "apply_started")
        if scenario == "apply_log_view":
            send_key(b"\t")
            wait_new("scroll log", "apply_logs_focused")
            send_key(b"\t")
            wait_new("select", "apply_targets_focused")
            send_key(b"\t")
            wait_new("scroll log", "apply_logs_refocused")
            wait_parts(
                ["Apply complete", "terraform_data.api", "y yank result"],
                "apply_success",
                timeout=60,
            )
            exit_code = quit_with_enter()
        elif scenario in ("apply_success", "apply_mapping"):
            wait_parts(["Apply complete", "terraform_data.api"], "apply_success")
            exit_code = quit_with_enter()
        elif scenario == "apply_failure":
            wait_parts(["Apply failed", "synthetic apply failure"], "apply_failure")
            exit_code = quit_with_enter()
        else:
            send_key(b"v")
            wait_new("Applying saved plan...", "apply_logs_open")
            send_key(b"\x03")
            wait_parts(["Stopping apply", "Apply interrupted"], "apply_interrupted")
            exit_code = quit_with_enter()
    elif scenario == "apply_resize":
        wait_parts(["Plan:", "terraform_data.api"], "plan_text", timeout=30)
        send_key(b"a")
        wait_new("Apply this reviewed plan?", "apply_confirmation")
        send_key(b"y")
        wait_parts(["Type yes to apply (exact match).", "y"], "apply_input_y")
        send_key(b"e")
        wait_parts(["Type yes to apply (exact match).", "ye"], "apply_input_ye")
        send_key(b"s")
        wait_parts(["Type yes to apply (exact match).", "yes"], "apply_input_yes")
        resize(24, 6)
        wait_new("Terminal too small", "apply_confirmation_narrow")
        send_key(b"\r")
        time.sleep(0.2)
        read_available()
        if "Applying..." in screen.text():
            raise RuntimeError("apply started while confirmation was not renderable")
        resize(100, 24)
        wait_parts(
            ["Type yes to apply (exact match).", "yes"],
            "apply_confirmation_resized",
        )
        send_key(b"\r")
        wait_new("Applying...", "apply_started")
        send_key(b"\r")
        observed.append("apply_second_enter")
        wait_new("Apply complete", "apply_result", timeout=60)
        exit_code = quit_with_enter()
    elif scenario in ("apply_no", "apply_escape"):
        wait_parts(["Plan:", "terraform_data.api"], "plan_text", timeout=30)
        send_key(b"a")
        wait_new("Apply this reviewed plan?", "apply_confirmation")
        send_text("no") if scenario == "apply_no" else send_key(b"\x1b")
        if scenario == "apply_no":
            send_key(b"\r")
            wait_new("Type yes to apply (exact match).", "apply_invalid_input")
            send_key(b"\x1b")
        wait_screen(
            lambda current: (
                "Type yes to apply (exact match)." not in current
                and "Apply this reviewed plan?" not in current
                and "terraform_data.api" in current
            ),
            "plan_restored",
            "review screen after cancelling apply",
        )
        exit_code = quit_with_enter()
    elif scenario == "diagnostic_success":
        wait_screen(
            plan_status_is_above_plan_text,
            "plan_status_and_text",
            (
                "Unique targets (replace once):",
                "terraform_data.api",
            ),
        )
        exit_code = quit_with_enter()
    elif scenario == "quit_confirmation":
        wait_parts(["Plan:", "terraform_data.api"], "plan_text", timeout=30)
        send_key(b"q")
        wait_new("Quit Terracotta?", "quit_confirmation")
        send_key(b"q")
        assert_screen_unchanged("quit_repeat")
        send_key(b"\x1b")
        wait_new("q quit", "quit_cancelled")
        send_key(b"\x03")
        wait_new("Quit Terracotta?", "quit_ctrl_c")
        send_key(b"\x1b")
        wait_new("q quit", "quit_ctrl_c_cancelled")
        send_key(b"q")
        wait_new("Quit Terracotta?", "quit_confirmation_again")
        send_key(b"y")
        wait_screen(
            lambda current: "Copied." in current or "Copy failed." in current,
            "quit_copy",
            "copy notice after cancelling quit confirmation",
        )
        send_key(b"q")
        wait_new("Quit Terracotta?", "quit_confirmation_after_copy")
        send_key(b"\r")
        exit_code = wait_exit()
    elif scenario == "failure":
        observed.append("failed")
        exit_code = wait_exit()
    elif scenario == "interrupt":
        wait_file(os.environ["TERRACOTTA_FAKE_PID_PATH"], "terraform_started")
        send_key(b"\x03")
        observed.append("interrupt_requested")
        exit_code = wait_exit()
    elif scenario == "narrow":
        wait_new("Terminal too small", "narrow")
        resize(100, 24)
        wait_new("Plan:", "resized")
        exit_code = quit_with_enter()
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
