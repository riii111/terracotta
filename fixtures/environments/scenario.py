#!/usr/bin/env python3
"""Disposable three-environment plans using only the built-in terraform_data resource."""

import argparse
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile

MARKER = ".terracotta-environments-scenario"
NAMES = ("dev", "prod", "stg")


def environment():
    return {
        **{key: value for key, value in os.environ.items()
           if not key.startswith(("TF_", "TOFU_")) and key != "CI"},
        "TF_CLI_CONFIG_FILE": os.devnull,
        "CHECKPOINT_DISABLE": "1",
    }


def configuration(name, changed, ready=False):
    value = "new" if changed else "old"
    required = (
        'variable "release" { type = string }\n'
        if changed and name == "prod" and not ready else ""
    )
    release = "var.release" if required else json.dumps(value)
    extra = 'resource "terraform_data" "dev_only" { input = "new" }\n' if changed and name == "dev" else ""
    return f'''terraform {{
  backend "local" {{}}
}}
{required}resource "terraform_data" "api" {{ input = {release} }}
resource "terraform_data" "unchanged" {{ input = "baseline" }}
resource "terraform_data" "server" {{
  count = {4 if name == "prod" else 2}
  input = "{value}"
}}
{extra}'''


def run(directory, tool, *arguments):
    result = subprocess.run([tool, *arguments], cwd=directory, env=environment(),
                            stdin=subprocess.DEVNULL, capture_output=True, text=True)
    if result.returncode:
        raise RuntimeError(result.stdout + result.stderr)


def setup(tool="terraform", *, ready=False):
    directory = Path(tempfile.mkdtemp(prefix="terracotta-environments-")).resolve()
    (directory / MARKER).write_text(str(directory) + "\n")
    try:
        for name in NAMES:
            child = directory / name
            child.mkdir()
            (child / "main.tf").write_text(configuration(name, False))
            run(child, tool, "init", "-input=false", "-no-color")
            run(child, tool, "apply", "-auto-approve", "-input=false", "-no-color")
            (child / "main.tf").write_text(configuration(name, True, ready=ready))
            # Keep local state; require Terracotta to initialize each environment.
            shutil.rmtree(child / ".terraform")
    except BaseException:
        shutil.rmtree(directory)
        raise
    return directory


def checked(directory):
    if directory.is_symlink():
        raise RuntimeError("Refusing a symlink scenario")
    directory = directory.resolve()
    marker = directory / MARKER
    if (not directory.name.startswith("terracotta-environments-")
            or not marker.is_file() or marker.is_symlink()
            or marker.read_text() != str(directory) + "\n"):
        raise RuntimeError("Not a scenario created by this script")
    return directory


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="command", required=True)
    create = commands.add_parser("setup")
    create.add_argument("--tool", choices=("terraform", "tofu"), default="terraform")
    demo = commands.add_parser("demo", help="Build and open a ready three-environment review")
    demo.add_argument("--tool", choices=("terraform", "tofu"), default="terraform")
    for name in ("repair", "clean"):
        commands.add_parser(name).add_argument("directory", type=Path)
    args = parser.parse_args()
    if args.command == "setup":
        print(setup(args.tool))
    elif args.command == "demo":
        sys.exit(run_demo(args.tool))
    elif args.command == "repair":
        (checked(args.directory) / "prod/retry.auto.tfvars").write_text('release = "new"\n')
    else:
        shutil.rmtree(checked(args.directory))


def run_demo(tool):
    if not sys.stdin.isatty() or not sys.stdout.isatty():
        raise RuntimeError("The demo requires an interactive terminal")

    repository = Path(__file__).resolve().parents[2]
    build_environment = environment()
    target = Path(build_environment.get("CARGO_TARGET_DIR", "target/environments-demo"))
    if not target.is_absolute():
        target = repository / target
    target = target.resolve()
    build_environment["CARGO_TARGET_DIR"] = str(target)
    print("Starting three-environment demo. Ctrl-C to cancel.", file=sys.stderr, flush=True)
    print("[1/3] Preparing ready local environments...", file=sys.stderr, flush=True)
    directory = setup(tool, ready=True)
    try:
        build_environment["RUSTC_WRAPPER"] = ""
        print(f"[2/3] Building Terracotta (cache: {target})...", file=sys.stderr, flush=True)
        build = subprocess.run(
            ["cargo", "build", "--locked"],
            cwd=repository,
            env=build_environment,
            stdin=subprocess.DEVNULL,
        )
        if build.returncode:
            return build.returncode

        executable = "terracotta.exe" if os.name == "nt" else "terracotta"
        print("[3/3] Opening the three-environment review...", file=sys.stderr, flush=True)
        return subprocess.run(
            [str(target / "debug" / executable), tool, f"-chdir={directory}", "plan"],
            cwd=directory,
            env=environment(),
        ).returncode
    finally:
        shutil.rmtree(checked(directory))


if __name__ == "__main__":
    main()
