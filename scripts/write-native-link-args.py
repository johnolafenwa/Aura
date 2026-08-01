#!/usr/bin/env python3
"""Write a validated native-link manifest from Cargo's rustc output."""

from __future__ import annotations

import json
import os
from pathlib import Path
import unicodedata


def strip_ansi_escape_sequences(text: str) -> str:
    clean: list[str] = []
    cursor = 0
    while cursor < len(text):
        if text[cursor] != "\x1b":
            clean.append(text[cursor])
            cursor += 1
            continue

        escape_start = cursor
        cursor += 1
        if cursor >= len(text):
            clean.append("\x1b")
            break
        introducer = text[cursor]
        cursor += 1
        sequence_end: int | None = None
        if introducer == "[":
            while cursor < len(text) and 0x30 <= ord(text[cursor]) <= 0x3F:
                cursor += 1
            while cursor < len(text) and 0x20 <= ord(text[cursor]) <= 0x2F:
                cursor += 1
            if cursor < len(text) and 0x40 <= ord(text[cursor]) <= 0x7E:
                sequence_end = cursor + 1
        elif introducer in ("]", "P", "X", "^", "_"):
            allow_bel = introducer == "]"
            payload_start = cursor
            while cursor < len(text):
                if text[cursor] == "\x07":
                    if allow_bel and not any(
                        unicodedata.category(character) == "Cc"
                        for character in text[payload_start:cursor]
                    ):
                        sequence_end = cursor + 1
                    break
                if text[cursor] == "\x1b" and text[cursor : cursor + 2] == "\x1b\\":
                    if not any(
                        unicodedata.category(character) == "Cc"
                        for character in text[payload_start:cursor]
                    ):
                        sequence_end = cursor + 2
                    break
                if text[cursor] == "\x1b":
                    break
                cursor += 1
        elif 0x20 <= ord(introducer) <= 0x2F:
            while cursor < len(text) and 0x20 <= ord(text[cursor]) <= 0x2F:
                cursor += 1
            if cursor < len(text) and 0x30 <= ord(text[cursor]) <= 0x7E:
                sequence_end = cursor + 1
        elif 0x30 <= ord(introducer) <= 0x7E:
            sequence_end = cursor

        if sequence_end is not None:
            cursor = sequence_end
        else:
            clean.append(text[escape_start])
            cursor = escape_start + 1
    return "".join(clean)


def native_link_args(cargo_output: str) -> list[str]:
    marker = "native-static-libs:"
    matching = [
        line.split(marker, 1)[1].strip()
        for line in strip_ansi_escape_sequences(cargo_output).splitlines()
        if marker in line
    ]
    if not matching:
        raise SystemExit("rustc did not report native-static-libs")

    arguments = matching[-1].split()
    for argument in arguments:
        if any(unicodedata.category(character) == "Cc" for character in argument):
            raise SystemExit(
                f"Aura runtime link argument {argument!r} contains a control character"
            )
    return arguments


def main() -> None:
    target = Path(os.environ["ARCHIVE_ROOT"]) / "lib/aura/native-link-args.json"
    target.parent.mkdir(parents=True, exist_ok=True)
    target.write_text(
        json.dumps(native_link_args(os.environ["NATIVE_STATIC_LIBS"])) + "\n",
        encoding="utf-8",
    )


if __name__ == "__main__":
    main()
