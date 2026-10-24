#!/usr/bin/env python3
"""Check that a release `aura` loads only libraries every supported host has.

`aura` links LLVM's static libraries, and LLVM names system libraries of its
own: zlib, zstd, and the C++ standard library. A build machine can satisfy
those from a package manager that users do not have, such as Homebrew, and
the archive then fails to start elsewhere. This script reads the
executable's dynamic dependencies and fails on any outside the allowed set:

- on Linux, the `DT_NEEDED` entries must be glibc, the GCC runtime and C++
  library, zlib, or zstd;
- on macOS, every load command must name a library under `/usr/lib/` or
  `/System/Library/`.
"""
from __future__ import annotations

import argparse
import platform
from pathlib import Path
import re
import subprocess
import sys

LINUX_ALLOWED = {
    'ld-linux-x86-64.so.2',
    'ld-linux-aarch64.so.1',
    'libc.so.6',
    'libdl.so.2',
    'libgcc_s.so.1',
    'libm.so.6',
    'libpthread.so.0',
    'librt.so.1',
    'libstdc++.so.6',
    'libutil.so.1',
    'libz.so.1',
    'libzstd.so.1',
}
MACOS_PREFIXES = ('/usr/lib/', '/System/Library/')
NEEDED = re.compile(r'\(NEEDED\)\s+Shared library: \[([^\]]+)\]')


def linux_libraries(readelf_output):
    """The `DT_NEEDED` names in `readelf -d` output."""
    return NEEDED.findall(readelf_output)


def macos_libraries(otool_output):
    """The library paths in `otool -L` output, without the file's own line."""
    libraries = []
    for line in otool_output.splitlines()[1:]:
        line = line.strip()
        if line:
            libraries.append(line.split(' (compatibility version', 1)[0].strip())
    return libraries


def disallowed(system, libraries):
    """The libraries a release `aura` must not load on `system`."""
    if system == 'Linux':
        return [library for library in libraries if library not in LINUX_ALLOWED]
    if system == 'Darwin':
        return [library for library in libraries if not library.startswith(MACOS_PREFIXES)]
    raise ValueError(f'no library rule for {system}')


def libraries_of(executable, system):
    if system == 'Linux':
        command, parse = ['readelf', '-d', str(executable)], linux_libraries
    elif system == 'Darwin':
        command, parse = ['otool', '-L', str(executable)], macos_libraries
    else:
        raise ValueError(f'no library rule for {system}')
    output = subprocess.run(command, check=True, stdout=subprocess.PIPE, text=True).stdout
    return parse(output)


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument('executable', type=Path, nargs='?',
                        default=Path(__file__).resolve().parent.parent / 'target/release/aura')
    args = parser.parse_args(argv)
    system = platform.system()
    libraries = libraries_of(args.executable, system)
    for library in libraries:
        print(library)
    bad = disallowed(system, libraries)
    for library in bad:
        print(f'error: {args.executable} loads {library}, which users may not have',
              file=sys.stderr)
    return 1 if bad else 0


if __name__ == '__main__':
    sys.exit(main())
