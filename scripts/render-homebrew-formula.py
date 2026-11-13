#!/usr/bin/env python3
"""Write the Homebrew formula for one Aura release.

The formula installs the release archives, so it needs their SHA-256
checksums. This script reads them from the release's `SHA256SUMS` file and
fills in `packaging/homebrew/aura.rb.in`.
"""
from __future__ import annotations

import argparse
from pathlib import Path
import re
import sys

REPO_ROOT = Path(__file__).resolve().parents[1]
TEMPLATE = REPO_ROOT / 'packaging' / 'homebrew' / 'aura.rb.in'
TAG = re.compile(r'v[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.-]+)?')
BASE_URL = re.compile(r'(https|file)://[^\s"#\\]+')
SHA256 = re.compile(r'[0-9a-f]{64}')
TARGETS = (
    'aarch64-apple-darwin',
    'x86_64-apple-darwin',
    'aarch64-unknown-linux-gnu',
    'x86_64-unknown-linux-gnu',
)


def checksums(sums_text):
    """Map each file name in `sha256sum` output to its checksum."""
    result = {}
    for line in sums_text.splitlines():
        fields = line.split(None, 1)
        if len(fields) == 2:
            result[fields[1].strip().lstrip('*')] = fields[0]
    return result


def render(template, tag, base_url, sums):
    """The formula text for release `tag`, whose archives are under `base_url`."""
    if not TAG.fullmatch(tag):
        raise ValueError(f'invalid release tag: {tag}')
    if not BASE_URL.fullmatch(base_url):
        raise ValueError(f'invalid base URL: {base_url}')
    values = {'TAG': tag, 'VERSION': tag[1:], 'BASE_URL': base_url.rstrip('/')}
    for target in TARGETS:
        archive = f'aura-{tag}-{target}.tar.gz'
        checksum = sums.get(archive)
        if checksum is None:
            raise ValueError(f'{archive} is missing from SHA256SUMS')
        if not SHA256.fullmatch(checksum):
            raise ValueError(f'{archive} has an invalid checksum: {checksum}')
        values['SHA256_' + target.upper().replace('-', '_')] = checksum
    formula = re.sub(r'@([A-Z0-9_]+)@', lambda match: values[match.group(1)], template)
    return formula


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument('--tag', required=True, help='release tag, such as v0.16.0-preview')
    parser.add_argument('--sha256sums', required=True, type=Path,
                        help="the release's SHA256SUMS file")
    parser.add_argument('--base-url', default=None,
                        help='where the archives are; defaults to the GitHub Release')
    parser.add_argument('--output', type=Path, default=None,
                        help='file to write; defaults to standard output')
    args = parser.parse_args(argv)
    base_url = args.base_url or (
        f'https://github.com/johnolafenwa/Aura/releases/download/{args.tag}')
    try:
        formula = render(TEMPLATE.read_text(encoding='utf-8'), args.tag, base_url,
                         checksums(args.sha256sums.read_text(encoding='utf-8')))
    except ValueError as error:
        print(f'render-homebrew-formula: {error}', file=sys.stderr)
        return 2
    if args.output is None:
        sys.stdout.write(formula)
    else:
        args.output.write_text(formula, encoding='utf-8')
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
