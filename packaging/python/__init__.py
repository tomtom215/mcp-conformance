# SPDX-License-Identifier: MIT
# Copyright 2026 Tom F. (https://github.com/tomtom215)

"""The mcp-conformance release binaries, installed by pip.

The wheel installs ``mcp-conformance``, ``mcp-trace-validator`` and
``mcp-trace-capture`` into the environment's scripts directory (``bin`` on
POSIX, ``Scripts`` on Windows) as the release binaries themselves, not as
Python wrappers. This package only finds them, for ``python -m
mcp_conformance`` and for callers that want a path. The source is
packaging/python in https://github.com/tomtom215/mcp-conformance.
"""

import os
import sys
import sysconfig

__all__ = ["binary_path"]

_NAMES = ("mcp-conformance", "mcp-trace-validator", "mcp-trace-capture")


def _candidates():
    """The scripts directories pip may have installed into, most likely first."""
    yield sysconfig.get_path("scripts")
    user_scheme = "nt_user" if os.name == "nt" else "posix_user"
    if sys.platform == "darwin" and "osx_framework_user" in sysconfig.get_scheme_names():
        user_scheme = "osx_framework_user"
    try:
        yield sysconfig.get_path("scripts", user_scheme)
    except KeyError:
        pass
    # `pip install --target DIR` puts the package in DIR and scripts in DIR/bin.
    yield os.path.join(os.path.dirname(os.path.dirname(os.path.abspath(__file__))), "bin")


def binary_path(name="mcp-conformance"):
    """The absolute path of one of the installed binaries.

    Raises ``FileNotFoundError`` when the binary is in none of the places pip
    installs scripts to, and ``ValueError`` for a name this wheel does not carry.
    """
    if name not in _NAMES:
        raise ValueError("not one of this package's binaries: " + repr(name))
    file = name + ".exe" if os.name == "nt" else name
    for directory in _candidates():
        path = os.path.join(directory, file)
        if os.path.isfile(path):
            return path
    raise FileNotFoundError(file + " is not in any scripts directory of this environment")
