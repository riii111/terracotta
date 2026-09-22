#!/usr/bin/env python3

import argparse
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile


FIXTURES = Path(__file__).resolve().parent
MARKER = ".terracotta-basic-scenario"
EXPECTED_ACTIONS = {
    "terraform_data.api": ["update"],
    "terraform_data.worker": ["delete", "create"],
    "terraform_data.old": ["delete"],
    "terraform_data.new": ["create"],
    "terraform_data.pending": ["update"],
}


def main():
    parser = argparse.ArgumentParser(description="Create an isolated local Terraform scenario.")
    commands = parser.add_subparsers(dest="command", required=True)
    setup_parser = commands.add_parser(
        "setup", help="Create a fresh temporary Git repository and plan"
    )
    setup_parser.add_argument("--tool", choices=("terraform", "tofu"), default="terraform")
    commands.add_parser("demo", help="Build with a checkout-local cache and open a temporary scenario")
    cleanup = commands.add_parser("clean", help="Remove a scenario created by this script")
    cleanup.add_argument("directory", type=Path)
    args = parser.parse_args()

    if args.command == "setup":
        print(setup(args.tool))
    elif args.command == "demo":
        sys.exit(demo())
    else:
        clean(args.directory)


def setup(tool="terraform"):
    for executable in ("git", tool):
        if shutil.which(executable) is None:
            raise RuntimeError(f"Required executable not found: {executable}")

    directory = Path(tempfile.mkdtemp(prefix="terracotta-basic-")).resolve()
    (directory / MARKER).write_text(str(directory) + "\n")
    environment = isolated_environment(directory)
    try:
        for source in (FIXTURES / "baseline").glob("*.tf"):
            shutil.copyfile(source, directory / source.name)
        (directory / ".gitignore").write_text(
            ".terraform/\n*.tfstate\n*.tfstate.*\n*.tfplan\n*.tfplan.json\n"
            ".terraform.lock.hcl\n" + MARKER + "\n"
        )
        run(directory, environment, "git", "init", "--initial-branch=main")
        run(directory, environment, "git", "add", ".")
        commit(directory, environment, "test: establish applied baseline")
        run(directory, environment, tool, "init", "-input=false", "-no-color")
        run(directory, environment, tool, "apply", "-auto-approve", "-input=false", "-no-color")

        shutil.copyfile(FIXTURES / "changes" / "pending.tf", directory / "pending.tf")
        run(directory, environment, "git", "add", "pending.tf")
        commit(directory, environment, "test: leave committed change unapplied")
        shutil.copyfile(FIXTURES / "changes" / "main.tf", directory / "main.tf")

        run(directory, environment, tool, "plan", "-input=false", "-no-color", "-out=review.tfplan")
        plan_json = run(directory, environment, tool, "show", "-json", "review.tfplan")
        plan = json.loads(plan_json)
        actual = {
            change["address"]: change["change"]["actions"]
            for change in plan["resource_changes"]
            if change["change"]["actions"] != ["no-op"]
        }
        if actual != EXPECTED_ACTIONS:
            raise RuntimeError(f"Unexpected plan actions: {actual}")
        changed = run(directory, environment, "git", "diff", "--name-only", "HEAD").splitlines()
        if changed != ["main.tf"]:
            raise RuntimeError(f"Unexpected Git changes: {changed}")
        (directory / "review.tfplan.json").write_text(plan_json)
    except BaseException:
        shutil.rmtree(directory)
        raise

    print("Verified: create 1 / update 2 / replace 1 / delete 1", file=sys.stderr)
    return directory


def demo():
    if not sys.stdin.isatty() or not sys.stdout.isatty():
        raise RuntimeError("The demo requires an interactive terminal")
    print("Starting Terracotta demo. No additional Enter is needed. Ctrl-C to cancel.",
          file=sys.stderr, flush=True)
    repository = FIXTURES.parent.parent
    target = repository / "target" / "demo"
    print("[1/3] Preparing local Terraform scenario...", file=sys.stderr, flush=True)
    directory = setup()
    try:
        environment = isolated_environment(directory)
        environment["RUSTC_WRAPPER"] = ""
        print(f"[2/3] Building Terracotta (cache: {target})...", file=sys.stderr, flush=True)
        build = subprocess.run(
            ["cargo", "build", "--locked", "--target-dir", str(target)],
            cwd=repository, env=environment, stdin=subprocess.DEVNULL,
        )
        if build.returncode:
            return build.returncode
        executable = "terracotta.exe" if os.name == "nt" else "terracotta"
        install_demo_terraform_wrapper(directory, environment)
        interactive_environment = environment.copy()
        interactive_environment.pop("TF_IN_AUTOMATION", None)
        print("[3/3] Opening plan review...", file=sys.stderr, flush=True)
        return subprocess.run(
            [str(target / "debug" / executable), "plan"],
            cwd=directory, env=interactive_environment,
        ).returncode
    finally:
        clean(directory)


def install_demo_terraform_wrapper(directory, environment):
    # Rust's Windows command lookup does not reliably select a .cmd shim.
    if os.name == "nt":
        return

    terraform = shutil.which("terraform")
    if terraform is None:
        raise RuntimeError("Required executable not found: terraform")
    terraform = str(Path(terraform).resolve())

    wrapper_directory = directory / ".terraform" / "terracotta-demo-bin"
    wrapper_directory.mkdir(parents=True)
    wrapper = wrapper_directory / "terraform"
    wrapper.write_text(
        "#!/usr/bin/env python3\n"
        "import os\nimport sys\nimport time\n"
        "if len(sys.argv) > 1 and sys.argv[1] == 'apply':\n"
        "    time.sleep(5)\n"
        f"os.execv({terraform!r}, [{terraform!r}, *sys.argv[1:]])\n"
    )
    wrapper.chmod(0o700)
    environment["PATH"] = str(wrapper_directory) + os.pathsep + environment.get("PATH", os.defpath)


def isolated_environment(directory):
    # Ambient Git/Terraform options must not redirect this disposable scenario.
    environment = {
        key: value for key, value in os.environ.items()
        if not key.startswith(("GIT_", "TF_"))
    }
    environment.update({
        "GIT_CONFIG_NOSYSTEM": "1",
        "GIT_CONFIG_GLOBAL": os.devnull,
        "GIT_TERMINAL_PROMPT": "0",
        "TF_IN_AUTOMATION": "1",
        "TF_DATA_DIR": str(directory / ".terraform"),
        "TF_CLI_CONFIG_FILE": os.devnull,
        "CHECKPOINT_DISABLE": "1",
    })
    return environment


def run(directory, environment, *command):
    print("Running: " + " ".join(command), file=sys.stderr, flush=True)
    result = subprocess.run(
        command, cwd=directory, env=environment, text=True,
        stdin=subprocess.DEVNULL, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
    )
    if result.returncode:
        raise RuntimeError(result.stdout + result.stderr)
    return result.stdout


def commit(directory, environment, message):
    run(directory, environment, "git",
        "-c", "user.name=Terracotta Fixture",
        "-c", "user.email=fixture@example.invalid",
        "-c", "commit.gpgsign=false",
        "-c", f"core.hooksPath={os.devnull}",
        "commit", "-m", message)


def clean(directory):
    if directory.is_symlink():
        raise RuntimeError("Refusing to remove a symlink")
    directory = directory.resolve()
    marker = directory / MARKER
    if (not directory.name.startswith("terracotta-basic-")
            or not marker.is_file() or marker.is_symlink()
            or marker.read_text() != str(directory) + "\n"):
        raise RuntimeError("Not a scenario directory created by this script")
    shutil.rmtree(directory)
    print(f"Removed: {directory}")


if __name__ == "__main__":
    try:
        main()
    except (OSError, RuntimeError, ValueError) as error:
        sys.exit(str(error))
