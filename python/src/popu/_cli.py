"""Thin console entry point; native stdout/stderr and exit codes pass through."""
from __future__ import annotations

import os
import subprocess
import sys

from ._errors import PopuError
from ._runtime import executable_path


def main() -> int:
    process: subprocess.Popen | None = None
    try:
        executable = executable_path()
        # Keep the caller's console handles valid. A new process group disables
        # Ctrl+C in the native child so this Python entry point owns cleanup.
        flags = subprocess.CREATE_NEW_PROCESS_GROUP if os.name == "nt" else 0
        process = subprocess.Popen(
            [str(executable), *sys.argv[1:]],
            stdin=sys.stdin,
            stdout=sys.stdout,
            stderr=sys.stderr,
            creationflags=flags,
        )
        while True:
            try:
                return process.wait(timeout=0.1)
            except subprocess.TimeoutExpired:
                continue
    except KeyboardInterrupt:
        return 130
    except (PopuError, OSError) as error:
        print(f"Popu error: {error}", file=sys.stderr)
        return 12
    finally:
        if process is not None:
            if process.poll() is None:
                process.kill()
            process.wait()
