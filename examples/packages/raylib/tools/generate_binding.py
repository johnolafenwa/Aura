#!/usr/bin/env python3
"""Writes `src/raylib.au` from raylib's own description of its API.

raylib ships `tools/rlparser/output/raylib_api.json`, which lists every struct,
enum, constant, and function of `raylib.h`. This script turns that file
into Aura `extern "C"` declarations. A fixed-size array in a struct is
an `array[T, N]` field, and a pointer to the first of a run of structs is
a `ptr[Name]` parameter. A function whose C signature has no Aura
spelling is left out, and the end of the generated module lists each
one with the reason.

Usage:
    python3 tools/generate_binding.py path/to/raylib_api.json > src/raylib.au
    aura fmt src/raylib.au
"""

import json
import sys

SCALARS = {
    "bool": "bool",
    "char": "int8",
    "unsigned char": "uint8",
    "short": "int16",
    "unsigned short": "uint16",
    "int": "int32",
    "unsigned int": "uint32",
    "long": "int64",
    "float": "float32",
    "double": "float64",
}

# Aura keywords and reserved names that raylib uses for parameters.
RENAMES = {
    "type": "kind",
    "from": "start",
    "in": "source",
    "is": "flag",
    "not": "flag",
    "and": "both",
    "or": "either",
    "def": "definition",
    "class": "group",
    "enum": "choice",
    "match": "found",
    "case": "branch",
    "pass": "step",
    "own": "owned",
    "mut": "changed",
    "view": "camera_view",
    "import": "source",
    "public": "shared",
    "extern": "outside",
    "opaque": "solid",
    "symbol": "glyph",
    "drop": "release",
    "self": "this",
    "true": "yes",
    "false": "no",
    "None": "nothing",
    "len": "length",
    "str": "text",
    "list": "items",
    "print": "show",
    "range": "span",
    "ptr": "pointer",
    "cstr": "text",
}

# Pointer parameters that point to the first of a run of values. Every
# other pointer to a scalar or struct is one value the function reads and
# may write.
ARRAY_PARAMS = {
    "points",
    "transforms",
    "codepoints",
    "samples",
    "glyphs",
    "glyphRecs",
    "fileData",
    "data",
    "compData",
    "colors",
    "palette",
    "animations",
}

# Functions that take or return memory the caller must not treat as text.
ADDRESS_RESULTS = {"char *", "unsigned char *", "void *", "int *", "float *", "const char **", "char **"}


# Names that `typedef` gives to a pointer type.
POINTER_ALIASES = set()


class Skip(Exception):
    """The declaration has no Aura spelling."""


def rename(name):
    return RENAMES.get(name, name)


def field_type(c_type, structs, aliases):
    c_type = c_type.strip()
    if c_type in SCALARS:
        return SCALARS[c_type]
    if c_type.endswith("*") or c_type in POINTER_ALIASES:
        # The address is passed back to raylib and never followed.
        return "uint64"
    if c_type.endswith("]"):
        # `float[4]` is four floats in a row inside the struct.
        element, _, length = c_type[:-1].rpartition("[")
        return f"array[{field_type(element, structs, aliases)}, {int(length)}]"
    name = aliases.get(c_type, c_type)
    if name in structs:
        return name
    raise Skip(f"unbound field `{c_type}`")


def param_type(c_type, name, structs, aliases, callbacks, function=""):
    c_type = c_type.strip()
    if c_type == "...":
        raise Skip("variadic")
    if c_type in callbacks:
        raise Skip(f"callback `{c_type}`")
    if c_type in SCALARS:
        return SCALARS[c_type]
    if c_type == "const char *":
        return "cstr"
    if c_type in ("void *", "const void *", "char *", "const char **", "char **"):
        return "uint64"
    if c_type in POINTER_ALIASES:
        return "uint64"
    if c_type.endswith("*"):
        const = c_type.startswith("const ")
        base = c_type[:-1].replace("const ", "").strip()
        if base.endswith("*"):
            raise Skip(f"pointer to pointer `{c_type}`")
        if base in SCALARS:
            scalar = SCALARS[base]
            if const or name in ARRAY_PARAMS:
                return f"ptr[{scalar}]" if const else f"mut ptr[{scalar}]"
            return f"mut {scalar}"
        base = aliases.get(base, base)
        if base in structs:
            if function.startswith("Unload"):
                # The address a `Load` function returned, passed back.
                return "uint64"
            if const:
                return f"ptr[{base}]"
            if name in ARRAY_PARAMS:
                return f"mut ptr[{base}]"
            return f"mut {base}"
        raise Skip(f"pointer `{c_type}`")
    name = aliases.get(c_type, c_type)
    if name in structs:
        return name
    raise Skip(f"type `{c_type}`")


def result_type(c_type, structs, aliases):
    c_type = c_type.strip()
    if c_type == "void":
        return "None"
    if c_type in SCALARS:
        return SCALARS[c_type]
    if c_type == "const char *":
        return "cstr"
    if c_type in ADDRESS_RESULTS or c_type.endswith("*") or c_type in POINTER_ALIASES:
        return "uint64"
    name = aliases.get(c_type, c_type)
    if name in structs:
        return name
    raise Skip(f"result `{c_type}`")


def unbound(c_type, aliases, skipped_structs):
    """Raises `Skip` when `c_type` names a struct the module leaves out."""
    base = c_type.replace("const ", "").replace("*", "").strip()
    base = aliases.get(base, base)
    if base in skipped_structs:
        raise Skip(f"uses `{base}`, which has an {skipped_structs[base]}")


def wrap(text, indent, width=78):
    words = text.split()
    lines, line = [], ""
    for word in words:
        if line and len(indent) + len(line) + 1 + len(word) > width:
            lines.append(indent + line)
            line = word
        else:
            line = f"{line} {word}".strip()
    if line:
        lines.append(indent + line)
    return lines


def load_api(path):
    """Reads raylib's API file.

    The file raylib 6.0 ships has a description with quotation marks that
    are not escaped, so each description is escaped again before parsing.
    """
    lines = []
    prefix = '"description": "'
    for line in open(path, encoding="utf-8"):
        body = line.rstrip("\n")
        start = body.find(prefix)
        if start >= 0:
            tail = "," if body.endswith(",") else ""
            end = len(body) - len(tail) - 1
            text = body[start + len(prefix):end].replace('\\"', '"').replace('"', '\\"')
            body = body[:start] + prefix + text + '"' + tail
        lines.append(body)
    return json.loads("\n".join(lines))


def main():
    api = load_api(sys.argv[1])
    aliases = {
        alias["name"]: alias["type"]
        for alias in api["aliases"]
        if not alias["name"].startswith("*")
    }
    # `typedef Transform *ModelAnimPose` names a pointer, which is an address.
    for alias in api["aliases"]:
        if alias["name"].startswith("*"):
            POINTER_ALIASES.add(alias["name"][1:])
    callbacks = {callback["name"] for callback in api["callbacks"]}

    out = []
    version = next(
        (define["value"] for define in api["defines"] if define["name"] == "RAYLIB_VERSION"),
        "",
    )
    out.append(f'"""raylib {version}, bound through FFI.'.replace("  ", " "))
    out.append("")
    out.append("Generated by `tools/generate_binding.py` from raylib's own")
    out.append("`raylib_api.json`. Do not edit this file by hand.")
    out.append("Names are raylib's own, so its cheatsheet and examples read")
    out.append("across: `InitWindow` in C is `raylib.InitWindow` here.")
    out.append("")
    out.append("A pointer inside a struct, such as `Image.data`, is a `uint64`")
    out.append("address that raylib owns. Pass the struct back to raylib and do")
    out.append("not change the address.")
    out.append('"""')
    out.append("")

    structs = {}
    skipped_structs = {}
    pending = list(api["structs"])
    # A struct can hold a struct declared before it, so one pass in file
    # order resolves every field.
    for struct in pending:
        try:
            fields = [
                (rename(field["name"]), field_type(field["type"], structs, aliases))
                for field in struct["fields"]
            ]
        except Skip as reason:
            skipped_structs[struct["name"]] = str(reason)
            continue
        if not fields:
            skipped_structs[struct["name"]] = "no fields"
            continue
        structs[struct["name"]] = fields
        out.append(f'public extern "C" class {struct["name"]}:')
        if struct.get("description"):
            out.append(f'    """{struct["description"]}."""')
        for name, ty in fields:
            out.append(f"    public {name}: {ty}")
        out.append("")

    for enum in api["enums"]:
        out.append(f'# {enum["name"]}: {enum["description"]}')
        for value in enum["values"]:
            out.append(f'public {value["name"]}: int32 = {value["value"]}')
        out.append("")

    out.append("# Colors")
    for define in api["defines"]:
        if define["type"] != "COLOR":
            continue
        channels = define["value"].split("{")[1].split("}")[0].split(",")
        r, g, b, a = (int(channel) for channel in channels)
        out.append(f'public {define["name"]}: Color = Color(r={r}, g={g}, b={b}, a={a})')
    out.append("")

    skipped = []
    for function in api["functions"]:
        name = function["name"]
        try:
            params = []
            for param in function.get("params", []):
                unbound(param["type"], aliases, skipped_structs)
                pname = rename(param["name"])
                ptype = param_type(
                    param["type"], param["name"], structs, aliases, callbacks, name
                )
                params.append(f"{pname}: {ptype}")
            unbound(function["returnType"], aliases, skipped_structs)
            result = result_type(function["returnType"], structs, aliases)
        except Skip as reason:
            skipped.append((name, str(reason)))
            continue
        out.extend(wrap(function["description"], "# "))
        line = f'public extern "C" def {name}({", ".join(params)}) -> {result}'
        if len(line) <= 100:
            out.append(line)
        else:
            out.append(f'public extern "C" def {name}(')
            for index, param in enumerate(params):
                comma = "," if index + 1 < len(params) else ""
                out.append(f"    {param}{comma}")
            out.append(f") -> {result}")
        out.append("")

    out.append("# Not bound: structs")
    for name, reason in skipped_structs.items():
        out.append(f"#   {name}: {reason}")
    out.append("#")
    out.append(f"# Not bound: {len(skipped)} of {len(api['functions'])} functions")
    for name, reason in skipped:
        out.append(f"#   {name}: {reason}")
    sys.stdout.write("\n".join(out) + "\n")


if __name__ == "__main__":
    main()
