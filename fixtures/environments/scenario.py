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


def configuration(name, changed):
    value = "new" if changed else "old"
    required = 'variable "release" { type = string }\n' if changed and name == "prod" else ""
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


def setup(tool):
    directory = Path(tempfile.mkdtemp(prefix="terracotta-environments-")).resolve()
    (directory / MARKER).write_text(str(directory) + "\n")
    try:
        for name in NAMES:
            child = directory / name
            child.mkdir()
            (child / "main.tf").write_text(configuration(name, False))
            run(child, tool, "init", "-input=false", "-no-color")
            run(child, tool, "apply", "-auto-approve", "-input=false", "-no-color")
            (child / "main.tf").write_text(configuration(name, True))
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
    for name in ("repair", "clean"):
        commands.add_parser(name).add_argument("directory", type=Path)
    args = parser.parse_args()
    if args.command == "setup":
        print(setup(args.tool))
    elif args.command == "repair":
        (checked(args.directory) / "prod/retry.auto.tfvars").write_text('release = "new"\n')
    else:
        shutil.rmtree(checked(args.directory))


if __name__ == "__main__":
    main()
