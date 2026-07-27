#!/usr/bin/env python3
"""ADR-0022 syntax and semantic-evidence inventory.

This tool deliberately separates evidence into three levels:

* syntax-aware evidence from Aurora AST JSON and token-aware source scans;
* deterministic compiler-source evidence for builtin declarations, rendered
  signatures, and passing metadata;
* explicit review queues where AST JSON does not contain enough checked type
  information to make a semantic claim.

The distinction matters. An unresolved imported type is not silently treated
as non-copy, an unparsed fixture is not hidden behind a sample count, and a
Rust string that contains the English word "borrow" is not called Aurora
syntax. Every record is sorted and carries a path and source location so the
inventory is a reviewable ledger rather than a collection of totals.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import re
import subprocess
import sys
from collections import Counter
from pathlib import Path, PurePosixPath
from typing import Iterable, Iterator


ROOT = Path(
    subprocess.run(
        ["git", "rev-parse", "--show-toplevel"],
        capture_output=True,
        text=True,
        check=True,
    ).stdout.strip()
)
AURA = ROOT / "target/debug/aura"

COPY = "copy"
MOVE = "move"
UNRESOLVED = "unresolved"

CONCRETE = "concrete"
GENERIC = "generic"

BUILTIN_COPY_SCALARS = {
    "None",
    "bool",
    "int",
    "int8",
    "int16",
    "int32",
    "int64",
    "int128",
    "intsize",
    "uint8",
    "uint16",
    "uint32",
    "uint64",
    "uint128",
    "uintsize",
    "float32",
    "float64",
    "Duration",
}
BUILTIN_COPY_HANDLES = {"Queue", "Task"}
STRUCTURAL_COPY_TYPES = {"Option", "Result", "SendError", "QueueReceive"}
BUILTIN_MOVE_TYPES = {
    "String",
    "Vec",
    "Map",
    "Set",
    "Range",
    "TaskResult",
    "WaitAny",
    "WaitAll",
    "TaskGroup",
    "random.Rng",
    "json.Value",
    "json.Error",
}
KNOWN_TYPE_CONSTRUCTORS = (
    BUILTIN_COPY_SCALARS
    | BUILTIN_COPY_HANDLES
    | STRUCTURAL_COPY_TYPES
    | BUILTIN_MOVE_TYPES
    | {"Tuple", "number"}
)

# These are the only Aurora files in which a live `borrow` token is expected:
# each exists to prove that the retired spelling is diagnosed.
RETIREMENT_FIXTURES = {
    PurePosixPath(
        "crates/aurora-compiler/tests/fixtures/check-fail/"
        "borrow_call_argument_not_supported.au"
    ),
    PurePosixPath(
        "crates/aurora-compiler/tests/fixtures/parse-fail/"
        "borrowed_return_label_in_trait_was_removed.au"
    ),
    PurePosixPath(
        "crates/aurora-compiler/tests/fixtures/parse-fail/"
        "borrowed_return_was_removed.au"
    ),
    PurePosixPath(
        "crates/aurora-compiler/tests/fixtures/parse-fail/"
        "prefix_borrow_param_not_supported.au"
    ),
}

EXPLICIT_MIGRATION_DOCUMENTS = {
    PurePosixPath("CHANGELOG.md"),
    PurePosixPath("docs/manual/grammar.md"),
    PurePosixPath("docs/manual/lexical-structure.md"),
    PurePosixPath("tutorials/14-current-language-surface.md"),
    PurePosixPath("work/2026-07-26-batch3-capability-syntax-migration.md"),
    PurePosixPath("work/2026-07-27-batch3-checkpoint.md"),
}


def _relative(path: Path | PurePosixPath | str) -> PurePosixPath:
    candidate = Path(path)
    if candidate.is_absolute():
        candidate = candidate.relative_to(ROOT)
    return PurePosixPath(candidate.as_posix())


def maintained_exclusion_reason(path: PurePosixPath) -> str | None:
    """Return the narrow, auditable reason a path is not maintained source."""
    parts = path.parts
    if "node_modules" in parts:
        return "installed dependency"
    if parts and parts[0] == "target":
        return "build output"
    if ".git" in parts:
        return "version-control metadata"
    for index in range(len(parts) - 2):
        if parts[index : index + 3] == ("docs", ".vitepress", "dist"):
            return "generated VitePress output"
    return None


def tracked(pattern: str | None = None) -> list[Path]:
    command = ["git", "ls-files"]
    if pattern is not None:
        command.extend(["--", pattern])
    listing = subprocess.run(
        command,
        cwd=ROOT,
        capture_output=True,
        text=True,
        check=True,
    ).stdout.splitlines()
    return [ROOT / name for name in sorted(listing)]


def maintained_tracked(pattern: str | None = None) -> tuple[list[Path], list[dict]]:
    included: list[Path] = []
    excluded: list[dict] = []
    for path in tracked(pattern):
        relative = _relative(path)
        reason = maintained_exclusion_reason(relative)
        if reason is None:
            included.append(path)
        else:
            excluded.append({"path": relative.as_posix(), "reason": reason})
    return included, excluded


def strip_aurora(source: str) -> str:
    """Blank Aurora comments and string bodies while preserving offsets."""
    out = list(source)
    i, length = 0, len(source)
    while i < length:
        ch = source[i]
        if ch == "#":
            while i < length and source[i] != "\n":
                out[i] = " "
                i += 1
            continue
        if ch in "\"'":
            quote = ch
            close = quote * 3 if source.startswith(quote * 3, i) else quote
            for offset in range(len(close)):
                if i + offset < length:
                    out[i + offset] = " "
            i += len(close)
            while i < length and not source.startswith(close, i):
                if source[i] == "\\":
                    out[i] = " "
                    i += 1
                    if i < length:
                        out[i] = " "
                        i += 1
                    continue
                if source[i] != "\n":
                    out[i] = " "
                i += 1
            for offset in range(len(close)):
                if i + offset < length:
                    out[i + offset] = " "
            i += len(close)
            continue
        i += 1
    return "".join(out)


def markdown_code(source: str) -> str:
    """Keep fenced and inline code, preserving source line numbers."""
    output: list[str] = []
    fenced = False
    fence_marker = ""
    for line in source.splitlines():
        stripped = line.lstrip()
        marker_match = re.match(r"(```+|~~~+)", stripped)
        if marker_match:
            marker = marker_match.group(1)
            if not fenced:
                fenced = True
                fence_marker = marker[0]
            elif marker[0] == fence_marker:
                fenced = False
                fence_marker = ""
            output.append("")
        elif fenced:
            output.append(line)
        else:
            # Preserve columns by blanking everything outside inline spans.
            kept = [" "] * len(line)
            for match in re.finditer(r"`([^`]*)`", line):
                for index in range(match.start(1), match.end(1)):
                    kept[index] = line[index]
            output.append("".join(kept))
    return "\n".join(output)


BORROW_TOKEN = re.compile(r"(?<![A-Za-z0-9_])borrow(?![A-Za-z0-9_])")


def borrow_tokens(text: str) -> int:
    return len(BORROW_TOKEN.findall(text))


def _line_column(text: str, offset: int) -> tuple[int, int]:
    line = text.count("\n", 0, offset) + 1
    line_start = text.rfind("\n", 0, offset) + 1
    return line, offset - line_start + 1


def borrow_occurrence_classification(
    path: PurePosixPath, surface: str
) -> str:
    if surface == "aurora" and path in RETIREMENT_FIXTURES:
        return "retirement_fixture"
    if path.parts[:2] == ("architecture_docs", "decisions"):
        return "historical_decision_context"
    if path in EXPLICIT_MIGRATION_DOCUMENTS:
        return "explicit_migration_documentation"
    if surface == "markdown":
        return "maintained_documentation_review"
    if surface == "aurora":
        return "maintained_aurora_source_review"
    return "supplementary_text_review"


def borrow_occurrence_records(
    path: PurePosixPath, source: str, surface: str
) -> list[dict]:
    if surface == "aurora":
        scanned = strip_aurora(source)
    elif surface == "markdown":
        scanned = markdown_code(source)
    else:
        scanned = source
    records = []
    for match in BORROW_TOKEN.finditer(scanned):
        line, column = _line_column(scanned, match.start())
        records.append(
            {
                "path": path.as_posix(),
                "line": line,
                "column": column,
                "surface": surface,
                "classification": borrow_occurrence_classification(path, surface),
            }
        )
    return records


def walk(node, key: str, hits: list) -> None:
    if isinstance(node, dict):
        if key in node:
            hits.append(node[key])
        for value in node.values():
            walk(value, key, hits)
    elif isinstance(node, list):
        for value in node:
            walk(value, key, hits)


def collect_type_declarations(tree: dict) -> dict[str, dict]:
    declarations: dict[str, dict] = {}
    for item in tree.get("items") or []:
        if not isinstance(item, dict):
            continue
        for kind in ("Class", "Enum"):
            declaration = item.get(kind)
            if isinstance(declaration, dict) and isinstance(
                declaration.get("name"), str
            ):
                declarations[declaration["name"]] = {
                    "kind": kind.lower(),
                    "declaration": declaration,
                }
    return declarations


def _classification(status: str, reason: str) -> dict[str, str]:
    return {"status": status, "reason": reason}


def _combine_structural(
    classifications: Iterable[dict[str, str]], reason: str
) -> dict[str, str]:
    values = list(classifications)
    if any(value["status"] == MOVE for value in values):
        return _classification(MOVE, reason)
    if values and all(value["status"] == COPY for value in values):
        return _classification(COPY, reason)
    if not values:
        return _classification(COPY, reason)
    return _classification(UNRESOLVED, f"{reason}; at least one component is unresolved")


def classify_type_ref(
    node,
    declarations: dict[str, dict] | None = None,
    type_params: Iterable[str] = (),
    substitutions: dict[str, dict] | None = None,
    visiting: frozenset[str] = frozenset(),
) -> dict[str, str]:
    """Conservatively mirror the compiler's declaration-known copy rules."""
    declarations = declarations or {}
    substitutions = substitutions or {}
    type_params = set(type_params)
    if not isinstance(node, dict):
        return _classification(UNRESOLVED, "missing or non-object type reference")

    elements = node.get("elements")
    if isinstance(elements, list):
        return _combine_structural(
            (
                classify_type_ref(
                    element,
                    declarations,
                    type_params,
                    substitutions,
                    visiting,
                )
                for element in elements
            ),
            "tuple copyability is structural",
        )

    name = node.get("name")
    args = node.get("args") or []
    if not isinstance(name, str) or not isinstance(args, list):
        return _classification(UNRESOLVED, "unrecognized type-reference shape")

    if name in substitutions and not args:
        replacement = substitutions[name]
        if (
            isinstance(replacement, dict)
            and replacement.get("name") == name
            and not replacement.get("args")
            and not replacement.get("elements")
        ):
            return _classification(
                UNRESOLVED,
                f"type parameter `{name}` remains unresolved after substitution",
            )
        return classify_type_ref(
            replacement,
            declarations,
            type_params,
            substitutions,
            visiting,
        )
    if name in type_params and not args:
        return _classification(
            UNRESOLVED,
            f"type parameter `{name}` is not declaration-known copy",
        )
    if name == "number" and not args:
        return _classification(COPY, "the builtin numeric family contains only copy types")
    if name in BUILTIN_COPY_SCALARS and not args:
        return _classification(COPY, f"`{name}` is a compiler-defined copy scalar")
    if name in BUILTIN_COPY_HANDLES and len(args) == 1:
        return _classification(
            COPY,
            f"`{name}[T]` is a copy handle regardless of payload copyability",
        )
    if name == "Option" and len(args) == 1:
        return _combine_structural(
            [
                classify_type_ref(
                    args[0], declarations, type_params, substitutions, visiting
                )
            ],
            "`Option[T]` follows payload copyability",
        )
    if name == "Result" and len(args) == 2:
        return _combine_structural(
            (
                classify_type_ref(
                    arg, declarations, type_params, substitutions, visiting
                )
                for arg in args
            ),
            "`Result[T, E]` follows both payload types",
        )
    if name in {"SendError", "QueueReceive"} and len(args) == 1:
        return _combine_structural(
            [
                classify_type_ref(
                    args[0], declarations, type_params, substitutions, visiting
                )
            ],
            f"`{name}[T]` follows payload copyability",
        )
    if name in BUILTIN_MOVE_TYPES:
        return _classification(MOVE, f"`{name}` is a compiler-defined move type")

    entry = declarations.get(name)
    if entry is None:
        return _classification(
            UNRESOLVED,
            f"`{name}` is imported, qualified, generic, or otherwise absent from this AST",
        )
    declaration = entry["declaration"]
    declared_params = declaration.get("type_params") or []
    if len(args) != len(declared_params):
        return _classification(
            UNRESOLVED,
            f"`{name}` has {len(declared_params)} declared type parameter(s), "
            f"but this reference supplies {len(args)}",
        )
    local_substitutions = dict(substitutions)
    local_substitutions.update(zip(declared_params, args))
    key = f"{name}[{len(args)}]"
    if key in visiting:
        return _classification(MOVE, f"recursive `{name}` is not copy-classified")
    nested_visiting = visiting | {key}

    if entry["kind"] == "class":
        if not declaration.get("copy", False):
            return _classification(MOVE, f"`{name}` is an ordinary move class")
        return _combine_structural(
            (
                classify_type_ref(
                    arg,
                    declarations,
                    type_params,
                    substitutions,
                    nested_visiting,
                )
                for arg in args
            ),
            f"`{name}` is declared as a copy class",
        )

    payload_classifications = []
    for variant in declaration.get("variants") or []:
        for payload in variant.get("payloads") or []:
            payload_classifications.append(
                classify_type_ref(
                    payload.get("ty"),
                    declarations,
                    type_params,
                    local_substitutions,
                    nested_visiting,
                )
            )
    return _combine_structural(
        payload_classifications,
        f"all declared payloads of enum `{name}` determine copyability",
    )


def is_copy_type(node) -> bool:
    """Compatibility wrapper used by older callers and work-note probes."""
    return classify_type_ref(node)["status"] == COPY


def render_type_ref(node) -> str:
    if not isinstance(node, dict):
        return "<unknown>"
    if isinstance(node.get("elements"), list):
        elements = ", ".join(render_type_ref(element) for element in node["elements"])
        if len(node["elements"]) == 1:
            elements += ","
        return f"({elements})"
    name = node.get("name")
    if not isinstance(name, str):
        return "<unknown>"
    args = node.get("args") or []
    if args:
        return f"{name}[{', '.join(render_type_ref(arg) for arg in args)}]"
    return name


def scrutinee_shape(match_node: dict) -> str:
    kind = (match_node.get("scrutinee") or {}).get("kind") or {}
    if any(name in kind for name in ("Name", "Member", "Index")):
        return "place"
    return "temporary"


def arm_moves_payload(match_node: dict) -> bool:
    """Return whether an arm binds a payload; this is syntax, not move proof."""
    hits: list = []
    walk(match_node.get("arms") or [], "bindings", hits)
    if any(hits):
        return True
    hits = []
    walk(match_node.get("arms") or [], "pattern", hits)
    for pattern in hits:
        names: list = []
        walk(pattern, "Binding", names)
        walk(pattern, "Name", names)
        if names:
            return True
    return False


def _span(record: dict | None) -> tuple[int | None, int | None]:
    span = (record or {}).get("span") or {}
    return span.get("line"), span.get("column")


def _type_names(node) -> set[str]:
    if not isinstance(node, dict):
        return set()
    names: set[str] = set()
    if isinstance(node.get("name"), str):
        names.add(node["name"])
    for element in node.get("elements") or []:
        names.update(_type_names(element))
    for arg in node.get("args") or []:
        names.update(_type_names(arg))
    return names


def _impl_concreteness(
    impl: dict,
    declarations: dict[str, dict],
    trait_type_params: dict[str, set[str]],
) -> tuple[str, str]:
    if impl.get("type_params"):
        return GENERIC, "the impl declares type parameters"
    names = _type_names(impl.get("for_type"))
    for arg in impl.get("trait_args") or []:
        names.update(_type_names(arg))
    generic_names = trait_type_params.get(impl.get("trait_name", ""), set())
    generic_hits = sorted(
        name
        for name in names
        if name in generic_names
        or (
            name not in KNOWN_TYPE_CONSTRUCTORS
            and name not in declarations
            and len(name) == 1
            and name.isupper()
        )
    )
    if generic_hits:
        return GENERIC, f"implicit generic name(s): {', '.join(generic_hits)}"
    unresolved = sorted(
        name
        for name in names
        if name not in KNOWN_TYPE_CONSTRUCTORS and name not in declarations
    )
    if unresolved:
        return (
            UNRESOLVED,
            "target or trait argument names are not declared in this AST: "
            + ", ".join(unresolved),
        )
    return CONCRETE, "all target and trait-argument names are declaration-known"


def collect_ast_evidence(tree: dict, path: PurePosixPath) -> dict[str, list]:
    declarations = collect_type_declarations(tree)
    trait_type_params = {
        item["Trait"]["name"]: set(item["Trait"].get("type_params") or [])
        for item in tree.get("items") or []
        if isinstance(item, dict) and isinstance(item.get("Trait"), dict)
    }
    evidence: dict[str, list] = {
        "parameters": [],
        "receivers": [],
        "trait_impls": [],
        "bare_matches": [],
    }

    def record_function(
        function: dict,
        owner_kind: str,
        owner_name: str,
        receiver_type: dict | None,
        container_type_params: Iterable[str],
    ) -> None:
        function_type_params = set(container_type_params)
        function_type_params.update(function.get("type_params") or [])
        for parameter in function.get("params") or []:
            classification = classify_type_ref(
                parameter.get("ty"),
                declarations,
                function_type_params,
            )
            line, column = _span(parameter)
            evidence["parameters"].append(
                {
                    "path": path.as_posix(),
                    "line": line,
                    "column": column,
                    "owner_kind": owner_kind,
                    "owner": owner_name,
                    "function": function.get("name"),
                    "parameter": parameter.get("name"),
                    "mode": parameter.get("mode"),
                    "type": render_type_ref(parameter.get("ty")),
                    "copy_classification": classification["status"],
                    "copy_reason": classification["reason"],
                }
            )
        receiver = function.get("receiver")
        if receiver is not None:
            if receiver_type is None:
                classification = _classification(
                    UNRESOLVED,
                    "trait `Self` has no declaration-known concrete type",
                )
                type_text = "Self"
            else:
                classification = classify_type_ref(
                    receiver_type,
                    declarations,
                    function_type_params,
                )
                type_text = render_type_ref(receiver_type)
            line, column = _span(function)
            evidence["receivers"].append(
                {
                    "path": path.as_posix(),
                    "line": line,
                    "column": column,
                    "owner_kind": owner_kind,
                    "owner": owner_name,
                    "function": function.get("name"),
                    "mode": receiver,
                    "type": type_text,
                    "copy_classification": classification["status"],
                    "copy_reason": classification["reason"],
                }
            )

    for item in tree.get("items") or []:
        if not isinstance(item, dict):
            continue
        if isinstance(item.get("Function"), dict):
            function = item["Function"]
            record_function(function, "function", function.get("name", ""), None, ())
        elif isinstance(item.get("Class"), dict):
            declaration = item["Class"]
            receiver_type = {
                "name": declaration.get("name"),
                "args": [
                    {"name": name, "args": []}
                    for name in declaration.get("type_params") or []
                ],
            }
            for method in declaration.get("methods") or []:
                record_function(
                    method,
                    "class_method",
                    declaration.get("name", ""),
                    receiver_type,
                    declaration.get("type_params") or [],
                )
        elif isinstance(item.get("Trait"), dict):
            declaration = item["Trait"]
            for method in declaration.get("methods") or []:
                record_function(
                    method,
                    "trait_method",
                    declaration.get("name", ""),
                    None,
                    declaration.get("type_params") or [],
                )
        elif isinstance(item.get("Impl"), dict):
            declaration = item["Impl"]
            concreteness, concrete_reason = _impl_concreteness(
                declaration, declarations, trait_type_params
            )
            target_classification = classify_type_ref(
                declaration.get("for_type"),
                declarations,
                declaration.get("type_params") or [],
            )
            line, column = _span(declaration)
            impl_record = {
                "path": path.as_posix(),
                "line": line,
                "column": column,
                "trait": declaration.get("trait_name"),
                "target_type": render_type_ref(declaration.get("for_type")),
                "concreteness": concreteness,
                "concreteness_reason": concrete_reason,
                "target_copy_classification": target_classification["status"],
                "target_copy_reason": target_classification["reason"],
                "methods": [
                    {
                        "name": method.get("name"),
                        "receiver": method.get("receiver"),
                        "parameters": [
                            {
                                "name": parameter.get("name"),
                                "mode": parameter.get("mode"),
                                "type": render_type_ref(parameter.get("ty")),
                            }
                            for parameter in method.get("params") or []
                        ],
                    }
                    for method in declaration.get("methods") or []
                ],
            }
            evidence["trait_impls"].append(impl_record)
            for method in declaration.get("methods") or []:
                record_function(
                    method,
                    "impl_method",
                    declaration.get("trait_name", ""),
                    declaration.get("for_type"),
                    declaration.get("type_params") or [],
                )

    matches: list = []
    walk(tree, "Match", matches)
    for match_node in matches:
        capability = match_node.get(
            "capability", match_node.get("borrow_mode")
        )
        if capability not in (None, "Borrow"):
            continue
        line, column = _span(match_node)
        scrutinee_kind = (match_node.get("scrutinee") or {}).get("kind") or {}
        if any(
            literal in scrutinee_kind
            for literal in ("Int", "Float", "Bool", "DurationNanos")
        ):
            copy_classification = COPY
            copy_reason = "literal scrutinee has a compiler-defined copy type"
        else:
            copy_classification = UNRESOLVED
            copy_reason = (
                "AST JSON does not expose the checked scrutinee type; "
                "semantic disposition requires compiler-native evidence"
            )
        evidence["bare_matches"].append(
            {
                "path": path.as_posix(),
                "line": line,
                "column": column,
                "scrutinee_shape": scrutinee_shape(match_node),
                "binds_payload": arm_moves_payload(match_node),
                "scrutinee_copy_classification": copy_classification,
                "scrutinee_copy_reason": copy_reason,
            }
        )
    return evidence


def _split_top_level(text: str, separator: str = ",") -> list[str]:
    parts: list[str] = []
    start = 0
    stack: list[str] = []
    pairs = {")": "(", "]": "[", "}": "{"}
    quote = ""
    escaped = False
    for index, char in enumerate(text):
        if quote:
            if escaped:
                escaped = False
            elif char == "\\":
                escaped = True
            elif char == quote:
                quote = ""
            continue
        if char in "\"'":
            quote = char
        elif char in "([{":
            stack.append(char)
        elif char in ")]}":
            if stack and stack[-1] == pairs[char]:
                stack.pop()
        elif char == separator and not stack:
            parts.append(text[start:index].strip())
            start = index + 1
    parts.append(text[start:].strip())
    return [part for part in parts if part]


def _parse_type_text(text: str) -> tuple[dict | None, dict[str, str]]:
    text = text.strip()
    if not text:
        return None, _classification(UNRESOLVED, "signature omits a type")
    alternatives = _split_top_level(text, "|")
    if len(alternatives) > 1:
        parsed = [_parse_type_text(alternative)[1] for alternative in alternatives]
        if all(value["status"] == COPY for value in parsed):
            return None, _classification(COPY, "every rendered union member is copy")
        if all(value["status"] == MOVE for value in parsed):
            return None, _classification(MOVE, "every rendered union member is move")
        return None, _classification(
            UNRESOLVED, "rendered union has mixed or unresolved copyability"
        )
    if text.startswith("(") and text.endswith(")"):
        elements = [
            _parse_type_text(part)[0]
            for part in _split_top_level(text[1:-1])
        ]
        node = {"elements": [element for element in elements if element is not None]}
        return node, classify_type_ref(node)
    bracket = text.find("[")
    if bracket > 0 and text.endswith("]"):
        name = text[:bracket].strip()
        args = []
        for part in _split_top_level(text[bracket + 1 : -1]):
            parsed, _ = _parse_type_text(part)
            if parsed is None:
                parsed = {"name": part.strip(), "args": []}
            args.append(parsed)
        node = {"name": name, "args": args}
    else:
        node = {"name": text, "args": []}
    return node, classify_type_ref(node)


def _rendered_parameter(text: str, position: int) -> dict:
    text = text.strip()
    if text in {"own ...", "mut ...", "..."}:
        capability = "bare" if text == "..." else text.split()[0]
        return {
            "name": "...",
            "position": position,
            "capability": capability,
            "type": None,
            "copy_classification": UNRESOLVED,
            "copy_reason": "variadic rendered parameter omits a type",
        }
    if ":" not in text:
        return {
            "name": text,
            "position": position,
            "capability": "bare",
            "type": None,
            "copy_classification": UNRESOLVED,
            "copy_reason": "rendered parameter omits a type",
        }
    name, type_text = (part.strip() for part in text.split(":", 1))
    type_text = type_text.split("=", 1)[0].strip()
    capability = "bare"
    for prefix in ("own ", "mut "):
        if type_text.startswith(prefix):
            capability = prefix.strip()
            type_text = type_text[len(prefix) :].strip()
            break
    _, classification = _parse_type_text(type_text)
    return {
        "name": name,
        "position": position,
        "capability": capability,
        "type": type_text,
        "copy_classification": classification["status"],
        "copy_reason": classification["reason"],
    }


RUST_DETAIL_ARM = re.compile(
    r"(?P<variants>Self::[A-Za-z_][A-Za-z0-9_]*"
    r"(?:\s*\|\s*Self::[A-Za-z_][A-Za-z0-9_]*)*)"
    r"\s*=>\s*(?:\{\s*)?\"(?P<detail>(?:\\.|[^\"\\])*)\""
    r"(?:\s*\})?"
)


def collect_rendered_builtin_signatures(
    source: str, path: PurePosixPath
) -> list[dict]:
    records = []
    for match in RUST_DETAIL_ARM.finditer(source):
        try:
            detail = json.loads(f'"{match.group("detail")}"')
        except json.JSONDecodeError:
            continue
        if ") ->" not in detail:
            continue
        variants = re.findall(r"Self::([A-Za-z_][A-Za-z0-9_]*)", match.group("variants"))
        for signature_text in _split_top_level(detail, ";"):
            signature_match = re.fullmatch(
                r"\s*([A-Za-z_][A-Za-z0-9_.]*)\((.*)\)\s*->\s*(.+)\s*",
                signature_text,
            )
            if signature_match is None:
                continue
            callable_name, params_text, return_type = signature_match.groups()
            parameters = [
                _rendered_parameter(parameter, index)
                for index, parameter in enumerate(_split_top_level(params_text))
            ]
            line, column = _line_column(source, match.start())
            for variant in variants:
                records.append(
                    {
                        "path": path.as_posix(),
                        "line": line,
                        "column": column,
                        "variant": variant,
                        "callable": callable_name,
                        "signature": signature_text.strip(),
                        "return_type": return_type.strip(),
                        "parameters": parameters,
                    }
                )
    return records


def _iter_named_calls(source: str, names: Iterable[str]) -> Iterator[tuple[str, int, str]]:
    pattern = re.compile(
        r"\b(" + "|".join(re.escape(name) for name in names) + r")\s*\("
    )
    for match in pattern.finditer(source):
        open_index = source.find("(", match.start(), match.end())
        depth = 1
        index = open_index + 1
        quote = ""
        escaped = False
        while index < len(source) and depth:
            char = source[index]
            if quote:
                if escaped:
                    escaped = False
                elif char == "\\":
                    escaped = True
                elif char == quote:
                    quote = ""
            elif char in "\"'":
                quote = char
            elif char == "(":
                depth += 1
            elif char == ")":
                depth -= 1
            index += 1
        if depth == 0:
            yield match.group(1), match.start(), source[open_index + 1 : index - 1]


def _rust_string(text: str) -> str | None:
    match = re.fullmatch(r'\s*"((?:\\.|[^"\\])*)"\s*', text)
    if match is None:
        return None
    try:
        return json.loads(f'"{match.group(1)}"')
    except json.JSONDecodeError:
        return None


def _classify_builtin_type_expression(expression: str) -> tuple[str, str, str]:
    direct = re.match(r'\s*type_ref\(\s*"([^"]+)"', expression)
    if direct:
        type_name = direct.group(1)
        classification = classify_type_ref({"name": type_name, "args": []})
        return type_name, classification["status"], classification["reason"]
    helpers = {
        "string()": ("String", MOVE),
        "bytes_type_ref()": ("Vec[uint8]", MOVE),
        "bytes_vec_type_ref()": ("Vec[uint8]", MOVE),
        "string_map_type_ref()": ("Map[String, String]", MOVE),
        "json_value()": ("json.Value", MOVE),
    }
    compact = "".join(expression.split())
    for spelling, (type_name, status) in helpers.items():
        if compact.startswith(spelling):
            return type_name, status, f"`{spelling}` has a deterministic helper type"
    return (
        expression.strip(),
        UNRESOLVED,
        "Rust type-construction expression is not syntax-parsed by this inventory",
    )


def collect_module_builtin_parameters(
    source: str, path: PurePosixPath
) -> list[dict]:
    records = []
    capabilities = {
        "value_param": "bare",
        "borrow_param": "bare",
        "own_param": "own",
    }
    for helper, offset, body in _iter_named_calls(source, capabilities):
        args = _split_top_level(body)
        if len(args) < 2:
            continue
        parameter = _rust_string(args[0])
        if parameter is None:
            continue
        type_text, status, reason = _classify_builtin_type_expression(args[1])
        line, column = _line_column(source, offset)
        records.append(
            {
                "path": path.as_posix(),
                "line": line,
                "column": column,
                "helper": helper,
                "parameter": parameter,
                "capability": capabilities[helper],
                "type": type_text,
                "copy_classification": status,
                "copy_reason": reason,
            }
        )
    return records


BUILTIN_PARAM_CONSTANT = re.compile(
    r"const\s+([A-Z][A-Z0-9_]*_PARAMS|NO_BUILTIN_PARAMS)"
    r"\s*:\s*\[BuiltinParam;\s*\d+\]\s*=\s*(.*?);",
    re.DOTALL,
)
BUILTIN_PARAM_MACRO = re.compile(
    r'builtin_param!\(\s*(required|optional)\s*,\s*"([^"]+)"\s*,\s*'
    r"ReceiverKind::(BorrowMut|Borrow|Value)\s*\)"
)
CALL_SHAPE_LINK = re.compile(
    r"(?P<variants>Self::[A-Za-z_][A-Za-z0-9_]*"
    r"(?:\s*\|\s*Self::[A-Za-z_][A-Za-z0-9_]*)*)"
    r"\s*=>\s*(?:(?!\n\s*Self::[A-Za-z_]).)*?"
    r"BuiltinCallShape::(?P<kind>fixed|variadic)\(\s*&(?P<constant>"
    r"[A-Z][A-Z0-9_]*_PARAMS|NO_BUILTIN_PARAMS)"
    r"(?P<tail>.*?)\)",
    re.DOTALL,
)


def _builtin_param_constants(source: str) -> dict[str, list[dict]]:
    constants: dict[str, list[dict]] = {}
    for match in BUILTIN_PARAM_CONSTANT.finditer(source):
        constants[match.group(1)] = [
            {
                "required": param.group(1) == "required",
                "name": param.group(2),
                "passing": param.group(3),
            }
            for param in BUILTIN_PARAM_MACRO.finditer(match.group(2))
        ]
    return constants


def _call_shape_links(source: str) -> dict[str, dict]:
    links: dict[str, dict] = {}
    for match in CALL_SHAPE_LINK.finditer(source):
        variants = re.findall(r"Self::([A-Za-z_][A-Za-z0-9_]*)", match.group("variants"))
        variadic = None
        if match.group("kind") == "variadic":
            passing = re.search(
                r"ReceiverKind::(BorrowMut|Borrow|Value)", match.group("tail")
            )
            if passing:
                variadic = passing.group(1)
        for variant in variants:
            links[variant] = {
                "constant": match.group("constant"),
                "kind": match.group("kind"),
                "variadic_passing": variadic,
            }
    return links


def collect_builtin_capability_consistency(
    source: str, path: PurePosixPath
) -> dict[str, list]:
    signatures = collect_rendered_builtin_signatures(source, path)
    constants = _builtin_param_constants(source)
    links = _call_shape_links(source)
    expected = {"bare": "Borrow", "own": "Value", "mut": "BorrowMut"}
    mismatches = []
    comparisons = []
    unlinked = []
    for signature in signatures:
        link = links.get(signature["variant"])
        if link is None:
            unlinked.append(
                {
                    "variant": signature["variant"],
                    "callable": signature["callable"],
                    "reason": "no BuiltinCallShape link; capability application is bespoke or unresolved",
                }
            )
            continue
        metadata = constants.get(link["constant"])
        if metadata is None:
            unlinked.append(
                {
                    "variant": signature["variant"],
                    "callable": signature["callable"],
                    "reason": f"metadata constant `{link['constant']}` was not parsed",
                }
            )
            continue
        for parameter in signature["parameters"]:
            position = parameter["position"]
            if parameter["name"] == "..." and link["kind"] == "variadic":
                actual = link["variadic_passing"]
            elif position < len(metadata):
                actual = metadata[position]["passing"]
            else:
                unlinked.append(
                    {
                        "variant": signature["variant"],
                        "callable": signature["callable"],
                        "reason": f"rendered parameter {position} has no metadata slot",
                    }
                )
                continue
            comparison = {
                "variant": signature["variant"],
                "callable": signature["callable"],
                "parameter": parameter["name"],
                "position": position,
                "rendered_capability": parameter["capability"],
                "expected_passing": expected[parameter["capability"]],
                "metadata_passing": actual,
                "metadata_constant": link["constant"],
            }
            comparisons.append(comparison)
            if actual != comparison["expected_passing"]:
                mismatches.append(comparison)
    return {
        "comparisons": sorted(
            comparisons,
            key=lambda record: (
                record["variant"],
                record["position"],
                record["callable"],
            ),
        ),
        "mismatches": sorted(
            mismatches,
            key=lambda record: (
                record["variant"],
                record["position"],
                record["callable"],
            ),
        ),
        "unlinked_signatures": sorted(
            unlinked, key=lambda record: (record["variant"], record["callable"])
        ),
    }


def _matching_rust_brace(source: str, open_index: int) -> int | None:
    depth = 1
    index = open_index + 1
    quote = ""
    escaped = False
    line_comment = False
    block_comment_depth = 0
    while index < len(source):
        char = source[index]
        following = source[index + 1] if index + 1 < len(source) else ""
        if line_comment:
            if char == "\n":
                line_comment = False
        elif block_comment_depth:
            if char == "/" and following == "*":
                block_comment_depth += 1
                index += 1
            elif char == "*" and following == "/":
                block_comment_depth -= 1
                index += 1
        elif quote:
            if escaped:
                escaped = False
            elif char == "\\":
                escaped = True
            elif char == quote:
                quote = ""
        elif char == "/" and following == "/":
            line_comment = True
            index += 1
        elif char == "/" and following == "*":
            block_comment_depth = 1
            index += 1
        elif char == '"':
            quote = char
        elif char == "{":
            depth += 1
        elif char == "}":
            depth -= 1
            if depth == 0:
                return index
        index += 1
    return None


def collect_builtin_variant_coverage(source: str) -> dict[str, list | dict]:
    """Ensure every callable builtin enum variant has inventory evidence."""
    enum_variants: dict[str, list[str]] = {}
    for enum_name in (
        "BuiltinFunction",
        "BuiltinAssociatedFunction",
        "BuiltinClassConstructor",
        "BuiltinMember",
    ):
        declaration = re.search(
            rf"\bpub\s+enum\s+{re.escape(enum_name)}\s*\{{", source
        )
        if declaration is None:
            continue
        open_index = source.find("{", declaration.start(), declaration.end())
        close_index = _matching_rust_brace(source, open_index)
        if close_index is None:
            enum_variants[enum_name] = []
            continue
        body = source[open_index + 1 : close_index]
        enum_variants[enum_name] = re.findall(
            r"^\s*([A-Za-z_][A-Za-z0-9_]*)\s*,\s*(?://.*)?$",
            body,
            re.MULTILINE,
        )

    rendered = {
        record["variant"]
        for record in collect_rendered_builtin_signatures(
            source, PurePosixPath("crates/aurora-compiler/src/call.rs")
        )
    }
    links = set(_call_shape_links(source))
    missing_rendered = []
    missing_shapes = []
    for enum_name, variants in enum_variants.items():
        for variant in variants:
            qualified = f"{enum_name}::{variant}"
            if variant not in rendered:
                missing_rendered.append(qualified)
            if (
                enum_name in {"BuiltinAssociatedFunction", "BuiltinMember"}
                and variant not in links
            ):
                missing_shapes.append(qualified)
    return {
        "enum_variants": enum_variants,
        "missing_rendered_signatures": sorted(missing_rendered),
        "missing_structured_call_shapes": sorted(missing_shapes),
    }


def _builtin_member_application_positions(sema_source: str) -> dict[str, set]:
    """Extract explicit argument-passing applications from member match arms."""
    applications: dict[str, set] = {}
    for match in re.finditer(r"\bmatch\s+builtin_member\s*\{", sema_source):
        open_index = sema_source.find("{", match.start(), match.end())
        close_index = _matching_rust_brace(sema_source, open_index)
        if close_index is None:
            continue
        body = sema_source[open_index + 1 : close_index]
        for arm in _split_top_level(body):
            if "=>" not in arm:
                continue
            pattern, implementation = arm.split("=>", 1)
            variants = re.findall(
                r"BuiltinMember::([A-Za-z_][A-Za-z0-9_]*)", pattern
            )
            if not variants:
                continue
            numeric_positions = {
                int(position)
                for position in re.findall(
                    r"apply_builtin_argument_passing\s*\(\s*"
                    r"builtin_member\s*,\s*(\d+)",
                    implementation,
                )
            }
            applies_all = bool(
                re.search(
                    r"apply_builtin_argument_passing\s*\(\s*"
                    r"builtin_member\s*,\s*index\b",
                    implementation,
                )
            )
            for variant in variants:
                target = applications.setdefault(variant, set())
                target.update(numeric_positions)
                if applies_all:
                    target.add("all")
    return applications


def _builtin_variant_owners(source: str) -> dict[str, set[str]]:
    """Map callable builtin variants to the enum family that dispatches them."""
    owners: dict[str, set[str]] = {}
    for enum_name in (
        "BuiltinFunction",
        "BuiltinAssociatedFunction",
        "BuiltinMember",
    ):
        declaration = re.search(
            rf"\bpub\s+enum\s+{re.escape(enum_name)}\s*\{{", source
        )
        if declaration is None:
            continue
        open_index = source.find("{", declaration.start(), declaration.end())
        close_index = _matching_rust_brace(source, open_index)
        if close_index is None:
            continue
        body = source[open_index + 1 : close_index]
        for variant in re.findall(
            r"^\s*([A-Za-z_][A-Za-z0-9_]*)\s*,\s*(?://.*)?$",
            body,
            re.MULTILINE,
        ):
            owners.setdefault(variant, set()).add(enum_name)
    return owners


def _centralized_builtin_application_families(sema_source: str) -> set[str]:
    """Find builtin families whose common call path enforces sibling loans."""
    helper_calls = {
        "BuiltinFunction": (
            "reject_builtin_function_argument_sibling_overlap",
            "builtin",
        ),
        "BuiltinAssociatedFunction": (
            "reject_builtin_associated_argument_sibling_overlap",
            "constructor",
        ),
        "BuiltinMember": (
            "reject_builtin_member_argument_sibling_overlap",
            "builtin_member",
        ),
    }
    return {
        family
        for family, (helper, first_argument) in helper_calls.items()
        if re.search(
            rf"\bself\.{re.escape(helper)}\s*\(\s*{re.escape(first_argument)}\b",
            sema_source,
        )
    }


def collect_builtin_application_evidence(
    call_source: str,
    sema_source: str,
    call_path: PurePosixPath,
    sema_path: PurePosixPath,
) -> dict[str, list]:
    """Compare bare-copy signature positions with explicit sema application."""
    del sema_path  # Reserved in the public API for future location records.
    signatures = collect_rendered_builtin_signatures(call_source, call_path)
    links = _call_shape_links(call_source)
    applications = _builtin_member_application_positions(sema_source)
    variant_owners = _builtin_variant_owners(call_source)
    centralized_families = _centralized_builtin_application_families(sema_source)
    records = []
    missing = []
    for signature in signatures:
        structured = signature["variant"] in links
        later_positions = [parameter["position"] for parameter in signature["parameters"]]
        for parameter in signature["parameters"]:
            if (
                parameter["capability"] != "bare"
                or parameter["copy_classification"] != COPY
            ):
                continue
            later = [
                position
                for position in later_positions
                if position > parameter["position"]
            ]
            applied = (
                applications.get(signature["variant"], set())
                if structured
                else set()
            )
            if structured and (
                variant_owners.get(signature["variant"], set())
                & centralized_families
            ):
                applied = set(applied)
                applied.add("all")
            record = {
                "variant": signature["variant"],
                "callable": signature["callable"],
                "parameter": parameter["name"],
                "position": parameter["position"],
                "type": parameter["type"],
                "later_parameter_positions": later,
                "sema_application_positions": sorted(
                    position for position in applied if isinstance(position, int)
                ),
            }
            if "all" in applied:
                record["sema_application_positions"] = ["all"]
            records.append(record)
            if later and "all" not in applied and parameter["position"] not in applied:
                missing.append(record)
    sort_key = lambda record: (
        record["variant"],
        record["position"],
        record["callable"],
    )
    return {
        "bare_copy_parameter_applications": sorted(records, key=sort_key),
        "missing_sibling_retention_applications": sorted(missing, key=sort_key),
    }


def _git_revision() -> str:
    return subprocess.run(
        ["git", "rev-parse", "HEAD"],
        cwd=ROOT,
        capture_output=True,
        text=True,
        check=True,
    ).stdout.strip()


def _git_dirty() -> bool:
    return bool(
        subprocess.run(
            ["git", "status", "--porcelain"],
            cwd=ROOT,
            capture_output=True,
            text=True,
            check=True,
        ).stdout
    )


def _semantic_interface_version() -> int | None:
    source = (ROOT / "crates/aurora-compiler/src/lib.rs").read_text(
        errors="replace"
    )
    match = re.search(
        r"SEMANTIC_INTERFACE_SCHEMA_VERSION\s*:\s*u32\s*=\s*(\d+)", source
    )
    return int(match.group(1)) if match else None


def _compiler_binary_evidence() -> dict:
    if not AURA.is_file():
        return {"path": _relative(AURA).as_posix(), "present": False}
    digest = hashlib.sha256()
    with AURA.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return {
        "path": _relative(AURA).as_posix(),
        "present": True,
        "sha256": digest.hexdigest(),
        "mtime_ns": AURA.stat().st_mtime_ns,
    }


def _read_text(path: Path) -> str | None:
    data = path.read_bytes()
    if b"\0" in data:
        return None
    return data.decode("utf-8", errors="replace")


def build_inventory() -> dict:
    au_files, au_excluded = maintained_tracked("*.au")
    md_files, md_excluded = maintained_tracked("*.md")
    rs_files, rs_excluded = maintained_tracked("*.rs")
    all_files, all_excluded = maintained_tracked()

    au_borrow_records: list[dict] = []
    md_borrow_records: list[dict] = []
    md_prose_count = 0
    for path in au_files:
        source = path.read_text(errors="replace")
        au_borrow_records.extend(
            borrow_occurrence_records(_relative(path), source, "aurora")
        )
    for path in md_files:
        source = path.read_text(errors="replace")
        records = borrow_occurrence_records(_relative(path), source, "markdown")
        md_borrow_records.extend(records)
        md_prose_count += borrow_tokens(source) - len(records)

    supplementary_records: list[dict] = []
    uppercase_borrow_identifiers: list[dict] = []
    primary_paths = set(au_files) | set(md_files)
    for path in all_files:
        if path in primary_paths:
            continue
        source = _read_text(path)
        if source is None:
            continue
        relative = _relative(path)
        surface = path.suffix.lstrip(".") or "text"
        supplementary_records.extend(
            borrow_occurrence_records(relative, source, surface)
        )
        for match in re.finditer(
            r"(?<![A-Za-z0-9_])Borrow(?![A-Za-z0-9_])", source
        ):
            line, column = _line_column(source, match.start())
            uppercase_borrow_identifiers.append(
                {
                    "path": relative.as_posix(),
                    "line": line,
                    "column": column,
                    "surface": surface,
                    "classification": "internal_identifier_or_text_review",
                }
            )

    parsed = 0
    unparsed: list[dict] = []
    parameters: list[dict] = []
    receivers: list[dict] = []
    trait_impls: list[dict] = []
    bare_matches: list[dict] = []
    match_modes = Counter()
    for path in au_files:
        result = subprocess.run(
            [str(AURA), "ast-json", str(path)],
            cwd=ROOT,
            capture_output=True,
            text=True,
        )
        if result.returncode != 0:
            unparsed.append(
                {
                    "path": _relative(path).as_posix(),
                    "returncode": result.returncode,
                    "first_error_line": next(
                        (
                            line.strip()
                            for line in result.stderr.splitlines()
                            if line.strip()
                        ),
                        "",
                    ),
                }
            )
            continue
        try:
            tree = json.loads(result.stdout)
        except json.JSONDecodeError as error:
            unparsed.append(
                {
                    "path": _relative(path).as_posix(),
                    "returncode": result.returncode,
                    "first_error_line": f"invalid AST JSON: {error}",
                }
            )
            continue
        parsed += 1
        matches: list = []
        walk(tree, "Match", matches)
        for match_node in matches:
            capability = match_node.get(
                "capability", match_node.get("borrow_mode")
            )
            if capability is None or capability == "Borrow":
                match_modes["bare"] += 1
            elif capability == "BorrowMut":
                match_modes["mut"] += 1
            elif capability == "Value":
                match_modes["own"] += 1
            else:
                match_modes[f"unknown:{capability}"] += 1
        evidence = collect_ast_evidence(tree, _relative(path))
        parameters.extend(evidence["parameters"])
        receivers.extend(evidence["receivers"])
        trait_impls.extend(evidence["trait_impls"])
        bare_matches.extend(evidence["bare_matches"])

    call_path = ROOT / "crates/aurora-compiler/src/call.rs"
    call_source = call_path.read_text(errors="replace")
    rendered_builtins = collect_rendered_builtin_signatures(
        call_source, _relative(call_path)
    )
    builtin_consistency = collect_builtin_capability_consistency(
        call_source, _relative(call_path)
    )
    builtin_variant_coverage = collect_builtin_variant_coverage(call_source)
    sema_path = ROOT / "crates/aurora-compiler/src/sema.rs"
    builtin_applications = collect_builtin_application_evidence(
        call_source,
        sema_path.read_text(errors="replace"),
        _relative(call_path),
        _relative(sema_path),
    )
    module_path = ROOT / "crates/aurora-compiler/src/builtin_modules.rs"
    module_builtins = collect_module_builtin_parameters(
        module_path.read_text(errors="replace"), _relative(module_path)
    )

    parameter_modes = Counter(record["mode"] for record in parameters)
    receiver_modes = Counter(record["mode"] for record in receivers)
    bare_copy_parameters = [
        record
        for record in parameters
        if record["mode"] == "Default"
        and record["copy_classification"] == COPY
    ]
    unresolved_bare_parameters = [
        record
        for record in parameters
        if record["mode"] == "Default"
        and record["copy_classification"] == UNRESOLVED
    ]
    bare_copy_receivers = [
        record
        for record in receivers
        if record["mode"] == "Borrow"
        and record["copy_classification"] == COPY
    ]
    unresolved_bare_receivers = [
        record
        for record in receivers
        if record["mode"] == "Borrow"
        and record["copy_classification"] == UNRESOLVED
    ]
    concrete_impls = [
        record
        for record in trait_impls
        if record["concreteness"] == CONCRETE
    ]
    unresolved_impls = [
        record
        for record in trait_impls
        if record["concreteness"] == UNRESOLVED
    ]
    builtin_bare_copy_parameters = [
        {
            "path": signature["path"],
            "line": signature["line"],
            "variant": signature["variant"],
            "callable": signature["callable"],
            **parameter,
        }
        for signature in rendered_builtins
        for parameter in signature["parameters"]
        if parameter["capability"] == "bare"
        and parameter["copy_classification"] == COPY
    ]
    builtin_unresolved_parameters = [
        {
            "path": signature["path"],
            "line": signature["line"],
            "variant": signature["variant"],
            "callable": signature["callable"],
            **parameter,
        }
        for signature in rendered_builtins
        for parameter in signature["parameters"]
        if parameter["copy_classification"] == UNRESOLVED
    ]

    def sorted_records(records: list[dict]) -> list[dict]:
        return sorted(
            records,
            key=lambda record: (
                record.get("path", ""),
                record.get("line") or -1,
                record.get("column") or -1,
                record.get("owner", ""),
                record.get("function", ""),
                record.get("parameter", ""),
            ),
        )

    excluded_records = {
        tuple(sorted(record.items()))
        for record in au_excluded + md_excluded + rs_excluded + all_excluded
    }
    excluded = [dict(record) for record in sorted(excluded_records)]

    summary = {
        "au_files": len(au_files),
        "md_files": len(md_files),
        "rs_files": len(rs_files),
        "maintained_tracked_files": len(all_files),
        "excluded_tracked_files": len(excluded),
        "borrow_keyword_au_files": len(
            {record["path"] for record in au_borrow_records}
        ),
        "borrow_keyword_au_tokens": len(au_borrow_records),
        "borrow_keyword_md_files": len(
            {record["path"] for record in md_borrow_records}
        ),
        "borrow_keyword_md_tokens": len(md_borrow_records),
        "borrow_prose_md_words": md_prose_count,
        "borrow_supplementary_text_files": len(
            {record["path"] for record in supplementary_records}
        ),
        "borrow_supplementary_text_tokens": len(supplementary_records),
        "borrow_identifier_tokens": len(uppercase_borrow_identifiers),
        "parsed_au_files": parsed,
        "unparsed_au_files": len(unparsed),
        "matches": sum(match_modes.values()),
        "bare_matches": match_modes["bare"],
        "mut_matches": match_modes["mut"],
        "own_matches": match_modes["own"],
        "bare_matches_place_scrutinee": sum(
            record["scrutinee_shape"] == "place" for record in bare_matches
        ),
        "bare_matches_temporary_scrutinee": sum(
            record["scrutinee_shape"] == "temporary" for record in bare_matches
        ),
        "bare_matches_binding_payload": sum(
            record["binds_payload"] for record in bare_matches
        ),
        "bare_matches_unresolved_scrutinee_type": sum(
            record["scrutinee_copy_classification"] == UNRESOLVED
            for record in bare_matches
        ),
        "parameters": len(parameters),
        "bare_parameters": parameter_modes["Default"],
        "bare_copy_parameters": len(bare_copy_parameters),
        "bare_unresolved_parameters": len(unresolved_bare_parameters),
        "explicit_borrow_parameters": parameter_modes["Borrow"],
        "mut_parameters": parameter_modes["BorrowMut"],
        "own_parameters": parameter_modes["Own"],
        "receivers": len(receivers),
        "bare_receivers": receiver_modes["Borrow"],
        "bare_copy_receivers": len(bare_copy_receivers),
        "bare_unresolved_receivers": len(unresolved_bare_receivers),
        "mut_receivers": receiver_modes["BorrowMut"],
        "own_receivers": receiver_modes["Value"],
        "trait_impls": len(trait_impls),
        "concrete_trait_impls": len(concrete_impls),
        "generic_trait_impls": sum(
            record["concreteness"] == GENERIC for record in trait_impls
        ),
        "unresolved_trait_impls": len(unresolved_impls),
        "rendered_builtin_signatures": len(rendered_builtins),
        "rendered_builtin_bare_copy_parameters": len(
            builtin_bare_copy_parameters
        ),
        "rendered_builtin_unresolved_parameters": len(
            builtin_unresolved_parameters
        ),
        "module_builtin_parameter_declarations": len(module_builtins),
        "builtin_capability_metadata_mismatches": len(
            builtin_consistency["mismatches"]
        ),
        "builtin_missing_sibling_retention_applications": len(
            builtin_applications["missing_sibling_retention_applications"]
        ),
        "builtin_signatures_without_call_shape_link": len(
            builtin_consistency["unlinked_signatures"]
        ),
        "builtin_variants_without_rendered_signature": len(
            builtin_variant_coverage["missing_rendered_signatures"]
        ),
        "structured_builtin_variants_without_call_shape": len(
            builtin_variant_coverage["missing_structured_call_shapes"]
        ),
    }
    return {
        "schema_version": 2,
        "baseline": {
            "git_revision": _git_revision(),
            "working_tree_dirty": _git_dirty(),
            "semantic_interface_schema_version": _semantic_interface_version(),
            "compiler_binary": _compiler_binary_evidence(),
        },
        "scope": {
            "policy": (
                "all git-tracked text is supplementary evidence; Aurora and "
                "Markdown receive syntax-aware scans; only generated, dependency, "
                "build, and VCS trees are excluded"
            ),
            "excluded": excluded,
        },
        "summary": summary,
        # Legacy flat totals remain for work-note consumers of the original tool.
        **summary,
        "evidence": {
            "borrow_aurora": sorted_records(au_borrow_records),
            "borrow_markdown_code": sorted_records(md_borrow_records),
            "borrow_supplementary_text": sorted_records(supplementary_records),
            "borrow_uppercase_identifiers": sorted_records(
                uppercase_borrow_identifiers
            ),
            "bare_matches": sorted_records(bare_matches),
            "bare_copy_parameters": sorted_records(bare_copy_parameters),
            "bare_copy_receivers": sorted_records(bare_copy_receivers),
            "concrete_trait_impls": sorted_records(concrete_impls),
            "rendered_builtin_bare_copy_parameters": sorted_records(
                builtin_bare_copy_parameters
            ),
            "module_builtin_parameters": sorted_records(module_builtins),
            "builtin_capability_consistency": builtin_consistency,
            "builtin_application_evidence": builtin_applications,
            "builtin_variant_coverage": builtin_variant_coverage,
        },
        "review_queue": {
            "unparsed_aurora_files": sorted_records(unparsed),
            "bare_matches_without_checked_scrutinee_type": sorted_records(
                [
                    record
                    for record in bare_matches
                    if record["scrutinee_copy_classification"] == UNRESOLVED
                ]
            ),
            "unresolved_bare_parameters": sorted_records(
                unresolved_bare_parameters
            ),
            "unresolved_bare_receivers": sorted_records(
                unresolved_bare_receivers
            ),
            "unresolved_trait_impls": sorted_records(unresolved_impls),
            "unresolved_rendered_builtin_parameters": sorted_records(
                builtin_unresolved_parameters
            ),
            "builtin_signatures_without_call_shape_link": builtin_consistency[
                "unlinked_signatures"
            ],
        },
        "limitations": [
            (
                "`aura ast-json` does not expose checked expression types, so "
                "non-literal match scrutinee copyability is an explicit review "
                "queue rather than a guessed classification"
            ),
            (
                "ASTs are per-file; imported or qualified user types remain "
                "unresolved unless compiler-native checked-program inventory is added"
            ),
            (
                "Builtin rendered-signature comparison is grounded in structured "
                "BuiltinCallShape metadata; any future unlinked callable is exposed "
                "as a review-queue entry and fails the strict inventory gate"
            ),
            (
                "Supplementary text occurrences are exhaustive token evidence, "
                "not claims that every occurrence is accepted Aurora syntax"
            ),
        ],
    }


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--check",
        action="store_true",
        help=(
            "fail for active Aurora retired tokens or definite rendered/metadata "
            "builtin capability mismatches, omissions, or unlinked signatures"
        ),
    )
    args = parser.parse_args(argv)
    inventory = build_inventory()
    print(json.dumps(inventory, indent=2, sort_keys=True))
    if not args.check:
        return 0
    active_tokens = [
        record
        for record in inventory["evidence"]["borrow_aurora"]
        if record["classification"] != "retirement_fixture"
    ]
    mismatches = inventory["evidence"]["builtin_capability_consistency"][
        "mismatches"
    ]
    application_mismatches = inventory["evidence"]["builtin_application_evidence"][
        "missing_sibling_retention_applications"
    ]
    coverage = inventory["evidence"]["builtin_variant_coverage"]
    missing_variants = coverage["missing_rendered_signatures"]
    missing_shapes = coverage["missing_structured_call_shapes"]
    unlinked_signatures = inventory["evidence"]["builtin_capability_consistency"][
        "unlinked_signatures"
    ]
    if (
        active_tokens
        or mismatches
        or application_mismatches
        or missing_variants
        or missing_shapes
        or unlinked_signatures
    ):
        print(
            "capability inventory check failed: "
            f"{len(active_tokens)} active retired Aurora token(s), "
            f"{len(mismatches)} builtin capability metadata mismatch(es), "
            f"{len(application_mismatches)} missing builtin sibling-retention "
            f"application(s), {len(missing_variants)} builtin variant(s) without "
            f"rendered signatures, {len(missing_shapes)} structured builtin "
            f"variant(s) without call shapes, {len(unlinked_signatures)} rendered "
            "signature(s) without call-shape metadata",
            file=sys.stderr,
        )
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
