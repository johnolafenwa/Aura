#!/usr/bin/env python3
"""Repository-wide identity and current-surface regression tests for Aura."""

from __future__ import annotations

from pathlib import Path
import re
import subprocess
import unittest


ROOT = Path(__file__).resolve().parents[1]
OLD = "Auro" + "ra"
OLD_LOWER = OLD.lower()

# Construct these spellings so this guard does not need to exempt its own
# implementation from a raw repository search.
NONCANONICAL_COLLECTION_TYPES = (
    "V" + "ec",
    "M" + "ap",
    "S" + "et",
    "S" + "tring",
)
NONCANONICAL_COLLECTION_METHODS = (
    "pu" + "sh",
    "sort" + "_by",
    "contains" + "_key",
    "entr" + "ies",
    "from" + "_vec",
)
CANONICAL_COLLECTION_TYPES = ("list", "dict", "set", "str")
CANONICAL_COLLECTION_METHODS = ("append", "sort", "contains", "items", "from_list")


def _mask_aura_comments_and_strings(source: str) -> str:
    """Preserve Aura source positions while hiding comments and literals."""

    masked = list(source)
    index = 0
    quote: str | None = None
    triple = False
    while index < len(source):
        if quote is not None:
            if source[index] == "\\" and not triple:
                masked[index] = " "
                if index + 1 < len(source) and source[index + 1] != "\n":
                    masked[index + 1] = " "
                    index += 2
                    continue
            closing = quote * (3 if triple else 1)
            if source.startswith(closing, index):
                for offset in range(len(closing)):
                    masked[index + offset] = " "
                index += len(closing)
                quote = None
                triple = False
                continue
            if source[index] != "\n":
                masked[index] = " "
            index += 1
            continue

        if source[index] == "#":
            while index < len(source) and source[index] != "\n":
                masked[index] = " "
                index += 1
            continue
        if source[index] in {'"', "'"}:
            quote = source[index]
            triple = source.startswith(quote * 3, index)
            length = 3 if triple else 1
            for offset in range(length):
                masked[index + offset] = " "
            index += length
            continue
        index += 1
    return "".join(masked)


def _line_number(text: str, offset: int) -> int:
    return text.count("\n", 0, offset) + 1


def _user_nominals(source: str) -> set[str]:
    return {
        match.group(1)
        for match in re.finditer(
            r"(?m)^\s*(?:public\s+)?(?:class|enum|trait)\s+([A-Za-z_]\w*)\b",
            source,
        )
    }


def _shadowed_surface_identifiers(source: str) -> set[str]:
    """Find builtin-looking words used as ordinary user bindings."""

    candidates = "|".join(map(re.escape, NONCANONICAL_COLLECTION_TYPES))
    shadowed = _user_nominals(source)
    patterns = (
        rf"(?m)^\s*(?:public\s+)?def\s+({candidates})\s*\(",
        rf"(?m)^\s*(?:mut\s+)?({candidates})\s*(?::[^=\n]+)?=",
        rf"(?:\(|,)\s*(?:own\s+|mut\s+)?({candidates})\s*:",
        rf"(?m)^\s*for\s+({candidates})\s+in\b",
    )
    for pattern in patterns:
        shadowed.update(match.group(1) for match in re.finditer(pattern, source))
    return shadowed


def _user_method_receivers(source: str) -> tuple[dict[str, set[str]], dict[str, str]]:
    """Return locally provable user methods and receiver nominal types."""

    methods: dict[str, set[str]] = {}
    block: tuple[int, str] | None = None
    for line in source.splitlines():
        if not line.strip():
            continue
        indent = len(line) - len(line.lstrip(" "))
        header = re.match(
            r"\s*(?:public\s+)?(?:class|trait)\s+([A-Za-z_]\w*)\b", line
        )
        if header:
            block = (indent, header.group(1))
            methods.setdefault(header.group(1), set())
            continue
        impl = re.match(
            r"\s*impl(?:\s+[A-Za-z_]\w*(?:\[[^\]]+\])?\s+for)?\s+"
            r"([A-Za-z_]\w*)\b",
            line,
        )
        if impl:
            block = (indent, impl.group(1))
            methods.setdefault(impl.group(1), set())
            continue
        if block is not None and indent <= block[0]:
            block = None
        if block is not None:
            method = re.match(r"\s*(?:public\s+)?def\s+([A-Za-z_]\w*)\s*\(", line)
            if method:
                methods[block[1]].add(method.group(1))

    receivers: dict[str, str] = {}
    nominal_alternation = "|".join(map(re.escape, methods))
    if nominal_alternation:
        for match in re.finditer(
            rf"\b([A-Za-z_]\w*)\s*:\s*(?:own\s+|mut\s+)?"
            rf"({nominal_alternation})\b",
            source,
        ):
            receivers[match.group(1)] = match.group(2)
        for match in re.finditer(
            rf"\b([A-Za-z_]\w*)\s*=\s*({nominal_alternation})"
            rf"(?:\s*\[[^\n\]]*\])?\s*\(",
            source,
        ):
            receivers[match.group(1)] = match.group(2)
        for match in re.finditer(
            rf"\b([A-Za-z_]\w*)\s*=\s*({nominal_alternation})"
            rf"\.[A-Za-z_]\w*\s*\(",
            source,
        ):
            receivers[match.group(1)] = match.group(2)
    return methods, receivers


def scan_aura_collection_surface(source: str) -> list[tuple[int, str]]:
    """Find noncanonical builtin collection syntax in one Aura source unit.

    The scan is deliberately lexical and local. Comments, string contents,
    user-declared nominal types, enum variants, and locally provable
    user-defined methods do not look like builtin collection syntax.
    """

    code = _mask_aura_comments_and_strings(source)
    nominals = _shadowed_surface_identifiers(code)
    methods, receivers = _user_method_receivers(code)
    findings: list[tuple[int, str]] = []

    for spelling in NONCANONICAL_COLLECTION_TYPES:
        if spelling in nominals:
            continue
        if spelling == NONCANONICAL_COLLECTION_TYPES[3]:
            pattern = re.compile(rf"(?<![.\w]){re.escape(spelling)}\b")
        else:
            pattern = re.compile(rf"(?<![.\w]){re.escape(spelling)}\s*\[")
        findings.extend(
            (_line_number(code, match.start()), f"builtin type {spelling}")
            for match in pattern.finditer(code)
        )

    set_spelling = re.escape(NONCANONICAL_COLLECTION_TYPES[2])
    for match in re.finditer(rf"(?<![.\w]){set_spelling}\s*\{{", code):
        if NONCANONICAL_COLLECTION_TYPES[2] not in nominals:
            findings.append((_line_number(code, match.start()), "builtin set literal"))

    class_at_line: dict[int, str] = {}
    current: tuple[int, str] | None = None
    for number, line in enumerate(code.splitlines(), start=1):
        if not line.strip():
            continue
        indent = len(line) - len(line.lstrip(" "))
        header = re.match(r"\s*(?:public\s+)?class\s+([A-Za-z_]\w*)\b", line)
        if header:
            current = (indent, header.group(1))
        elif current is not None and indent <= current[0]:
            current = None
        if current is not None:
            class_at_line[number] = current[1]

    noncanonical_method_pattern = "|".join(
        map(re.escape, NONCANONICAL_COLLECTION_METHODS)
    )
    for match in re.finditer(
        rf"\b(?P<receiver>[A-Za-z_]\w*)\."
        rf"(?P<method>{noncanonical_method_pattern})\s*\(",
        code,
    ):
        line = _line_number(code, match.start())
        receiver = match.group("receiver")
        method = match.group("method")
        nominal = class_at_line.get(line) if receiver == "self" else receivers.get(receiver)
        if nominal is not None and method in methods.get(nominal, set()):
            continue
        findings.append((line, f"builtin method {method}"))

    array_from_vec = re.compile(
        rf"\bArray\s*\[[^\]\n]+\]\s*\.\s*"
        rf"{re.escape(NONCANONICAL_COLLECTION_METHODS[-1])}\s*\("
    )
    findings.extend(
        (_line_number(code, match.start()), "builtin method from_vec")
        for match in array_from_vec.finditer(code)
    )

    return sorted(set(findings))


def scan_public_python_surface(text: str) -> list[tuple[int, str]]:
    """Find noncanonical S1 syntax or transition promises in public text."""

    findings: list[tuple[int, str]] = []
    type_pattern = "|".join(map(re.escape, NONCANONICAL_COLLECTION_TYPES[:3]))
    method_pattern = "|".join(map(re.escape, NONCANONICAL_COLLECTION_METHODS))
    patterns = (
        (
            re.compile(rf"(?<![.\w])(?:{type_pattern})\s*\["),
            "noncanonical collection type",
        ),
        (
            re.compile(
                rf"(?<![.\w]){re.escape(NONCANONICAL_COLLECTION_TYPES[2])}\s*\{{"
            ),
            "noncanonical set literal",
        ),
        (
            re.compile(rf"\.(?:{method_pattern})\s*\("),
            "noncanonical collection method",
        ),
        (
            re.compile(
                rf"(?i:\b(?:retired|legacy|deprecated|former|old)\b)"
                rf"[^.\n]{{0,100}}"
                rf"\b(?:{type_pattern}|{method_pattern})\b"
            ),
            "noncanonical-surface guidance",
        ),
        (
            re.compile(
                rf"\b(?:{type_pattern}|{method_pattern})\b[^.\n]{{0,100}}"
                r"(?i:\b(?:was renamed|write instead|replacement|fix-it|"
                r"compatibility shim|compatibility alias|deprecated|retired)\b)"
            ),
            "noncanonical-surface guidance",
        ),
        (
            re.compile(
                rf"(?i:\b(?:alias|shim|allowlist|exception)\b)"
                rf"[^.\n]{{0,100}}"
                rf"\b(?:{type_pattern}|{method_pattern})\b"
            ),
            "noncanonical-surface compatibility",
        ),
    )
    for pattern, label in patterns:
        findings.extend(
            (_line_number(text, match.start()), label) for match in pattern.finditer(text)
        )

    string_spelling = re.escape(NONCANONICAL_COLLECTION_TYPES[3])
    string_patterns = (
        re.compile(rf"(?<![.\w]){string_spelling}\s*(?:\[|\()"),
        re.compile(rf"(?:->|:)\s*(?:own\s+|mut\s+)?`?{string_spelling}\b"),
        re.compile(rf"[\[,]\s*{string_spelling}\s*(?=[,\]])"),
        re.compile(rf"`{string_spelling}\.[A-Za-z_]\w*"),
        re.compile(rf"`{string_spelling}`(?i:\s+(?:type|for owned values?)\b)"),
        re.compile(rf"\b{string_spelling}(?i:\s+type\b)"),
    )
    for pattern in string_patterns:
        findings.extend(
            (_line_number(text, match.start()), "noncanonical string type")
            for match in pattern.finditer(text)
        )
    return sorted(set(findings))


def _aura_document_fences(text: str) -> list[tuple[int, str]]:
    fences: list[tuple[int, str]] = []
    pattern = re.compile(r"(?ms)^```(?:aura|au|python)\s*\n(.*?)^```\s*$")
    for match in pattern.finditer(text):
        fences.append((_line_number(text, match.start(1)), match.group(1)))
    return fences


def _mask_document_fences(text: str) -> str:
    masked = list(text)
    for match in re.finditer(r"(?ms)^```[^\n]*\n.*?^```\s*$", text):
        for index in range(match.start(), match.end()):
            if masked[index] != "\n":
                masked[index] = " "
    return "".join(masked)


def scan_clean_slate_narrative(text: str) -> list[tuple[int, str]]:
    """Find maintained prose or machinery that encodes source transitions."""

    patterns = (
        re.compile(
            r"(?i)\b(?:retired|legacy|former|old)[_ -]+"
            r"(?:Aura[_ -]+)?(?:syntax|spellings?|keyword|surface|forms?|fix)\b"
        ),
        re.compile(
            r"(?i)\b(?:syntax|spellings?|keyword|surface|forms?)[_ -]+"
            r"(?:is[_ -]+|are[_ -]+)?(?:retired|legacy|former|old)\b"
        ),
        re.compile(
            r"(?i)\b(?:retired|legacy|former|old)[^\n.]{0,120}"
            r"\b(?:remains? supported|compatibility|fix-it|replacement diagnostic)\b"
        ),
        re.compile(
            r"(?i)\b(?:fix-it|replacement diagnostic)[^\n.]{0,120}"
            r"\b(?:retired|legacy|former|old|renamed)\b"
        ),
        re.compile(
            r"(?i)`borrow(?: mut)?`[^\n.]{0,100}"
            r"\b(?:reserved|unsupported|supported|invalid|replace|write)\b"
        ),
    )
    findings: list[tuple[int, str]] = []
    for pattern in patterns:
        findings.extend(
            (_line_number(text, match.start()), "noncanonical-syntax narrative")
            for match in pattern.finditer(text)
        )
    return sorted(set(findings))


def _json_string_variant_declaration_spans(
    text: str, relative: str | None
) -> list[tuple[int, int]]:
    """Locate the canonical json.Value.String(str) builtin declaration."""

    production = relative == "crates/aura-compiler/src/builtin_modules.rs"
    contract_test = relative == "crates/aura-compiler/src/builtin_modules_tests.rs"
    runtime_test = relative == "crates/aura-compiler/src/mir_runtime_tests.rs"
    if not production and not contract_test and not runtime_test:
        return []
    function_name = (
        r"json_value_enum_info\(\) -> EnumInfo"
        if production
        else (
            r"json_namespace_exposes_dynamic_tree_contract\(\)"
            if contract_test
            else r"mir_json_variant_construction_moves_owned_payload_allocations\(\)"
        )
    )
    function = re.search(rf"(?m)^fn {function_name} \{{", text)
    if function is None:
        return []
    next_function = re.search(r"(?m)^(?:#\[test\]\s*\n)?fn ", text[function.end() :])
    function_end = (
        len(text)
        if next_function is None
        else function.end() + next_function.start()
    )
    if production:
        declaration_pattern = (
            r"\(\s*\"String\"\s*,\s*positional\(\s*type_ref\(\s*\"str\"\s*,"
            r"\s*Vec::new\(\)\s*\)\s*\)\s*,\s*false\s*\)"
        )
    elif contract_test:
        declaration_pattern = (
            r"\(\s*\"String\"\s*,\s*vec!\[Type::named\(\"str\"\)\]\s*\)"
        )
    else:
        declaration_pattern = (
            r"\(\s*\"text\"\s*,\s*\"String\"\s*,\s*Type::named\(\"str\"\)\s*,"
            r"\s*Value::String\(text\)\s*\)"
        )
    declaration = re.compile(declaration_pattern)
    return [
        (match.start(), match.end())
        for match in declaration.finditer(text, function.start(), function_end)
    ]


def scan_transition_alias_pairs(
    text: str, relative: str | None = None
) -> list[tuple[int, str]]:
    """Find code or data that pairs two public spellings as accepted names."""

    findings: list[tuple[int, str]] = []
    json_variant_spans = _json_string_variant_declaration_spans(text, relative)
    pairs = zip(
        (*NONCANONICAL_COLLECTION_TYPES, *NONCANONICAL_COLLECTION_METHODS),
        (*CANONICAL_COLLECTION_TYPES, *CANONICAL_COLLECTION_METHODS),
    )
    for noncanonical, canonical in pairs:
        quoted_noncanonical = rf"[\"']{re.escape(noncanonical)}[\"']"
        quoted_canonical = rf"[\"']{re.escape(canonical)}[\"']"
        pattern = re.compile(
            rf"(?:{quoted_noncanonical}[^\n]{{0,200}}{quoted_canonical}|"
            rf"{quoted_canonical}[^\n]{{0,200}}{quoted_noncanonical})"
        )
        for match in pattern.finditer(text):
            if noncanonical == NONCANONICAL_COLLECTION_TYPES[3] and any(
                start <= match.start() and match.end() <= end
                for start, end in json_variant_spans
            ):
                continue
            findings.append(
                (_line_number(text, match.start()), "public-spelling alias pair")
            )
    return sorted(set(findings))


def is_python_surface_history(path: Path) -> bool:
    relative = path.relative_to(ROOT).as_posix()
    if relative.startswith("work/"):
        return True
    if relative.startswith("architecture_docs/decisions/"):
        return relative != "architecture_docs/decisions/README.md"
    return relative in {
        f"docs/{OLD_LOWER}_language_proposal.md",
        f"docs/{OLD_LOWER}_language_proposal.html",
    }


def _current_changelog_text(text: str) -> str:
    release = re.search(r"(?m)^## \[?0\.2\.0\b", text)
    return text if release is None else text[: release.start()]


def _quoted_body(text: str, start: int, quote: str) -> tuple[str, int]:
    """Return an escaped quoted body and the first offset after its close."""

    index = start + 1
    while index < len(text):
        if text[index] == "\\":
            index += 2
            continue
        if text[index] == quote:
            return text[start + 1 : index], index + 1
        index += 1
    return text[start + 1 :], len(text)


def _skip_block_comment(text: str, start: int) -> int:
    """Skip a possibly nested Rust-style block comment."""

    index = start + 2
    depth = 1
    while index < len(text) and depth:
        if text.startswith("/*", index):
            depth += 1
            index += 2
        elif text.startswith("*/", index):
            depth -= 1
            index += 2
        else:
            index += 1
    return index


def _embedded_quoted_fragments(
    text: str, suffix: str = ".rs"
) -> list[tuple[int, str]]:
    """Extract string literals without treating Rust apostrophes as strings."""

    fragments: list[tuple[int, str]] = []
    rust = suffix == ".rs"
    javascript = suffix in {".js", ".mjs", ".cjs", ".ts"}
    rust_raw_string = re.compile(r"(?:br|rb|r)(#{0,8})\"")
    index = 0
    while index < len(text):
        if text.startswith("//", index):
            newline = text.find("\n", index + 2)
            index = len(text) if newline < 0 else newline + 1
            continue
        if text.startswith("/*", index):
            index = _skip_block_comment(text, index)
            continue

        if rust:
            raw = rust_raw_string.match(text, index)
            if raw is not None and (
                index == 0 or not (text[index - 1].isalnum() or text[index - 1] == "_")
            ):
                hashes = raw.group(1)
                body_start = raw.end()
                closing = '"' + hashes
                body_end = text.find(closing, body_start)
                if body_end < 0:
                    fragments.append((_line_number(text, index), text[body_start:]))
                    break
                fragments.append(
                    (_line_number(text, index), text[body_start:body_end])
                )
                index = body_end + len(closing)
                continue
            if text[index] == "'":
                # Rust has character literals and lifetime apostrophes, never
                # single-quoted strings. Skip a complete character literal so
                # a character such as '"' cannot open a false string.
                if index + 1 < len(text) and text[index + 1] == "\\":
                    end = index + 3
                    while end < len(text) and text[end] != "'":
                        end += 1
                    index = min(end + 1, len(text))
                elif index + 2 < len(text) and text[index + 2] == "'":
                    index += 3
                else:
                    index += 1
                    while index < len(text) and (
                        text[index].isalnum() or text[index] == "_"
                    ):
                        index += 1
                continue
            if text[index] == '"':
                body, end = _quoted_body(text, index, '"')
                fragments.append((_line_number(text, index), body))
                index = end
                continue

        if javascript and text[index] in {'"', "'", "`"}:
            body, end = _quoted_body(text, index, text[index])
            fragments.append((_line_number(text, index), body))
            index = end
            continue

        index += 1
    return fragments


def _looks_like_aura_source_fragment(fragment: str) -> bool:
    if "\n" in fragment:
        return True
    stripped = fragment.lstrip()
    if re.match(
        r"(?:public\s+)?(?:def|class|enum|trait|impl|import|from|for|match|"
        r"return|mut|own|assert|with|spawn)\b",
        stripped,
    ):
        return True
    if re.match(r"[A-Za-z_]\w*\s*:\s*", stripped):
        return True
    # A standalone host string such as a private runtime type tag is not a
    # source unit. Embedded tests are recognized by source-shaped statements,
    # declarations, annotations, or preserved line structure above.
    return False


def _is_private_runtime_collection_tag(
    relative: str, text: str, base_line: int, fragment: str
) -> bool:
    """Recognize the one private ABI type tag that is not Aura source."""

    if relative != "crates/aura-compiler/tests/native_runtime_ffi.rs":
        return False
    if fragment != "Vec[{runtime_type}]":
        return False
    lines = text.splitlines()
    return (
        0 < base_line <= len(lines)
        and lines[base_line - 1].strip()
        == 'let vector_type = format!("Vec[{runtime_type}]");'
    )


def _is_public_document(relative: str) -> bool:
    if relative in {
        "README.md",
        "SECURITY.md",
        "SUPPORTED_PLATFORMS.md",
        "CHANGELOG.md",
        "scripts/reference-integrity.json",
    }:
        return True
    return relative.startswith(
        ("docs/", "tutorials/", "examples/", "llms/", "release-notes/")
    ) and Path(relative).suffix.lower() in {".md", ".txt", ".json"}


def _dedicated_noncanonical_surface_path(relative: str) -> bool:
    lowered = relative.lower()
    path = Path(lowered)
    stem = path.stem
    if relative.startswith("examples/collections/") and re.search(
        r"(?:^|_)(?:vec|map)(?:_|$)", stem
    ):
        return True
    if relative.startswith("crates/aura-compiler/tests/fixtures/"):
        if re.search(r"(?:^|_)(?:vec|vector)(?:_|$)", stem):
            return True
        if re.search(r"(?:^|_)map(?:_|$)", stem) and stem not in {
            "array_map_callback_trap",
            "array_map_output_dtype",
            "array_map_requires_repeatable_callback",
            "array_map_requires_shared_callback",
            "list_map_callback_requires_shared",
            "list_map_set_utilities",
        }:
            return True
        if stem == "prefix_" + "borrow_mut_param_not_supported":
            return True
    history_words = r"(?:retired|legacy|deprecated|old[-_]?syntax|old[-_]?spelling)"
    surface_words = "|".join(
        re.escape(word.lower())
        for word in (
            *NONCANONICAL_COLLECTION_TYPES,
            *NONCANONICAL_COLLECTION_METHODS,
        )
    )
    surface_specific = bool(
        re.search(rf"{history_words}[^/]*(?:{surface_words})", lowered)
        or re.search(rf"(?:{surface_words})[^/]*{history_words}", lowered)
    )
    syntax_specific = bool(
        re.search(
            r"(?:retired|legacy|deprecated|old[-_]?)"
            r"[^/]*(?:syntax|spelling|keyword)",
            lowered,
        )
        or re.search(
            r"(?:syntax|spelling|keyword)[^/]*"
            r"(?:retired|legacy|deprecated|old)",
            lowered,
        )
    )
    return surface_specific or syntax_specific


def _contains_embedded_aura_test_sources(relative: str) -> bool:
    """Identify host-language files whose literals contain Aura test programs."""

    path = Path(relative)
    return (
        "/test" in relative
        or path.name.endswith("_tests.rs")
        or relative.startswith("test_edge/")
        or relative.startswith("scripts/")
    )


def tracked_paths() -> list[Path]:
    output = subprocess.run(
        ["git", "ls-files", "-z"],
        cwd=ROOT,
        check=True,
        stdout=subprocess.PIPE,
    ).stdout
    paths = [ROOT / item.decode() for item in output.split(b"\0") if item]
    return [path for path in paths if path.exists()]


def is_history(path: Path) -> bool:
    relative = path.relative_to(ROOT).as_posix()
    if relative == "CHANGELOG.md" or relative.startswith("work/"):
        return True
    if relative.startswith("architecture_docs/decisions/"):
        return relative != "architecture_docs/decisions/README.md"
    return relative in {
        f"docs/{OLD_LOWER}_language_proposal.md",
        f"docs/{OLD_LOWER}_language_proposal.html",
    }


def readable_text(path: Path) -> str | None:
    try:
        return path.read_text(encoding="utf-8")
    except (UnicodeDecodeError, IsADirectoryError):
        return None


class AuraIdentityTests(unittest.TestCase):
    def test_maintained_tree_uses_one_product_identity(self) -> None:
        stale: list[str] = []
        for path in tracked_paths():
            if is_history(path):
                continue
            relative = path.relative_to(ROOT).as_posix()
            if OLD_LOWER in relative.lower():
                stale.append(f"path: {relative}")
            text = readable_text(path)
            if text is None:
                continue
            for number, line in enumerate(text.splitlines(), start=1):
                if (
                    relative == "architecture_docs/decisions/README.md"
                    and line.strip()
                    == "Pre-ADR-0042 documents use the former working name " + OLD + "."
                ):
                    continue
                if re.search(OLD_LOWER, line, re.IGNORECASE):
                    stale.append(f"{relative}:{number}: {line.strip()}")
        self.assertEqual(stale, [], "stale product identity:\n" + "\n".join(stale))

    def test_old_technical_contracts_are_absent(self) -> None:
        old_contracts = (
            OLD.upper() + "_",
            OLD_LOWER + "_direct_",
            OLD_LOWER + "-compiler",
            OLD_LOWER + "_compiler",
            OLD + ".toml",
            OLD + ".lock",
            "github.com/johnolafenwa/" + OLD,
            "source." + OLD_LOWER,
            OLD.encode().hex(),
            ", ".join(str(byte) for byte in OLD.encode()),
            OLD_LOWER.encode().hex(),
            ", ".join(str(byte) for byte in OLD_LOWER.encode()),
        )
        stale: list[str] = []
        for path in tracked_paths():
            if is_history(path):
                continue
            text = readable_text(path)
            if text is None:
                continue
            relative = path.relative_to(ROOT)
            for contract in old_contracts:
                if contract in text:
                    stale.append(f"{relative}: {contract}")
        for contract in old_contracts:
            with self.subTest(contract=contract):
                self.assertFalse(
                    any(item.endswith(f": {contract}") for item in stale),
                    "stale technical contracts:\n" + "\n".join(stale),
                )

    def test_old_identity_paths_are_absent_outside_history(self) -> None:
        stale = []
        for path in tracked_paths():
            if is_history(path):
                continue
            relative = path.relative_to(ROOT).as_posix()
            if OLD_LOWER in relative.lower():
                stale.append(relative)
            if path.name in {OLD + ".toml", OLD + ".lock"}:
                stale.append(relative)
        self.assertEqual(stale, [])

    def test_current_public_docs_do_not_narrate_removed_or_unimplemented_features(self) -> None:
        roots = (
            ROOT / "README.md",
            ROOT / "SECURITY.md",
            ROOT / "SUPPORTED_PLATFORMS.md",
            ROOT / "architecture_docs",
            ROOT / "crates",
            ROOT / "docs",
            ROOT / "examples",
            ROOT / "tutorials",
            ROOT / "tools",
        )
        patterns = (
            re.compile(r"\blegacy\b", re.IGNORECASE),
            re.compile(r"\bretired\b", re.IGNORECASE),
            re.compile(r"\bformerly\b", re.IGNORECASE),
            re.compile(r"\bsuperseded\b", re.IGNORECASE),
            re.compile(r"\bhistorical(?:ly)?\b", re.IGNORECASE),
            re.compile(r"\bfuture work\b", re.IGNORECASE),
            re.compile(r"\bAura\s+0\.4\b", re.IGNORECASE),
            re.compile(r"\b(?:borrow|return)[- ]source\b", re.IGNORECASE),
            re.compile(r"\breturn[- ]label\b", re.IGNORECASE),
            re.compile(r"\blifetime[- ]label\b", re.IGNORECASE),
            re.compile(r"\bfirst-class (?:loan|view)", re.IGNORECASE),
            re.compile(r"\bloan/view\b", re.IGNORECASE),
        )
        files: set[Path] = set()
        for root in roots:
            if root.is_file():
                files.add(root)
            elif root.exists():
                files.update(root.rglob("*.md"))
        stale: list[str] = []
        for path in sorted(files):
            if is_history(path):
                continue
            for number, line in enumerate(path.read_text().splitlines(), start=1):
                if any(pattern.search(line) for pattern in patterns):
                    stale.append(
                        f"{path.relative_to(ROOT)}:{number}: {line.strip()}"
                    )
        self.assertEqual(
            stale,
            [],
            "stale feature-history narrative:\n" + "\n".join(stale),
        )

    def test_public_measurements_use_plain_factual_voice(self) -> None:
        roots = (
            ROOT / "README.md",
            ROOT / "CHANGELOG.md",
            ROOT / "docs",
            ROOT / "tutorials",
            ROOT / "examples",
            ROOT / "benchmarks",
            ROOT / "llms",
            ROOT / "marketplace",
            ROOT / "release-notes",
        )
        patterns = (
            re.compile(r"\bmeasured snapshot,\s+not\b", re.IGNORECASE),
            re.compile(
                r"\b(?:observations?|measurements?|results?|numbers?)\b"
                r"[^.]{0,180}\bnot (?:an? )?(?:portable|general|broad)\b",
                re.IGNORECASE,
            ),
            re.compile(
                r"\bnot (?:an? )?(?:portable|general|broad)\s+"
                r"(?:performance|speed|capacity|benchmark)\b",
                re.IGNORECASE,
            ),
            re.compile(
                r"\bnot (?:an? )?(?:CI\s+)?(?:performance|benchmark|release)\s+"
                r"(?:claim|promise|gate)\b",
                re.IGNORECASE,
            ),
            re.compile(r"\bno (?:floating-kernel )?vectorization claim\b", re.IGNORECASE),
            re.compile(
                r"\bmakes? no (?:portable )?"
                r"(?:performance|benchmark|vectorization)[^\n.]{0,40}\bclaim\b",
                re.IGNORECASE,
            ),
            re.compile(r"\bmust not be presented as\s+measurements?\b", re.IGNORECASE),
            re.compile(r"\bshould not be treated as\b", re.IGNORECASE),
            re.compile(r"\bmust not be presented as\b", re.IGNORECASE),
            re.compile(
                r"\bnot an? (?:portable claim|benchmark promise)\b",
                re.IGNORECASE,
            ),
            re.compile(
                r"\b(?:observations?|measurements?|results?|numbers?)\b"
                r"[^.]{0,180}\bnot an? guarantee\b",
                re.IGNORECASE,
            ),
            re.compile(
                r"\bwithout (?:becoming|turning it into) an? "
                r"(?:product|performance|marketing) claim\b",
                re.IGNORECASE,
            ),
            re.compile(r"\bnot representative of every application\b", re.IGNORECASE),
            re.compile(r"\bnot a robust capacity guarantee\b", re.IGNORECASE),
            re.compile(r"\b(?:do|does) not imply that\s+all integer work\b", re.IGNORECASE),
            re.compile(r"\bnot a claim of NumPy\b", re.IGNORECASE),
            re.compile(r"\bnot a stable\s+contract\b", re.IGNORECASE),
            re.compile(
                r"\b(?:does not|do not) (?:support|maintain)\s+"
                r"(?:an? )?[^.]{0,60}\bclaim\b",
                re.IGNORECASE,
            ),
            re.compile(
                r"\bmakes? no\s+[^.]{0,60}\b(?:claim|promise)\b",
                re.IGNORECASE,
            ),
            re.compile(
                r"\b(?:not claiming|does not claim)\s+"
                r"(?:feature parity|production stability|GPU programming)",
                re.IGNORECASE,
            ),
            re.compile(r"\bWhat Aura Does Not Claim Yet\b", re.IGNORECASE),
            re.compile(r"\bThose wider claims require separate evidence\b", re.IGNORECASE),
        )
        files: set[Path] = set()
        for root in roots:
            if root.is_file():
                files.add(root)
            elif root.exists():
                files.update(root.rglob("*.md"))
                files.update(root.rglob("*.txt"))
        files.discard(ROOT / f"docs/{OLD_LOWER}_language_proposal.md")
        stale: list[str] = []
        for path in sorted(files):
            text = path.read_text(encoding="utf-8")
            seen: set[tuple[int, str]] = set()
            for pattern in patterns:
                for match in pattern.finditer(text):
                    number = text.count("\n", 0, match.start()) + 1
                    line = text.splitlines()[number - 1].strip()
                    seen.add((number, line))
            stale.extend(
                f"{path.relative_to(ROOT)}:{number}: {line}"
                for number, line in sorted(seen)
            )
        self.assertEqual(
            stale,
            [],
            "defensive measurement disclaimers:\n" + "\n".join(stale),
        )

    def test_python_surface_scanner_rejects_noncanonical_builtins(self) -> None:
        source = "\n".join(
            (
                "def sample(values: Vec[int64], label: String):",
                "    flags = Set{1, 2}",
                "    values.push(3)",
                "    values.sort_by(key = identity)",
                "    table.contains_key(label)",
                "    table.entries()",
                "    array = Array[int64].from_vec(values, [1])",
            )
        )
        labels = {label for _, label in scan_aura_collection_surface(source)}
        self.assertEqual(
            labels,
            {
                "builtin type Vec",
                "builtin type String",
                "builtin set literal",
                "builtin method push",
                "builtin method sort_by",
                "builtin method contains_key",
                "builtin method entries",
                "builtin method from_vec",
            },
        )

    def test_python_surface_scanner_allows_user_symbols_and_borrow_terms(self) -> None:
        source = """# Vec[int64] and values.push(1) are inert examples in comments.
enum Flag:
    Set(bool)

class Vec[T]:
    value: T

class Stack:
    def push(mut self, value: int64):
        pass

    def fill(mut self):
        self.push(1)

def inspect(stack: Stack, item: Vec[int64], alias: int64):
    stack.push(alias)
    flag = Flag.Set(true)
    String = Stack()
    text = "Map[str, int64] and table.entries()"
"""
        self.assertEqual(scan_aura_collection_surface(source), [])
        conceptual_prose = (
            "A shared borrow permits read-only aliasing. Mutable aliases conflict "
            "with simultaneous shared access."
        )
        self.assertEqual(scan_public_python_surface(conceptual_prose), [])
        self.assertEqual(scan_clean_slate_narrative(conceptual_prose), [])
        self.assertEqual(
            scan_clean_slate_narrative("legacy = 1\nborrow = legacy\n"), []
        )
        self.assertEqual(
            scan_public_python_surface(
                "The enum variants are `String`, `Array`, and `Set`."
            ),
            [],
        )

    def test_embedded_fragment_scanner_respects_host_language_literals(self) -> None:
        rust = r'''// Read each child's stderr without treating the apostrophe as a string.
fn retain<'a>(value: &'a str) -> &'a str {
    let quote = '"';
    let apostrophe = '\'';
    let mut host_values = Vec::new();
    host_values.push(String::new());
    let raw_source = r#"def main():
    values: Vec[int64] = []
"#;
    let normal_source = "def inspect(value: Map[str, int64], text: String):\\n    pass\\n";
    value
}
'''
        rust_findings = [
            label
            for _, fragment in _embedded_quoted_fragments(rust, ".rs")
            if _looks_like_aura_source_fragment(fragment)
            for _, label in scan_aura_collection_surface(fragment)
        ]
        self.assertEqual(
            set(rust_findings),
            {"builtin type Vec", "builtin type Map", "builtin type String"},
        )
        self.assertNotIn("builtin method push", rust_findings)

        javascript = r'''// A caller's host comment must not open a string.
const source = 'def main():\n    values.push(1)\n';
const template = `def check(values: Vec[int64]):
    pass
`;
'''
        javascript_findings = [
            label
            for _, fragment in _embedded_quoted_fragments(javascript, ".js")
            if _looks_like_aura_source_fragment(fragment)
            for _, label in scan_aura_collection_surface(fragment)
        ]
        self.assertEqual(
            set(javascript_findings),
            {"builtin method push", "builtin type Vec"},
        )
        self.assertFalse(_looks_like_aura_source_fragment("Vec[int64]"))

        runtime_tag = 'let vector_type = format!("Vec[{runtime_type}]");\n'
        runtime_path = "crates/aura-compiler/tests/native_runtime_ffi.rs"
        self.assertTrue(
            _is_private_runtime_collection_tag(
                runtime_path, runtime_tag, 1, "Vec[{runtime_type}]"
            )
        )
        self.assertFalse(
            _is_private_runtime_collection_tag(
                "crates/aura-compiler/tests/other.rs",
                runtime_tag,
                1,
                "Vec[{runtime_type}]",
            )
        )
        self.assertTrue(
            _contains_embedded_aura_test_sources(
                "crates/aura-compiler/src/analysis_tests.rs"
            )
        )
        self.assertFalse(
            _contains_embedded_aura_test_sources(
                "crates/aura-compiler/src/analysis.rs"
            )
        )
        self.assertFalse(
            _is_private_runtime_collection_tag(
                runtime_path,
                'let source = "Vec[{runtime_type}]";\n',
                1,
                "Vec[{runtime_type}]",
            )
        )

    def test_python_surface_scanner_rejects_compatibility_guidance(self) -> None:
        type_name = NONCANONICAL_COLLECTION_TYPES[0]
        method_name = NONCANONICAL_COLLECTION_METHODS[0]
        samples = (
            "The " + "ret" + f"ired {type_name} spelling receives a fix-it.",
            f"A compatibility alias accepts {type_name} during migration.",
            "The " + "leg" + f"acy {method_name} method remains supported.",
        )
        for sample in samples:
            with self.subTest(sample=sample):
                self.assertTrue(scan_public_python_surface(sample))
        narratives = (
            "The " + "ret" + "ired spelling remains supported by a fix-it.",
            "The " + "old" + " syntax is no longer " + "accepted.",
            "`bor" + "row` is reserved; write the canonical form.",
        )
        for sample in narratives:
            with self.subTest(narrative=sample):
                self.assertTrue(scan_clean_slate_narrative(sample))
        alias_source = (
            'match "'
            + type_name
            + '" | "'
            + CANONICAL_COLLECTION_TYPES[0]
            + '" => true'
        )
        self.assertTrue(scan_transition_alias_pairs(alias_source))
        method_alias_source = (
            '"contains" | "contains_'
            + 'key" => true'
        )
        self.assertEqual(
            scan_transition_alias_pairs(method_alias_source),
            [(1, "public-spelling alias pair")],
        )
        self.assertEqual(
            scan_transition_alias_pairs('Type::Vec => "list"'), []
        )
        variant_name = NONCANONICAL_COLLECTION_TYPES[3]
        payload_name = CANONICAL_COLLECTION_TYPES[3]
        variant_declaration = f'''fn json_value_enum_info() -> EnumInfo {{
    builtin_enum_info(
        "json",
        "Value",
        vec![("{variant_name}", positional(type_ref("{payload_name}", Vec::new())), false)],
    )
}}
'''
        builtin_modules = "crates/aura-compiler/src/builtin_modules.rs"
        self.assertEqual(
            scan_transition_alias_pairs(variant_declaration, builtin_modules), []
        )
        self.assertTrue(scan_transition_alias_pairs(variant_declaration))
        string_alias = (
            'match name { "'
            + variant_name
            + '" | "'
            + payload_name
            + '" => true }'
        )
        self.assertTrue(
            scan_transition_alias_pairs(string_alias, builtin_modules)
        )
        variant_contract = (
            "fn json_namespace_exposes_dynamic_tree_contract() {\n"
            + '    let value_payloads = [("'
            + variant_name
            + '", vec![Type::named("'
            + payload_name
            + '")])];\n}\n'
        )
        self.assertEqual(
            scan_transition_alias_pairs(
                variant_contract,
                "crates/aura-compiler/src/builtin_modules_tests.rs",
            ),
            [],
        )
        qualified_variant_use = (
            "def main():\n    value = json.Value.String(\"text\")\n"
        )
        self.assertEqual(
            scan_aura_collection_surface(qualified_variant_use), []
        )

    def test_maintained_python_surface_is_clean_slate(self) -> None:
        stale: list[str] = []
        embedded_suffixes = {".rs", ".js", ".mjs", ".cjs", ".ts"}
        diagnostic_suffixes = {".diag", ".stderr", ".stdout"}

        for path in tracked_paths():
            if is_python_surface_history(path):
                continue
            relative = path.relative_to(ROOT).as_posix()
            if relative == "personal/file_ops.au":
                continue
            if _dedicated_noncanonical_surface_path(relative):
                stale.append(f"path: {relative}: dedicated noncanonical surface")

            text = readable_text(path)
            if text is None:
                continue
            inspected_text = (
                _current_changelog_text(text)
                if relative == "CHANGELOG.md"
                else text
            )

            if path.suffix == ".au":
                findings = scan_aura_collection_surface(text)
            elif _is_public_document(relative):
                findings = scan_public_python_surface(
                    _mask_document_fences(inspected_text)
                )
            elif path.suffix in diagnostic_suffixes:
                findings = scan_public_python_surface(inspected_text)
            else:
                findings = []

            findings.extend(scan_clean_slate_narrative(inspected_text))
            findings.extend(scan_transition_alias_pairs(inspected_text, relative))

            if _is_public_document(relative):
                for base_line, fence in _aura_document_fences(inspected_text):
                    findings.extend(
                        (base_line + number - 1, f"fenced {label}")
                        for number, label in scan_aura_collection_surface(fence)
                    )

            if path.suffix in diagnostic_suffixes:
                string_name = re.escape(NONCANONICAL_COLLECTION_TYPES[3])
                for match in re.finditer(rf"(?<![.\w]){string_name}\b", inspected_text):
                    line_start = inspected_text.rfind("\n", 0, match.start()) + 1
                    line_end = inspected_text.find("\n", match.end())
                    if line_end < 0:
                        line_end = len(inspected_text)
                    line = inspected_text[line_start:line_end]
                    if re.search(
                        rf"(?i)\b(?:variant|case)\s+`?{string_name}\b", line
                    ):
                        continue
                    findings.append(
                        (_line_number(inspected_text, match.start()),
                         "noncanonical string type")
                    )

            stale.extend(
                f"{relative}:{number}: {label}" for number, label in findings
            )

            if path.suffix in embedded_suffixes and _contains_embedded_aura_test_sources(
                relative
            ):
                for base_line, fragment in _embedded_quoted_fragments(
                    text, path.suffix
                ):
                    if _is_private_runtime_collection_tag(
                        relative, text, base_line, fragment
                    ):
                        continue
                    if _looks_like_aura_source_fragment(fragment):
                        for number, label in scan_aura_collection_surface(fragment):
                            stale.append(
                                f"{relative}:{base_line + number - 1}: "
                                f"embedded {label}"
                            )
                    for number, label in scan_public_python_surface(fragment):
                        if label.endswith("guidance") or label.endswith("compatibility"):
                            stale.append(
                                f"{relative}:{base_line + number - 1}: embedded {label}"
                            )

            # Production editor rules and completions must contain only the
            # canonical public names. Test files are handled as embedded Aura
            # sources above, so absence assertions remain expressible.
            if relative.startswith(("tools/vscode-aura/", "tools/aura-language-server/")):
                if "/test" not in relative and ".test." not in relative:
                    for spelling in (
                        *NONCANONICAL_COLLECTION_TYPES,
                        *NONCANONICAL_COLLECTION_METHODS,
                    ):
                        quoted = re.compile(
                            rf"[\"']{re.escape(spelling)}[\"']"
                        )
                        for match in quoted.finditer(text):
                            stale.append(
                                f"{relative}:{_line_number(text, match.start())}: "
                                f"deprecated editor rule {spelling}"
                            )

            # Compatibility tables and allowlists are forbidden only when
            # they name this noncanonical collection surface. Ordinary type
            # compatibility and ownership alias discussions remain valid.
            surface_words = "|".join(
                re.escape(word)
                for word in (
                    *NONCANONICAL_COLLECTION_TYPES,
                    *NONCANONICAL_COLLECTION_METHODS,
                )
            )
            machinery = re.compile(
                rf"(?i:(?:retired[_ -]?spellings?|legacy[_ -]?syntax|"
                rf"deprecated[_ -]?(?:builtin|syntax|spelling)|"
                rf"compat(?:ibility)?[_ -]?(?:alias|shim)|migration[_ -]?(?:map|table)|"
                rf"old[_ -]?syntax[_ -]?(?:fixture|allowlist)))"
                rf"[^\n]{{0,160}}(?:{surface_words})"
            )
            for match in machinery.finditer(inspected_text):
                stale.append(
                    f"{relative}:{_line_number(inspected_text, match.start())}: "
                    "noncanonical-surface compatibility machinery"
                )

        self.assertEqual(
            stale,
            [],
            "non-canonical Python surface:\n" + "\n".join(sorted(set(stale))),
        )

    def test_required_aura_identity_surfaces_exist(self) -> None:
        required = (
            ROOT / "crates/aura-compiler/Cargo.toml",
            ROOT / "tools/aura-language-server/package.json",
            ROOT / "tools/vscode-aura/package.json",
            ROOT / "docs/public/aura-mark.svg",
        )
        self.assertEqual([path for path in required if not path.exists()], [])
        self.assertTrue(any(path.name == "Aura.toml" for path in tracked_paths()))


if __name__ == "__main__":
    unittest.main()
