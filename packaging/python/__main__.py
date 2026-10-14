# SPDX-License-Identifier: MIT
# Copyright 2026 Tom F. (https://github.com/tomtom215)

"""``python -m mcp_conformance [ARGS]`` runs the ``mcp-conformance`` binary.

For an environment whose scripts directory is not on ``PATH``. The binary
replaces this process on POSIX; on Windows, which cannot replace a process,
it runs as a child and its exit status becomes this one's.
"""

import os
import subprocess
import sys

from mcp_conformance import binary_path


def main():
    """Runs the binary with this process's arguments."""
    path = binary_path("mcp-conformance")
    if os.name == "nt":
        sys.exit(subprocess.call([path] + sys.argv[1:]))
    os.execv(path, [path] + sys.argv[1:])


if __name__ == "__main__":
    main()
