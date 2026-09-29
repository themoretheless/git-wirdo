#!/usr/bin/env python3
"""Preserve CI exit codes and expose concise failures in GitHub check annotations."""

import subprocess
import sys


def main():
    if len(sys.argv) < 2:
        print("Usage: run_check.py COMMAND [ARG ...]", file=sys.stderr)
        return 2

    result = subprocess.run(
        sys.argv[1:], stdout=subprocess.PIPE, stderr=subprocess.STDOUT
    )
    output = result.stdout.decode("utf-8", errors="replace")
    print(output, end="" if output.endswith("\n") else "\n", flush=True)
    if result.returncode:
        diagnostic = f"Exit code {result.returncode}:\n{output[-1800:]}"
        diagnostic = diagnostic.replace("%", "%25").replace("\r", "%0D").replace("\n", "%0A")
        print(f"::error title=Rust check failed::{diagnostic}", flush=True)
    return result.returncode


if __name__ == "__main__":
    sys.exit(main())
