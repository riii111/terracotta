#!/usr/bin/env python3

import argparse
from pathlib import Path
import sys

ROOT = Path(__file__).resolve().parents[3]
sys.path.insert(0, str(ROOT))

from fixtures.basic import testing  # noqa: E402


def main():
    parser = argparse.ArgumentParser(
        description="Manage the basic Terraform CLI test fixture."
    )
    commands = parser.add_subparsers(dest="command", required=True)
    commands.add_parser("setup")
    clean = commands.add_parser("clean")
    clean.add_argument("directory", type=Path)
    args = parser.parse_args()
    if args.command == "setup":
        print(testing.setup())
    else:
        testing.clean(args.directory)


if __name__ == "__main__":
    main()
