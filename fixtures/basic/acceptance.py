#!/usr/bin/env python3

import argparse
from pathlib import Path
import sys

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT))

from fixtures.basic import testing  # noqa: E402


def main():
    parser = argparse.ArgumentParser(
        description="Validate local plans with a real Terraform-compatible CLI."
    )
    parser.add_argument("--tool", choices=("terraform", "tofu"), required=True)
    tool = parser.parse_args().tool

    for setup in (testing.setup, testing.setup_group_expansion):
        directory = setup(tool)
        try:
            print(f"Verified {directory.name} with {tool}")
        finally:
            testing.clean(directory)


if __name__ == "__main__":
    main()
