#!/usr/bin/env python3
"""Inventory every Manual fence and execute its verified examples safely.

The sidecar metadata makes every fence an explicit contract. Aura source may
be checked, run, checked as an expected rejection, or checked in a local
package; one allowlisted CLI form may run without a shell. Every remaining
fence is classified as illustrative with a specific reason. Source hashes make
documentation edits fail closed until the block's contract is reviewed.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import re
import shlex
import subprocess
import sys
import tempfile
from collections import Counter, defaultdict
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Mapping, Sequence


AURA_FENCE_LANGUAGES = frozenset({"aura"})
VERIFIED_MODES = frozenset(
    {"check", "run", "check-fail", "package-check", "command"}
)
SUPPORTED_MODES = VERIFIED_MODES | {"illustrative"}
CANONICAL_REQUIRED_SECTIONS = (
    "Grammar",
    "Typing Rules",
    "Runtime Semantics",
    "Ownership And Evaluation Order",
    "Diagnostics",
    "Backend Support",
    "Limits And Implementation-Defined Behavior",
    "Status",
)
NO_DIAGNOSTICS_SENTINEL = "No feature-specific diagnostics."
FENCE_OPEN_RE = re.compile(r"^(?P<indent>[ \t]*)(?P<fence>`{3,}|~{3,})(?P<info>.*)$")
H2_RE = re.compile(r"^## (?P<title>.+?)\s*$")
DIAGNOSTIC_CODE_RE = re.compile(r"\bAU\d{4}\b")


@dataclass(frozen=True)
class ReferenceBlock:
    path: str
    ordinal: int
    line: int
    language: str
    source: str
    identifier_kind: str = "aura"

    @property
    def identifier(self) -> str:
        return f"{self.path}#{self.identifier_kind}-{self.ordinal}"

    @property
    def is_aura(self) -> bool:
        return self.language in AURA_FENCE_LANGUAGES

    @property
    def sha256(self) -> str:
        return hashlib.sha256(self.source.encode("utf-8")).hexdigest()


@dataclass(frozen=True)
class ManualInventory:
    pages: tuple[str, ...]
    fence_count: int
    fence_languages: Mapping[str, int]
    blocks: tuple[ReferenceBlock, ...]
    aura_blocks: tuple[ReferenceBlock, ...]

    @property
    def page_count(self) -> int:
        return len(self.pages)


def _fence_language(info: str) -> str:
    stripped = info.strip()
    if not stripped:
        return "unlabeled"
    return stripped.split(maxsplit=1)[0].lower()


def _is_closing_fence(line: str, marker: str) -> bool:
    stripped = line.strip()
    return bool(stripped) and set(stripped) == {marker[0]} and len(stripped) >= len(marker)


def collect_manual(manual_dir: Path) -> ManualInventory:
    pages: list[str] = []
    blocks: list[ReferenceBlock] = []
    aura_blocks: list[ReferenceBlock] = []
    languages: Counter[str] = Counter()
    fence_count = 0

    for page in sorted(manual_dir.rglob("*.md")):
        relative = f"docs/manual/{page.relative_to(manual_dir).as_posix()}"
        pages.append(relative)
        lines = page.read_text(encoding="utf-8").splitlines(keepends=True)
        index = 0
        aura_ordinal = 0
        non_aura_ordinals: Counter[str] = Counter()
        while index < len(lines):
            match = FENCE_OPEN_RE.match(lines[index].rstrip("\r\n"))
            if match is None:
                index += 1
                continue
            marker = match.group("fence")
            language = _fence_language(match.group("info"))
            opening_line = index + 1
            index += 1
            body: list[str] = []
            while index < len(lines) and not _is_closing_fence(lines[index], marker):
                body.append(lines[index])
                index += 1
            if index >= len(lines):
                raise ValueError(f"unterminated fence at {relative}:{opening_line}")
            index += 1
            fence_count += 1
            languages[language] += 1
            source = "".join(body)
            if source and not source.endswith("\n"):
                source += "\n"
            if language in AURA_FENCE_LANGUAGES:
                aura_ordinal += 1
                ordinal = aura_ordinal
                identifier_kind = "aura"
            else:
                non_aura_ordinals[language] += 1
                ordinal = non_aura_ordinals[language]
                identifier_kind = language
            block = ReferenceBlock(
                path=relative,
                ordinal=ordinal,
                line=opening_line,
                language=language,
                source=source,
                identifier_kind=identifier_kind,
            )
            blocks.append(block)
            if block.is_aura:
                aura_blocks.append(block)

    return ManualInventory(
        pages=tuple(pages),
        fence_count=fence_count,
        fence_languages=dict(sorted(languages.items())),
        blocks=tuple(blocks),
        aura_blocks=tuple(aura_blocks),
    )


def validate_block_metadata(
    blocks: Sequence[ReferenceBlock], metadata: Mapping[str, Any]
) -> list[str]:
    errors: list[str] = []
    expected = {block.identifier: block for block in blocks}
    actual_ids = set(metadata)

    for identifier in sorted(set(expected) - actual_ids):
        block = expected[identifier]
        errors.append(
            f"missing metadata for {identifier} (opening fence at line {block.line})"
        )
    for identifier in sorted(actual_ids - set(expected)):
        errors.append(f"metadata names a block that no longer exists: {identifier}")

    for identifier in sorted(set(expected) & actual_ids):
        block = expected[identifier]
        entry = metadata[identifier]
        if not isinstance(entry, Mapping):
            errors.append(f"metadata for {identifier} must be an object")
            continue
        if entry.get("sha256") != block.sha256:
            errors.append(
                f"stale sha256 for {identifier}: expected {block.sha256}, "
                f"found {entry.get('sha256')!r}"
            )
        mode = entry.get("mode")
        if mode not in SUPPORTED_MODES:
            errors.append(
                f"metadata for {identifier} has unsupported mode {mode!r}"
            )
            continue
        if mode == "illustrative":
            reason = entry.get("reason")
            if not isinstance(reason, str) or not reason.strip():
                errors.append(
                    f"illustrative metadata for {identifier} needs a non-empty reason"
                )
            continue
        if mode in {"check", "run", "package-check", "command"}:
            for stream in ("stdout", "stderr"):
                if not isinstance(entry.get(stream), str):
                    errors.append(
                        f"{mode} metadata for {identifier} must pin exact {stream}"
                    )
            if mode in {"check", "run", "package-check"} and not block.is_aura:
                errors.append(
                    f"{mode} metadata for {identifier} requires an Aura/Python fence"
                )
            if mode == "command" and block.language != "bash":
                errors.append(
                    f"command metadata for {identifier} requires a bash fence"
                )
            if mode == "package-check":
                entry_path = entry.get("entry", "src/main.au")
                files = entry.get("files")
                if not isinstance(entry_path, str) or not entry_path.strip():
                    errors.append(
                        f"package-check metadata for {identifier} needs a non-empty entry"
                    )
                if not isinstance(files, Mapping) or not files:
                    errors.append(
                        f"package-check metadata for {identifier} needs supporting files"
                    )
                elif not all(
                    isinstance(path, str)
                    and path.strip()
                    and isinstance(contents, str)
                    for path, contents in files.items()
                ):
                    errors.append(
                        f"package-check files for {identifier} must map paths to text"
                    )
            continue
        exit_code = entry.get("exit_code")
        if mode == "check-fail" and not block.is_aura:
            errors.append(
                f"check-fail metadata for {identifier} requires an Aura/Python fence"
            )
        if not isinstance(exit_code, int) or exit_code == 0:
            errors.append(
                f"check-fail metadata for {identifier} must pin a non-zero exit_code"
            )
        diagnostic = entry.get("stderr_contains")
        if not isinstance(diagnostic, str) or not diagnostic.strip():
            errors.append(
                f"check-fail metadata for {identifier} must pin stderr_contains"
            )

    return errors


def audit_feature_executable_examples(
    blocks: Sequence[ReferenceBlock],
    metadata: Mapping[str, Any],
    page_roles: Mapping[str, Any],
) -> list[str]:
    verified_pages = {
        block.path
        for block in blocks
        if isinstance(metadata.get(block.identifier), Mapping)
        and metadata[block.identifier].get("mode") in VERIFIED_MODES
        and metadata[block.identifier].get("sha256") == block.sha256
    }
    return [
        path
        for path, role in sorted(page_roles.items())
        if isinstance(role, Mapping)
        and role.get("kind") == "feature"
        and path not in verified_pages
    ]


def validate_page_roles(
    pages: Sequence[str], page_roles: Mapping[str, Any]
) -> list[str]:
    errors: list[str] = []
    page_set = set(pages)
    role_set = set(page_roles)
    for path in sorted(page_set - role_set):
        errors.append(f"Manual page has no feature/structural classification: {path}")
    for path in sorted(role_set - page_set):
        errors.append(f"page classification names a missing Manual page: {path}")
    for path in sorted(page_set & role_set):
        entry = page_roles[path]
        if not isinstance(entry, Mapping):
            errors.append(f"page classification for {path} must be an object")
            continue
        kind = entry.get("kind")
        if kind not in {"feature", "structural"}:
            errors.append(f"page classification for {path} has invalid kind {kind!r}")
        if kind == "structural":
            reason = entry.get("reason")
            if not isinstance(reason, str) or not reason.strip():
                errors.append(
                    f"structural page classification for {path} needs a reason"
                )
    return errors


def _section_bodies(text: str) -> tuple[dict[str, str], set[str]]:
    lines = text.splitlines()
    headings: list[tuple[str, int]] = []
    duplicates: set[str] = set()
    seen: set[str] = set()
    for index, line in enumerate(lines):
        match = H2_RE.match(line)
        if match is None:
            continue
        title = match.group("title")
        if title in seen:
            duplicates.add(title)
        seen.add(title)
        headings.append((title, index))
    bodies: dict[str, str] = {}
    for heading_index, (title, line_index) in enumerate(headings):
        next_index = (
            headings[heading_index + 1][1]
            if heading_index + 1 < len(headings)
            else len(lines)
        )
        bodies[title] = "\n".join(lines[line_index + 1 : next_index]).strip()
    return bodies, duplicates


def audit_normative_sections(
    manual_dir: Path,
    page_roles: Mapping[str, Any],
    required_sections: Sequence[str] = CANONICAL_REQUIRED_SECTIONS,
) -> dict[str, list[str]]:
    missing: dict[str, list[str]] = {}
    for path, role in sorted(page_roles.items()):
        if not isinstance(role, Mapping) or role.get("kind") != "feature":
            continue
        page = manual_dir / path.removeprefix("docs/manual/")
        if not page.is_file():
            continue
        bodies, duplicates = _section_bodies(page.read_text(encoding="utf-8"))
        page_missing: list[str] = []
        for section in required_sections:
            if section not in bodies:
                page_missing.append(section)
                continue
            if not bodies[section]:
                page_missing.append(f"{section} (empty)")
            if section in duplicates:
                page_missing.append(f"{section} (duplicate heading)")
        diagnostics = bodies.get("Diagnostics", "")
        if diagnostics and not (
            DIAGNOSTIC_CODE_RE.search(diagnostics)
            or NO_DIAGNOSTICS_SENTINEL in diagnostics
        ):
            page_missing.append(
                "Diagnostics (must name AU#### codes or explicitly state none)"
            )
        if page_missing:
            missing[path] = page_missing
    return missing


def _safe_example_name(block: ReferenceBlock) -> str:
    stem = (
        block.path.removeprefix("docs/manual/")
        .removesuffix(".md")
        .replace("/", "__")
        .replace("-", "_")
    )
    return f"{stem}_{block.ordinal}.au"


def _safe_relative_path(path: str) -> Path | None:
    candidate = Path(path)
    if candidate.is_absolute() or not candidate.parts or ".." in candidate.parts:
        return None
    return candidate


def _safe_command_argv(
    block: ReferenceBlock, aura_binary: Path, repository_root: Path
) -> tuple[list[str] | None, str | None]:
    lines = [line.strip() for line in block.source.splitlines() if line.strip()]
    if len(lines) != 1:
        return None, "safe command mode requires exactly one non-empty command line"
    try:
        argv = shlex.split(lines[0], posix=True)
    except ValueError as error:
        return None, f"cannot parse command safely: {error}"
    cargo_prefix = ["cargo", "run", "-p", "aura", "--"]
    if argv[: len(cargo_prefix)] == cargo_prefix:
        aura_args = argv[len(cargo_prefix) :]
    elif argv[:1] == ["aura"]:
        aura_args = argv[1:]
    else:
        return None, "safe command mode permits only `aura` CLI commands"
    if len(aura_args) != 2 or aura_args[0] not in {"check", "run"}:
        return None, "safe command mode permits only `aura check PATH` or `aura run PATH`"
    source = _safe_relative_path(aura_args[1])
    if (
        source is None
        or source.suffix != ".au"
        or not source.parts
        or source.parts[0] != "examples"
    ):
        return None, "safe command mode requires a repository examples/*.au path"
    resolved = (repository_root / source).resolve()
    try:
        resolved.relative_to(repository_root.resolve())
    except ValueError:
        return None, "safe command path escapes the repository"
    if not resolved.is_file():
        return None, f"safe command source does not exist: {source}"
    return [str(aura_binary), aura_args[0], str(resolved)], None


def _write_package_example(
    directory: Path, block: ReferenceBlock, entry: Mapping[str, Any]
) -> tuple[Path | None, str | None]:
    entry_name = entry.get("entry", "src/main.au")
    entry_path = _safe_relative_path(entry_name) if isinstance(entry_name, str) else None
    if entry_path is None:
        return None, "package-check entry must be a safe relative path"
    files = entry.get("files")
    if not isinstance(files, Mapping):
        return None, "package-check needs supporting files"
    package_root = directory / _safe_example_name(block).removesuffix(".au")
    for relative_name, contents in files.items():
        relative = (
            _safe_relative_path(relative_name)
            if isinstance(relative_name, str)
            else None
        )
        if relative is None or not isinstance(contents, str):
            return None, "package-check files must use safe relative text paths"
        destination = package_root / relative
        destination.parent.mkdir(parents=True, exist_ok=True)
        destination.write_text(contents, encoding="utf-8")
    source_path = package_root / entry_path
    source_path.parent.mkdir(parents=True, exist_ok=True)
    source_path.write_text(block.source, encoding="utf-8")
    return source_path, None


def execute_examples(
    blocks: Sequence[ReferenceBlock],
    metadata: Mapping[str, Any],
    aura_binary: Path,
    repository_root: Path,
) -> list[str]:
    errors: list[str] = []
    with tempfile.TemporaryDirectory(prefix="aura-reference-") as directory:
        extracted = Path(directory)
        for block in blocks:
            entry = metadata.get(block.identifier)
            if not isinstance(entry, Mapping):
                continue
            mode = entry.get("mode")
            if mode == "illustrative" or entry.get("sha256") != block.sha256:
                continue
            if mode == "command":
                argv, preparation_error = _safe_command_argv(
                    block, aura_binary, repository_root
                )
                command = "command"
                command_cwd = repository_root
            elif mode == "package-check":
                source_path, preparation_error = _write_package_example(
                    extracted, block, entry
                )
                argv = (
                    [str(aura_binary), "check", str(source_path)]
                    if source_path is not None
                    else None
                )
                command = "package-check"
                command_cwd = source_path.parent if source_path is not None else extracted
            elif mode in {"check", "run", "check-fail"}:
                command = "check" if mode in {"check", "check-fail"} else "run"
                source_path = extracted / _safe_example_name(block)
                source_path.write_text(block.source, encoding="utf-8")
                argv = [str(aura_binary), command, str(source_path)]
                preparation_error = None
                command_cwd = repository_root
            else:
                continue
            if preparation_error is not None or argv is None:
                errors.append(
                    f"{block.identifier}: cannot prepare {mode}: {preparation_error}"
                )
                continue
            try:
                completed = subprocess.run(
                    argv,
                    cwd=command_cwd,
                    text=True,
                    stdout=subprocess.PIPE,
                    stderr=subprocess.PIPE,
                    timeout=20,
                    check=False,
                )
            except subprocess.TimeoutExpired:
                errors.append(f"{block.identifier}: aura {command} timed out after 20s")
                continue
            if mode in {"check", "run", "package-check", "command"}:
                if completed.returncode != 0:
                    errors.append(
                        f"{block.identifier}: aura {command} exited "
                        f"{completed.returncode}; stderr={completed.stderr!r}"
                    )
                    continue
                if completed.stdout != entry.get("stdout"):
                    errors.append(
                        f"{block.identifier}: stdout mismatch; expected "
                        f"{entry.get('stdout')!r}, got {completed.stdout!r}"
                    )
                if completed.stderr != entry.get("stderr"):
                    errors.append(
                        f"{block.identifier}: stderr mismatch; expected "
                        f"{entry.get('stderr')!r}, got {completed.stderr!r}"
                    )
                continue
            if mode == "check-fail":
                if completed.returncode != entry.get("exit_code"):
                    errors.append(
                        f"{block.identifier}: rejection exit mismatch; expected "
                        f"{entry.get('exit_code')}, got {completed.returncode}"
                    )
                expected_diagnostic = entry.get("stderr_contains", "")
                if expected_diagnostic not in completed.stderr:
                    errors.append(
                        f"{block.identifier}: rejection stderr does not contain "
                        f"{expected_diagnostic!r}; got {completed.stderr!r}"
                    )
    return errors


def _load_metadata(path: Path) -> tuple[dict[str, Any], list[str]]:
    try:
        payload = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error:
        return {}, [f"cannot read reference metadata {path}: {error}"]
    if not isinstance(payload, dict):
        return {}, [f"reference metadata {path} must contain a JSON object"]
    return payload, []


def _resolve_aura_binary(repository_root: Path, requested: str | None) -> Path:
    candidate = Path(requested) if requested else repository_root / "target/debug/aura"
    if not candidate.is_absolute():
        candidate = repository_root / candidate
    if requested is None:
        subprocess.run(
            ["cargo", "build", "--quiet", "-p", "aura"],
            cwd=repository_root,
            check=True,
        )
    if not candidate.is_file() or not os.access(candidate, os.X_OK):
        raise FileNotFoundError(f"Aura CLI is not executable: {candidate}")
    return candidate


def _mode_counts(
    blocks: Sequence[ReferenceBlock], metadata: Mapping[str, Any]
) -> Counter[str]:
    counts: Counter[str] = Counter()
    for block in blocks:
        entry = metadata.get(block.identifier)
        if isinstance(entry, Mapping):
            mode = entry.get("mode")
            if isinstance(mode, str):
                counts[mode] += 1
    return counts


def _print_inventory(
    inventory: ManualInventory,
    metadata: Mapping[str, Any],
    page_roles: Mapping[str, Any],
) -> None:
    modes = _mode_counts(inventory.blocks, metadata)
    verified = sum(modes[mode] for mode in VERIFIED_MODES)
    aura_modes = _mode_counts(inventory.aura_blocks, metadata)
    aura_verified = sum(aura_modes[mode] for mode in VERIFIED_MODES)
    feature_pages = sum(
        1
        for role in page_roles.values()
        if isinstance(role, Mapping) and role.get("kind") == "feature"
    )
    structural_pages = sum(
        1
        for role in page_roles.values()
        if isinstance(role, Mapping) and role.get("kind") == "structural"
    )
    print("Manual reference inventory")
    print(f"  pages: {inventory.page_count} ({feature_pages} feature, {structural_pages} structural)")
    print(f"  all fenced blocks: {inventory.fence_count}")
    print(
        "  fenced-block languages: "
        + ", ".join(
            f"{language}={count}"
            for language, count in inventory.fence_languages.items()
        )
    )
    print(
        f"  verified fenced blocks: {verified}; "
        f"illustrative fenced blocks: {modes['illustrative']}"
    )
    if verified:
        print(
            "  verified modes: "
            + ", ".join(
                f"{mode}={modes[mode]}"
                for mode in sorted(VERIFIED_MODES)
                if modes[mode]
            )
        )
    print(f"  Aura blocks: {len(inventory.aura_blocks)}")
    print(
        "  verified Aura blocks: "
        f"{aura_verified} (check={aura_modes['check']}, "
        f"run={aura_modes['run']}, "
        f"expected-rejection={aura_modes['check-fail']}, "
        f"package-check={aura_modes['package-check']})"
    )
    print(f"  illustrative Aura blocks: {aura_modes['illustrative']}")
    print("  per-page fenced blocks:")
    page_blocks: dict[str, list[ReferenceBlock]] = defaultdict(list)
    for block in inventory.blocks:
        page_blocks[block.path].append(block)
    for path in inventory.pages:
        blocks = page_blocks[path]
        if not blocks:
            continue
        counts = _mode_counts(blocks, metadata)
        page_verified = sum(counts[mode] for mode in VERIFIED_MODES)
        aura_count = sum(1 for block in blocks if block.is_aura)
        print(
            f"    {path}: total={len(blocks)}, verified={page_verified}, "
            f"illustrative={counts['illustrative']}, aura={aura_count}"
        )


def _parse_args(argv: Sequence[str] | None) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--root",
        type=Path,
        default=Path(__file__).resolve().parent.parent,
        help="repository root",
    )
    parser.add_argument(
        "--metadata",
        type=Path,
        help="metadata path (defaults to scripts/reference-integrity.json)",
    )
    parser.add_argument(
        "--aura-bin",
        default=os.environ.get("AURA_BIN"),
        help="Aura CLI binary (or set AURA_BIN)",
    )
    parser.add_argument(
        "--inventory-only",
        action="store_true",
        help="validate and report metadata/sections without invoking the compiler",
    )
    return parser.parse_args(argv)


def main(argv: Sequence[str] | None = None) -> int:
    args = _parse_args(argv)
    root = args.root.resolve()
    manual_dir = root / "docs/manual"
    metadata_path = (
        args.metadata.resolve()
        if args.metadata is not None
        else root / "scripts/reference-integrity.json"
    )
    try:
        inventory = collect_manual(manual_dir)
    except (OSError, ValueError) as error:
        print(f"reference-integrity error: {error}", file=sys.stderr)
        return 1
    payload, errors = _load_metadata(metadata_path)
    blocks = payload.get("blocks", {})
    page_roles = payload.get("pages", {})
    required_sections = payload.get("required_sections", [])
    if payload.get("schema_version") != 1:
        errors.append("reference metadata schema_version must be 1")
    if not isinstance(blocks, Mapping):
        errors.append("reference metadata blocks must be an object")
        blocks = {}
    if not isinstance(page_roles, Mapping):
        errors.append("reference metadata pages must be an object")
        page_roles = {}
    if tuple(required_sections) != CANONICAL_REQUIRED_SECTIONS:
        errors.append(
            "reference metadata required_sections must match the canonical "
            "eight-section feature-page contract"
        )
        required_sections = CANONICAL_REQUIRED_SECTIONS
    errors.extend(validate_block_metadata(inventory.blocks, blocks))
    errors.extend(validate_page_roles(inventory.pages, page_roles))
    missing_sections = audit_normative_sections(
        manual_dir, page_roles, required_sections
    )
    feature_pages_without_examples = audit_feature_executable_examples(
        inventory.blocks, blocks, page_roles
    )

    _print_inventory(inventory, blocks, page_roles)
    if missing_sections:
        print("  feature pages missing normative sections:")
        for path, sections in missing_sections.items():
            print(f"    {path}: {', '.join(sections)}")
    else:
        print("  feature pages missing normative sections: none")
    if feature_pages_without_examples:
        print("  feature pages without a verified executable example:")
        for path in feature_pages_without_examples:
            print(f"    {path}")
    else:
        print("  feature pages without a verified executable example: none")

    if not errors and not args.inventory_only:
        try:
            aura_binary = _resolve_aura_binary(root, args.aura_bin)
        except (OSError, subprocess.CalledProcessError) as error:
            errors.append(f"cannot prepare Aura CLI: {error}")
        else:
            errors.extend(
                execute_examples(inventory.blocks, blocks, aura_binary, root)
            )

    if missing_sections:
        errors.append(
            f"{len(missing_sections)} feature page(s) do not satisfy the "
            "normative section contract"
        )
    if feature_pages_without_examples:
        errors.append(
            f"{len(feature_pages_without_examples)} feature page(s) have no "
            "verified executable fenced example"
        )
    if errors:
        print("reference-integrity failures:", file=sys.stderr)
        for error in errors:
            print(f"  - {error}", file=sys.stderr)
        return 1
    print("Reference integrity passed.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
