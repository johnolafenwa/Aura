"use strict";

const KEYWORDS = [
  "class",
  "enum",
  "trait",
  "def",
  "if",
  "elif",
  "else",
  "while",
  "for",
  "in",
  "match",
  "case",
  "with",
  "select",
  "return",
  "try",
  "spawn",
  "as",
  "and",
  "or",
  "not",
  "detached",
  "public",
  "mut",
  "borrow",
  "indirect",
  "copy",
  "break",
  "continue",
  "pass"
];

const PRIMITIVE_TYPES = new Set([
  "bool",
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
  "String",
  "None",
  "Duration",
  "Queue",
  "Task",
  "TaskGroup"
]);

const BUILTIN_MEMBERS = {
  float64: [
    {
      name: "sqrt",
      kind: "method",
      detail: "sqrt() -> float64",
      documentation: "Returns the square root of a `float64` value."
    }
  ],
  String: [
    {
      name: "len",
      kind: "method",
      detail: "len() -> int32",
      documentation: "Returns the number of bytes in the string."
    },
    {
      name: "contains",
      kind: "method",
      detail: "contains(text: String) -> bool",
      documentation: "Returns true when the string contains `text`."
    },
    {
      name: "starts_with",
      kind: "method",
      detail: "starts_with(text: String) -> bool",
      documentation: "Returns true when the string starts with `text`."
    },
    {
      name: "ends_with",
      kind: "method",
      detail: "ends_with(text: String) -> bool",
      documentation: "Returns true when the string ends with `text`."
    },
    {
      name: "split",
      kind: "method",
      detail: "split(text: String) -> Vec[String]",
      documentation: "Splits the string on each occurrence of `text` and returns the pieces as `Vec[String]`."
    },
    {
      name: "replace",
      kind: "method",
      detail: "replace(from: String, to: String) -> String",
      documentation: "Returns a new `String` with each occurrence of `from` replaced by `to`."
    },
    {
      name: "to_lower",
      kind: "method",
      detail: "to_lower() -> String",
      documentation: "Returns a new `String` with Unicode lowercase conversion applied."
    },
    {
      name: "to_upper",
      kind: "method",
      detail: "to_upper() -> String",
      documentation: "Returns a new `String` with Unicode uppercase conversion applied."
    },
    {
      name: "strip_prefix",
      kind: "method",
      detail: "strip_prefix(text: String) -> Option[String]",
      documentation: "Removes `text` from the front of the string and returns the remaining `String`, or `Option.None` when it does not match."
    },
    {
      name: "strip_suffix",
      kind: "method",
      detail: "strip_suffix(text: String) -> Option[String]",
      documentation: "Removes `text` from the end of the string and returns the remaining `String`, or `Option.None` when it does not match."
    },
    {
      name: "trim",
      kind: "method",
      detail: "trim() -> String",
      documentation: "Creates a new `String` with leading and trailing whitespace removed."
    },
    {
      name: "join",
      kind: "method",
      detail: "join(parts: Vec[String]) -> String",
      documentation: "Joins `parts` with this string as the separator."
    },
    {
      name: "clone",
      kind: "method",
      detail: "clone() -> String",
      documentation: "Creates a new owned `String` with the same contents."
    }
  ],
  Vec: [
    {
      name: "len",
      kind: "method",
      detail: "len() -> int32",
      documentation: "Returns the number of items in the vector."
    },
    {
      name: "is_empty",
      kind: "method",
      detail: "is_empty() -> bool",
      documentation: "Returns true when the vector contains no elements."
    },
    {
      name: "clone",
      kind: "method",
      detail: "clone() -> Vec[T]",
      documentation: "Creates a new vector with cloned contents."
    },
    {
      name: "push",
      kind: "method",
      detail: "push(value) -> None",
      documentation: "Appends a value to the end of the vector."
    },
    {
      name: "pop",
      kind: "method",
      detail: "pop() -> Option[T]",
      documentation: "Removes and returns the final element, or `Option.None` when empty."
    },
    {
      name: "get",
      kind: "method",
      detail: "get(index: int32) -> Option[T]",
      documentation: "Returns the element at `index`, or `Option.None` when the index is out of bounds."
    },
    {
      name: "set",
      kind: "method",
      detail: "set(index: int32, value: T) -> Option[T]",
      documentation: "Replaces the element at `index` and returns the previous element, or `Option.None` when the index is out of bounds."
    },
    {
      name: "remove",
      kind: "method",
      detail: "remove(index: int32) -> Option[T]",
      documentation: "Removes the element at `index` and returns it, or `Option.None` when the index is out of bounds."
    },
    {
      name: "swap",
      kind: "method",
      detail: "swap(first: int32, second: int32) -> bool",
      documentation: "Swaps the elements at `first` and `second`, returning `false` when either index is out of bounds."
    },
    {
      name: "contains",
      kind: "method",
      detail: "contains(value: T) -> bool",
      documentation: "Returns true when the vector contains `value`."
    },
    {
      name: "insert",
      kind: "method",
      detail: "insert(index: int32, value: T) -> bool",
      documentation: "Inserts `value` at `index`, returning false when the index is beyond the current length."
    },
    {
      name: "clear",
      kind: "method",
      detail: "clear() -> None",
      documentation: "Removes all elements from the vector."
    },
    {
      name: "reverse",
      kind: "method",
      detail: "reverse() -> None",
      documentation: "Reverses the vector in place."
    },
    {
      name: "extend",
      kind: "method",
      detail: "extend(other: Vec[T]) -> None",
      documentation: "Appends the elements of `other` to the end of the vector."
    }
  ],
  Map: [
    {
      name: "len",
      kind: "method",
      detail: "len() -> int32",
      documentation: "Returns the number of entries in the map."
    },
    {
      name: "is_empty",
      kind: "method",
      detail: "is_empty() -> bool",
      documentation: "Returns true when the map contains no entries."
    },
    {
      name: "clone",
      kind: "method",
      detail: "clone() -> Map[K, V]",
      documentation: "Creates a new owned `Map[K, V]` with cloned keys and values."
    },
    {
      name: "get",
      kind: "method",
      detail: "get(key: K) -> Option[V]",
      documentation: "Returns the value for `key`, or `Option.None` when the key is missing."
    },
    {
      name: "set",
      kind: "method",
      detail: "set(key: K, value: V) -> Option[V]",
      documentation: "Inserts or replaces the value for `key`, returning the previous value when one existed."
    },
    {
      name: "remove",
      kind: "method",
      detail: "remove(key: K) -> Option[V]",
      documentation: "Removes `key` from the map and returns its previous value, or `Option.None` when absent."
    },
    {
      name: "contains_key",
      kind: "method",
      detail: "contains_key(key: K) -> bool",
      documentation: "Returns true when `key` is present in the map."
    },
    {
      name: "keys",
      kind: "method",
      detail: "keys() -> Vec[K]",
      documentation: "Returns the current keys as a `Vec[K]`."
    },
    {
      name: "values",
      kind: "method",
      detail: "values() -> Vec[V]",
      documentation: "Returns the current values as a `Vec[V]`."
    },
    {
      name: "items",
      kind: "method",
      detail: "items() -> Vec[MapEntry[K, V]]",
      documentation: "Returns the current entries as `Vec[MapEntry[K, V]]` in insertion order."
    },
    {
      name: "entries",
      kind: "method",
      detail: "entries() -> Vec[MapEntry[K, V]]",
      documentation: "Returns the current entries as `Vec[MapEntry[K, V]]` in insertion order."
    },
    {
      name: "clear",
      kind: "method",
      detail: "clear() -> None",
      documentation: "Removes all entries from the map."
    },
    {
      name: "extend",
      kind: "method",
      detail: "extend(other: Map[K, V]) -> None",
      documentation: "Merges entries from `other` into the map, overwriting existing keys."
    }
  ],
  Set: [
    {
      name: "len",
      kind: "method",
      detail: "len() -> int32",
      documentation: "Returns the number of elements in the set."
    },
    {
      name: "is_empty",
      kind: "method",
      detail: "is_empty() -> bool",
      documentation: "Returns true when the set contains no elements."
    },
    {
      name: "clone",
      kind: "method",
      detail: "clone() -> Set[T]",
      documentation: "Creates a new owned `Set[T]` with cloned elements."
    },
    {
      name: "contains",
      kind: "method",
      detail: "contains(value: T) -> bool",
      documentation: "Returns true when the set contains `value`."
    },
    {
      name: "insert",
      kind: "method",
      detail: "insert(value: T) -> bool",
      documentation: "Inserts `value` into the set and returns true when it was newly added."
    },
    {
      name: "remove",
      kind: "method",
      detail: "remove(value: T) -> bool",
      documentation: "Removes `value` from the set and returns true when it was present."
    }
  ],
  MapEntry: [
    {
      name: "key",
      kind: "field",
      detail: "key: K",
      type: "K",
      documentation: "The key component of a `MapEntry[K, V]`."
    },
    {
      name: "value",
      kind: "field",
      detail: "value: V",
      type: "V",
      documentation: "The value component of a `MapEntry[K, V]`."
    }
  ],
  Queue: [
    {
      name: "put",
      kind: "method",
      detail: "put(value) -> Result[None, SendError[T]]",
      documentation: "Puts a value into the queue or returns `SendError.Closed(value)` if the queue is closed."
    },
    {
      name: "get",
      kind: "method",
      detail: "get(timeout: Duration = ...) -> Option[T]",
      documentation: "Receives the next value from the queue, or `Option.None` when the queue is closed or the optional timeout expires."
    },
    {
      name: "close",
      kind: "method",
      detail: "close() -> None",
      documentation: "Closes the queue and wakes blocked receivers."
    }
  ],
  Task: [
    {
      name: "result",
      kind: "method",
      detail: "result() -> T",
      documentation: "Waits for the spawned task to finish and returns its value."
    }
  ],
  TaskGroup: [
    {
      name: "start",
      kind: "method",
      detail: "start(function, ...) -> Task[T]",
      documentation: "Starts a child task in the current task group."
    },
    {
      name: "cancel",
      kind: "method",
      detail: "cancel() -> None",
      documentation: "Signals cancellation to child tasks in the current task group."
    }
  ]
};

const BUILTIN_FUNCTIONS = [
  {
    name: "print",
    kind: "function",
    detail: "print(value) -> None",
    documentation: "Writes a value followed by a newline."
  },
  {
    name: "range",
    kind: "function",
    detail: "range(stop: int32) -> Range; range(start: int32, stop: int32) -> Range",
    documentation:
      "Builds an integer range from 0 up to, but not including, `stop`, or from `start` up to, but not including, `stop`."
  },
  {
    name: "queue",
    kind: "function",
    detail: "queue() -> Queue[T]",
    documentation: "Creates a typed queue when the surrounding annotation or expectation provides `T`."
  },
  {
    name: "tasks",
    kind: "function",
    detail: "tasks() -> TaskGroup",
    documentation: "Creates a managed structured-concurrency task group for use with `with`."
  },
  {
    name: "cancelled",
    kind: "function",
    detail: "cancelled() -> bool",
    documentation: "Returns true when the current task has been cancelled."
  },
  {
    name: "after",
    kind: "function",
    detail: "after(duration: Duration) -> Duration",
    documentation: "Builds a timeout/select timer expression from a duration literal or duration value."
  },
  {
    name: "sleep",
    kind: "function",
    detail: "sleep(duration: Duration) -> None",
    documentation: "Blocks the current task for the requested duration."
  },
  {
    name: "abs",
    kind: "function",
    detail: "abs(value: Number) -> Number",
    documentation: "Returns the absolute value of an integer or floating-point number."
  },
  {
    name: "min",
    kind: "function",
    detail: "min(left: Number, right: Number) -> Number",
    documentation: "Returns the smaller of two numeric values with the same type."
  },
  {
    name: "max",
    kind: "function",
    detail: "max(left: Number, right: Number) -> Number",
    documentation: "Returns the larger of two numeric values with the same type."
  },
  {
    name: "sqrt",
    kind: "function",
    detail: "sqrt(value: float32|float64) -> float64",
    documentation: "Returns the square root of a floating-point value."
  },
  {
    name: "parse_int32",
    kind: "function",
    detail: "parse_int32(text: String) -> Result[int32, String]",
    documentation: "Parses a `String` into an `int32`, returning `Result.Err(String)` on failure."
  },
  {
    name: "parse_int64",
    kind: "function",
    detail: "parse_int64(text: String) -> Result[int64, String]",
    documentation: "Parses a `String` into an `int64`, returning `Result.Err(String)` on failure."
  },
  {
    name: "parse_float64",
    kind: "function",
    detail: "parse_float64(text: String) -> Result[float64, String]",
    documentation: "Parses a `String` into a `float64`, returning `Result.Err(String)` on failure."
  }
];

const BUILTIN_FUNCTION_MAP = new Map(BUILTIN_FUNCTIONS.map((item) => [item.name, item]));
const BUILTIN_ENUMS = new Map([
  [
    "Option",
    {
      kind: "enum",
      name: "Option",
      detail: "enum Option[T]",
      documentation: "Optional values with `Some(T)` and `None`.",
      variants: [
        {
          kind: "variant",
          name: "Some",
          returnType: "Option",
          payloadType: "T",
          detail: "Some(T) -> Option"
        },
        {
          kind: "variant",
          name: "None",
          returnType: "Option",
          payloadType: null,
          detail: "None -> Option"
        }
      ]
    }
  ],
  [
    "Result",
    {
      kind: "enum",
      name: "Result",
      detail: "enum Result[T, E]",
      documentation: "Success-or-error values with `Ok(T)` and `Err(E)`.",
      variants: [
        {
          kind: "variant",
          name: "Ok",
          returnType: "Result",
          payloadType: "T",
          detail: "Ok(T) -> Result"
        },
        {
          kind: "variant",
          name: "Err",
          returnType: "Result",
          payloadType: "E",
          detail: "Err(E) -> Result"
        }
      ]
    }
  ],
  [
    "SendError",
    {
      kind: "enum",
      name: "SendError",
      detail: "enum SendError[T]",
      documentation: "Queue send failures that preserve the unsent value.",
      variants: [
        {
          kind: "variant",
          name: "Closed",
          returnType: "SendError",
          payloadType: "T",
          detail: "Closed(T) -> SendError"
        }
      ]
    }
  ]
]);

function analyzeDocument(text) {
  const lines = text.replace(/\r\n/g, "\n").split("\n");
  const moduleInfo = {
    classes: new Map(),
    enums: new Map(),
    functions: new Map(),
    methods: [],
    topLevelBindings: new Map(),
    diagnostics: [],
    lines
  };

  for (let i = 0; i < lines.length; i += 1) {
    const rawLine = lines[i];
    const trimmed = rawLine.trim();
    if (!trimmed || trimmed.startsWith("#")) {
      continue;
    }

    const indent = countIndent(rawLine);
    if (indent !== 0) {
      continue;
    }

    const classMatch = trimmed.match(/^(?:copy\s+)?class\s+([A-Z][A-Za-z0-9_]*)(?:\[[^\]]+\])?\s*:/);
    if (classMatch) {
      const parsed = parseClass(lines, i, indent, moduleInfo);
      registerTopLevelSymbol(moduleInfo, moduleInfo.classes, parsed.classInfo, "class");
      i = parsed.endLine;
      continue;
    }

    const enumMatch = trimmed.match(/^enum\s+([A-Z][A-Za-z0-9_]*)(?:\[[^\]]+\])?\s*:/);
    if (enumMatch) {
      const parsed = parseEnum(lines, i, indent, moduleInfo);
      registerTopLevelSymbol(moduleInfo, moduleInfo.enums, parsed.enumInfo, "enum");
      i = parsed.endLine;
      continue;
    }

    const functionMatch = trimmed.match(/^def\s+([a-zA-Z_][A-Za-z0-9_]*)(?:\[[^\]]+\])?\s*\(/);
    if (functionMatch) {
      const parsed = parseFunctionSignature(lines, i, indent);
      registerTopLevelSymbol(moduleInfo, moduleInfo.functions, parsed.functionInfo, "function");
      i = parsed.endLine;
    }
  }

  for (const functionInfo of allCallableInfos(moduleInfo)) {
    populateFunctionLocals(functionInfo, lines, moduleInfo);
  }

  populateTopLevelBindings(moduleInfo);
  collectDiagnostics(moduleInfo);
  return moduleInfo;
}

function registerTopLevelSymbol(moduleInfo, registry, symbol, kind) {
  const existing = registry.get(symbol.name);
  if (existing) {
    moduleInfo.diagnostics.push(
      makeDiagnostic(
        symbol.line,
        symbol.startCharacter,
        symbol.endCharacter,
        `duplicate ${kind} \`${symbol.name}\``
      )
    );
    return;
  }
  registry.set(symbol.name, symbol);
}

function parseClass(lines, startLine, indent, moduleInfo) {
  const rawLine = lines[startLine];
  const trimmed = rawLine.trim();
  const headerMatch = trimmed.match(/^(?:copy\s+)?class\s+([A-Z][A-Za-z0-9_]*)(?:\[[^\]]+\])?\s*:/);
  const name = headerMatch[1];
  const startCharacter = rawLine.indexOf(name);
  const classInfo = {
    kind: "class",
    name,
    line: startLine,
    startCharacter,
    endCharacter: startCharacter + name.length,
    fields: [],
    methods: [],
    members: new Map()
  };

  let i = startLine + 1;
  for (; i < lines.length; i += 1) {
    const raw = lines[i];
    const currentTrimmed = raw.trim();
    if (!currentTrimmed || currentTrimmed.startsWith("#")) {
      continue;
    }

    const currentIndent = countIndent(raw);
    if (currentIndent <= indent) {
      break;
    }

    if (currentIndent !== indent + 4) {
      continue;
    }

    const methodMatch = currentTrimmed.match(
      /^def\s+([a-zA-Z_][A-Za-z0-9_]*)\s*\((.*)\)(?:\s*->\s*([^:]+))?\s*:/
    );
    if (methodMatch) {
      const parsed = parseMethodSignature(lines, i, currentIndent, classInfo.name);
      registerMember(moduleInfo, classInfo, parsed.memberSymbol);
      moduleInfo.methods.push(parsed.methodInfo);
      i = parsed.endLine;
      continue;
    }

    const fieldMatch = currentTrimmed.match(
      /^(?:public\s+)?([a-zA-Z_][A-Za-z0-9_]*)\s*:\s*([^=]+?)(?:\s*=\s*.+)?$/
    );
    if (fieldMatch) {
      const fieldName = fieldMatch[1];
      const fieldSymbol = {
        kind: "field",
        name: fieldName,
        type: normalizeType(fieldMatch[2]),
        detail: `${fieldName}: ${normalizeType(fieldMatch[2])}`,
        line: i,
        startCharacter: raw.indexOf(fieldName),
        endCharacter: raw.indexOf(fieldName) + fieldName.length
      };
      registerMember(moduleInfo, classInfo, fieldSymbol);
    }
  }

  return { classInfo, endLine: i - 1 };
}

function parseEnum(lines, startLine, indent, moduleInfo) {
  const rawLine = lines[startLine];
  const trimmed = rawLine.trim();
  const headerMatch = trimmed.match(/^enum\s+([A-Z][A-Za-z0-9_]*)(?:\[[^\]]+\])?\s*:/);
  const name = headerMatch[1];
  const startCharacter = rawLine.indexOf(name);
  const enumInfo = {
    kind: "enum",
    name,
    line: startLine,
    startCharacter,
    endCharacter: startCharacter + name.length,
    variants: [],
    members: new Map()
  };

  let i = startLine + 1;
  for (; i < lines.length; i += 1) {
    const raw = lines[i];
    const currentTrimmed = raw.trim();
    if (!currentTrimmed || currentTrimmed.startsWith("#")) {
      continue;
    }

    const currentIndent = countIndent(raw);
    if (currentIndent <= indent) {
      break;
    }

    if (currentIndent !== indent + 4) {
      continue;
    }

    const variantMatch = currentTrimmed.match(/^([A-Z][A-Za-z0-9_]*)(?:\(([^)]+)\))?$/);
    if (!variantMatch) {
      continue;
    }

    const variantName = variantMatch[1];
    const payloadType = variantMatch[2] ? normalizeType(variantMatch[2]) : null;
    const variantSymbol = {
      kind: "variant",
      name: variantName,
      returnType: name,
      payloadType,
      detail: payloadType
        ? `${variantName}(${payloadType}) -> ${name}`
        : `${variantName} -> ${name}`,
      line: i,
      startCharacter: raw.indexOf(variantName),
      endCharacter: raw.indexOf(variantName) + variantName.length
    };

    const existing = enumInfo.members.get(variantName);
    if (existing) {
      moduleInfo.diagnostics.push(
        makeDiagnostic(
          i,
          variantSymbol.startCharacter,
          variantSymbol.endCharacter,
          `duplicate variant \`${variantName}\` in enum \`${name}\``
        )
      );
      continue;
    }

    enumInfo.members.set(variantName, variantSymbol);
    enumInfo.variants.push(variantSymbol);
  }

  return { enumInfo, endLine: i - 1 };
}

function registerMember(moduleInfo, classInfo, symbol) {
  const existing = classInfo.members.get(symbol.name);
  if (existing) {
    moduleInfo.diagnostics.push(
      makeDiagnostic(
        symbol.line,
        symbol.startCharacter,
        symbol.endCharacter,
        `duplicate member \`${symbol.name}\` in class \`${classInfo.name}\``
      )
    );
    return;
  }

  classInfo.members.set(symbol.name, symbol);
  if (symbol.kind === "field") {
    classInfo.fields.push(symbol);
  } else {
    classInfo.methods.push(symbol);
  }
}

function parseFunctionSignature(lines, startLine, indent) {
  const rawLine = lines[startLine];
  const trimmed = rawLine.trim();
  const headerMatch = trimmed.match(
    /^def\s+([a-zA-Z_][A-Za-z0-9_]*)(?:\[[^\]]+\])?\s*\((.*)\)(?:\s*->\s*([^:]+))?\s*:/
  );
  const name = headerMatch[1];
  const params = parseCallableParams(rawLine, headerMatch[2], startLine, null);
  const functionInfo = {
    kind: "function",
    name,
    params,
    returnType: normalizeType(headerMatch[3] || "None"),
    detail: formatFunctionDetail(name, params.map((param) => param.type), headerMatch[3] || "None"),
    locals: new Map(),
    line: startLine,
    startCharacter: rawLine.indexOf(name),
    endCharacter: rawLine.indexOf(name) + name.length,
    endLine: startLine,
    indent
  };

  for (const param of params) {
    functionInfo.locals.set(param.name, {
      kind: "param",
      name: param.name,
      type: param.type,
      detail: `${param.name}: ${param.type}`,
      line: param.line,
      startCharacter: param.startCharacter,
      endCharacter: param.endCharacter
    });
  }

  let i = startLine + 1;
  for (; i < lines.length; i += 1) {
    const raw = lines[i];
    const currentTrimmed = raw.trim();
    if (!currentTrimmed || currentTrimmed.startsWith("#")) {
      continue;
    }
    const currentIndent = countIndent(raw);
    if (currentIndent <= indent) {
      break;
    }
  }

  functionInfo.endLine = i - 1;
  return { functionInfo, endLine: i - 1 };
}

function parseMethodSignature(lines, startLine, indent, className) {
  const rawLine = lines[startLine];
  const trimmed = rawLine.trim();
  const headerMatch = trimmed.match(
    /^def\s+([a-zA-Z_][A-Za-z0-9_]*)(?:\[[^\]]+\])?\s*\((.*)\)(?:\s*->\s*([^:]+))?\s*:/
  );
  const name = headerMatch[1];
  const params = parseCallableParams(rawLine, headerMatch[2], startLine, className);
  const explicitParams = params.filter((param) => param.name !== "self");
  const methodInfo = {
    kind: "method",
    owner: className,
    name,
    params,
    returnType: normalizeType(headerMatch[3] || "None"),
    detail: formatFunctionDetail(name, explicitParams.map((param) => param.type), headerMatch[3] || "None"),
    locals: new Map(),
    line: startLine,
    startCharacter: rawLine.indexOf(name),
    endCharacter: rawLine.indexOf(name) + name.length,
    endLine: startLine,
    indent
  };

  for (const param of params) {
    methodInfo.locals.set(param.name, {
      kind: "param",
      name: param.name,
      type: param.type,
      detail: `${param.name}: ${param.type}`,
      line: param.line,
      startCharacter: param.startCharacter,
      endCharacter: param.endCharacter
    });
  }

  let i = startLine + 1;
  for (; i < lines.length; i += 1) {
    const raw = lines[i];
    const currentTrimmed = raw.trim();
    if (!currentTrimmed || currentTrimmed.startsWith("#")) {
      continue;
    }
    const currentIndent = countIndent(raw);
    if (currentIndent <= indent) {
      break;
    }
  }

  methodInfo.endLine = i - 1;
  return {
    methodInfo,
    memberSymbol: {
      kind: "method",
      name,
      returnType: methodInfo.returnType,
      detail: methodInfo.detail,
      line: methodInfo.line,
      startCharacter: methodInfo.startCharacter,
      endCharacter: methodInfo.endCharacter
    },
    endLine: i - 1
  };
}

function parseCallableParams(rawLine, rawParams, line, selfType) {
  if (!rawParams.trim()) {
    return [];
  }

  const params = [];
  const openParen = rawLine.indexOf("(");
  const paramsOffset = openParen >= 0 ? openParen + 1 : 0;
  for (const segment of splitTopLevelCommaSegments(rawParams)) {
    const trimmed = segment.text.trim();
    if (!trimmed) {
      continue;
    }

    const receiverMatch = trimmed.match(
      /^(?:borrow(?:\s+mut)?(?:\[[A-Za-z_][A-Za-z0-9_]*\])?\s+)?self$/
    );
    if (receiverMatch && selfType) {
      const selfOffset = segment.start + trimmed.indexOf("self");
      params.push({
        name: "self",
        type: selfType,
        line,
        startCharacter: paramsOffset + selfOffset,
        endCharacter: paramsOffset + selfOffset + 4
      });
      continue;
    }

    const [namePart, typePart] = splitTopLevelColon(trimmed);
    if (!namePart || !typePart) {
      continue;
    }

    const name = namePart.trim();
    if (!/^[A-Za-z_][A-Za-z0-9_]*$/.test(name)) {
      continue;
    }

    const rawType = stripTopLevelDefaultValue(typePart);
    const nameOffset = segment.start + trimmed.indexOf(name);
    params.push({
      name,
      type: normalizeParamType(rawType),
      line,
      startCharacter: paramsOffset + nameOffset,
      endCharacter: paramsOffset + nameOffset + name.length
    });
  }
  return params;
}

function parseParamTypes(rawParams) {
  return parseCallableParams(`(${rawParams})`, rawParams, 0, null).map((param) => param.type);
}

function normalizeParamType(rawType) {
  return normalizeType(rawType).replace(
    /^borrow(?: mut)?(?:\[[A-Za-z_][A-Za-z0-9_]*\])?\s+/,
    ""
  );
}

function populateFunctionLocals(functionInfo, lines, moduleInfo) {
  for (let i = functionInfo.line + 1; i <= functionInfo.endLine; i += 1) {
    const rawLine = lines[i];
    if (typeof rawLine !== "string") {
      continue;
    }

    const trimmed = rawLine.trim();
    if (!trimmed || trimmed.startsWith("#")) {
      continue;
    }

    const forBindingMatch = trimmed.match(
      /^for\s+([a-zA-Z_][A-Za-z0-9_]*)\s+in\s+(.+)\s*:\s*$/
    );
    if (forBindingMatch) {
      const bindingName = forBindingMatch[1];
      if (!functionInfo.locals.has(bindingName)) {
        functionInfo.locals.set(bindingName, {
          kind: "local",
          name: bindingName,
          type: inferForBindingType(forBindingMatch[2], moduleInfo, functionInfo) || "Unknown",
          detail: `${bindingName}: ${inferForBindingType(forBindingMatch[2], moduleInfo, functionInfo) || "Unknown"}`,
          line: i,
          startCharacter: rawLine.indexOf(bindingName),
          endCharacter: rawLine.indexOf(bindingName) + bindingName.length
        });
      }
      continue;
    }

    const withBindingMatch = trimmed.match(
      /^with\s+([a-zA-Z_][A-Za-z0-9_]*)\s*=\s*(.+)\s*:\s*$/
    );
    if (withBindingMatch) {
      const bindingName = withBindingMatch[1];
      if (!functionInfo.locals.has(bindingName)) {
        const inferredType = inferExpressionType(withBindingMatch[2], moduleInfo, functionInfo) || "Unknown";
        functionInfo.locals.set(bindingName, {
          kind: "local",
          name: bindingName,
          type: inferredType,
          detail: `${bindingName}: ${inferredType}`,
          line: i,
          startCharacter: rawLine.indexOf(bindingName),
          endCharacter: rawLine.indexOf(bindingName) + bindingName.length
        });
      }
      continue;
    }

    const withAsBindingMatch = trimmed.match(
      /^with\s+(.+)\s+as\s+([a-zA-Z_][A-Za-z0-9_]*)\s*:\s*$/
    );
    if (withAsBindingMatch) {
      const bindingName = withAsBindingMatch[2];
      if (!functionInfo.locals.has(bindingName)) {
        const inferredType =
          inferExpressionType(withAsBindingMatch[1], moduleInfo, functionInfo) || "Unknown";
        functionInfo.locals.set(bindingName, {
          kind: "local",
          name: bindingName,
          type: inferredType,
          detail: `${bindingName}: ${inferredType}`,
          line: i,
          startCharacter: rawLine.indexOf(bindingName),
          endCharacter: rawLine.indexOf(bindingName) + bindingName.length
        });
      }
      continue;
    }

    const selectBindingMatch = trimmed.match(
      /^case\s+([a-zA-Z_][A-Za-z0-9_]*)\s*=\s*(.+)\s*:\s*$/
    );
    if (selectBindingMatch) {
      const bindingName = selectBindingMatch[1];
      if (!functionInfo.locals.has(bindingName)) {
        const inferredType =
          inferExpressionType(selectBindingMatch[2], moduleInfo, functionInfo) || "Unknown";
        functionInfo.locals.set(bindingName, {
          kind: "local",
          name: bindingName,
          type: inferredType,
          detail: `${bindingName}: ${inferredType}`,
          line: i,
          startCharacter: rawLine.indexOf(bindingName),
          endCharacter: rawLine.indexOf(bindingName) + bindingName.length
        });
      }
      continue;
    }

    const caseBindingMatch = trimmed.match(
      /^case\s+(?:[A-Z][A-Za-z0-9_]*\.)?[A-Z][A-Za-z0-9_]*\(([a-zA-Z_][A-Za-z0-9_]*)\)\s*:/
    );
    if (caseBindingMatch) {
      const bindingName = caseBindingMatch[1];
      if (!functionInfo.locals.has(bindingName)) {
        const inferredType = inferCaseBindingType(trimmed, moduleInfo) || "Unknown";
        functionInfo.locals.set(bindingName, {
          kind: "local",
          name: bindingName,
          type: inferredType,
          detail: `${bindingName}: ${inferredType}`,
          line: i,
          startCharacter: rawLine.indexOf(bindingName),
          endCharacter: rawLine.indexOf(bindingName) + bindingName.length
        });
      }
      continue;
    }

    const assignMatch = trimmed.match(
      /^(?:mut\s+)?([a-zA-Z_][A-Za-z0-9_]*)(?:\s*:\s*([^=]+))?\s*(?:=|\+=|-=|\*=|\/=|%=)\s*(.+)$/
    );
    if (!assignMatch) {
      continue;
    }

    const name = assignMatch[1];
    if (functionInfo.locals.has(name)) {
      continue;
    }

    const annotation = assignMatch[2] ? normalizeType(assignMatch[2]) : null;
    const expression = assignMatch[3].trim();
    const inferredType = annotation || inferExpressionType(expression, moduleInfo, functionInfo);
    if (!inferredType) {
      continue;
    }

    functionInfo.locals.set(name, {
      kind: "local",
      name,
      type: inferredType,
      detail: `${name}: ${inferredType}`,
      line: i,
      startCharacter: rawLine.indexOf(name),
      endCharacter: rawLine.indexOf(name) + name.length
    });
  }
}

function populateTopLevelBindings(moduleInfo) {
  const ranges = collectTopLevelStatementRanges(moduleInfo);
  for (const range of ranges) {
    for (let line = range.startLine; line <= range.endLine; line += 1) {
      const rawLine = moduleInfo.lines[line];
      if (typeof rawLine !== "string") {
        continue;
      }

      const trimmed = rawLine.trim();
      if (!trimmed || trimmed.startsWith("#")) {
        continue;
      }

      const assignMatch = trimmed.match(
        /^(?:mut\s+)?([a-zA-Z_][A-Za-z0-9_]*)(?:\s*:\s*([^=]+))?\s*(?:=|\+=|-=|\*=|\/=|%=)\s*(.+)$/
      );
      if (!assignMatch) {
        continue;
      }

      const name = assignMatch[1];
      if (moduleInfo.topLevelBindings.has(name)) {
        continue;
      }

      const annotation = assignMatch[2] ? normalizeType(assignMatch[2]) : null;
      const expression = assignMatch[3].trim();
      const inferredType = annotation || inferExpressionType(expression, moduleInfo, null);
      if (!inferredType) {
        continue;
      }

      moduleInfo.topLevelBindings.set(name, {
        kind: "binding",
        name,
        type: inferredType,
        detail: `${name}: ${inferredType}`,
        line,
        startCharacter: rawLine.indexOf(name),
        endCharacter: rawLine.indexOf(name) + name.length
      });
    }
  }
}

function collectDiagnostics(moduleInfo) {
  for (const functionInfo of allCallableInfos(moduleInfo)) {
    collectDiagnosticsForBody(moduleInfo, functionInfo.line + 1, functionInfo.endLine, functionInfo);
  }

  const topLevelRanges = collectTopLevelStatementRanges(moduleInfo);
  for (const range of topLevelRanges) {
    collectDiagnosticsForBody(moduleInfo, range.startLine, range.endLine, null);
  }
}

function collectTopLevelStatementRanges(moduleInfo) {
  const occupiedLines = new Set();
  for (const classInfo of moduleInfo.classes.values()) {
    occupiedLines.add(classInfo.line);
    for (const field of classInfo.fields) {
      occupiedLines.add(field.line);
    }
    for (const method of classInfo.methods) {
      occupiedLines.add(method.line);
    }
  }
  for (const enumInfo of moduleInfo.enums.values()) {
    occupiedLines.add(enumInfo.line);
    for (const variant of enumInfo.variants) {
      occupiedLines.add(variant.line);
    }
  }
  for (const functionInfo of moduleInfo.functions.values()) {
    for (let line = functionInfo.line; line <= functionInfo.endLine; line += 1) {
      occupiedLines.add(line);
    }
  }

  const ranges = [];
  let startLine = null;
  for (let line = 0; line < moduleInfo.lines.length; line += 1) {
    const trimmed = moduleInfo.lines[line].trim();
    if (!trimmed || trimmed.startsWith("#") || occupiedLines.has(line)) {
      if (startLine !== null) {
        ranges.push({ startLine, endLine: line - 1 });
        startLine = null;
      }
      continue;
    }
    if (countIndent(moduleInfo.lines[line]) !== 0) {
      continue;
    }
    if (startLine === null) {
      startLine = line;
    }
  }
  if (startLine !== null) {
    ranges.push({ startLine, endLine: moduleInfo.lines.length - 1 });
  }
  return ranges;
}

function collectDiagnosticsForBody(moduleInfo, startLine, endLine, functionInfo) {
  for (let line = startLine; line <= endLine; line += 1) {
    const rawLine = moduleInfo.lines[line];
    if (typeof rawLine !== "string") {
      continue;
    }
    const trimmed = rawLine.trim();
    if (!trimmed || trimmed.startsWith("#")) {
      continue;
    }

    const exprSegments = extractExpressionSegments(rawLine);
    for (const segment of exprSegments) {
      diagnoseExpression(moduleInfo, functionInfo, line, segment.startCharacter, segment.text);
    }
  }
}

function extractExpressionSegments(rawLine) {
  const trimmed = rawLine.trim();
  if (/^(?:copy\s+)?class\b/.test(trimmed) || /^enum\b/.test(trimmed) || /^def\b/.test(trimmed)) {
    return [];
  }
  if (/^else\s*:/.test(trimmed)) {
    return [];
  }
  const selectBindingMatch = trimmed.match(/^case\s+[a-zA-Z_][A-Za-z0-9_]*\s*=\s*(.+)\s*:\s*$/);
  if (selectBindingMatch) {
    return [
      {
        text: selectBindingMatch[1],
        startCharacter: rawLine.indexOf(selectBindingMatch[1])
      }
    ];
  }

  const selectExprMatch = trimmed.match(/^case\s+(.+)\s*:\s*$/);
  if (selectExprMatch) {
    if (
      /^(?:_|[A-Z][A-Za-z0-9_]*(?:\.[A-Z][A-Za-z0-9_]*)?(?:\([a-zA-Z_][A-Za-z0-9_]*\))?)$/.test(
        selectExprMatch[1]
      )
    ) {
      return [];
    }
    return [
      {
        text: selectExprMatch[1],
        startCharacter: rawLine.indexOf(selectExprMatch[1])
      }
    ];
  }

  const segments = [];
  const assignmentMatch = trimmed.match(
    /^(?:mut\s+)?[a-zA-Z_][A-Za-z0-9_]*(?:\s*:\s*[^=]+)?\s*(?:=|\+=|-=|\*=|\/=|%=)\s*(.+)$/
  );
  if (assignmentMatch) {
    segments.push({
      text: assignmentMatch[1],
      startCharacter: rawLine.indexOf(assignmentMatch[1])
    });
    return segments;
  }

  const controlMatch = trimmed.match(/^(?:if|elif|while|match)\s+(.+)\s*:\s*$/);
  if (controlMatch) {
    segments.push({
      text: controlMatch[1],
      startCharacter: rawLine.indexOf(controlMatch[1])
    });
    return segments;
  }

  const forMatch = trimmed.match(/^for\s+[a-zA-Z_][A-Za-z0-9_]*\s+in\s+(.+)\s*:\s*$/);
  if (forMatch) {
    segments.push({
      text: forMatch[1],
      startCharacter: rawLine.indexOf(forMatch[1])
    });
    return segments;
  }

  const withMatch = trimmed.match(/^with\s+[a-zA-Z_][A-Za-z0-9_]*\s*=\s*(.+)\s*:\s*$/);
  if (withMatch) {
    segments.push({
      text: withMatch[1],
      startCharacter: rawLine.indexOf(withMatch[1])
    });
    return segments;
  }

  const withAsMatch = trimmed.match(/^with\s+(.+)\s+as\s+[a-zA-Z_][A-Za-z0-9_]*\s*:\s*$/);
  if (withAsMatch) {
    segments.push({
      text: withAsMatch[1],
      startCharacter: rawLine.indexOf(withAsMatch[1])
    });
    return segments;
  }

  const returnMatch = trimmed.match(/^return\s+(.+)$/);
  if (returnMatch) {
    segments.push({
      text: returnMatch[1],
      startCharacter: rawLine.indexOf(returnMatch[1])
    });
    return segments;
  }

  if (/^(?:break|continue)\b/.test(trimmed)) {
    return [];
  }

  segments.push({
    text: trimmed,
    startCharacter: rawLine.indexOf(trimmed)
  });
  return segments;
}

function diagnoseExpression(moduleInfo, functionInfo, line, baseCharacter, expression) {
  for (const chain of collectIdentifierChains(expression, baseCharacter)) {
    const localStart = chain.startCharacter - baseCharacter;
    const receiver = !chain.text.includes(".")
      ? extractReceiverBeforeIdentifier(expression, localStart)
      : null;

    if (receiver) {
      diagnoseResolvedMemberAccess(moduleInfo, functionInfo, line, chain, receiver);
      continue;
    }

    if (chain.text.includes(".")) {
      diagnoseMemberChain(moduleInfo, functionInfo, line, chain);
    } else {
      diagnoseBareName(moduleInfo, functionInfo, line, chain);
    }
  }
}

function diagnoseResolvedMemberAccess(moduleInfo, functionInfo, line, chain, receiver) {
  const receiverType = inferExpressionType(receiver, moduleInfo, functionInfo);
  if (!receiverType) {
    return;
  }

  const memberSymbol = resolveTypeMember(moduleInfo, receiverType, chain.text);
  if (!memberSymbol) {
    if (isUnresolvedTypeParamType(moduleInfo, receiverType)) {
      return;
    }
    pushDiagnosticIfNew(
      moduleInfo,
      makeDiagnostic(
        line,
        chain.startCharacter,
        chain.endCharacter,
        `type \`${baseTypeName(receiverType)}\` has no member \`${chain.text}\``
      )
    );
  }
}

function diagnoseBareName(moduleInfo, functionInfo, line, chain) {
  const name = chain.text;
  if (
    KEYWORDS.includes(name) ||
    PRIMITIVE_TYPES.has(name) ||
    BUILTIN_FUNCTION_MAP.has(name) ||
    name === "true" ||
    name === "false"
  ) {
    return;
  }

  const symbol = resolveIdentifierSymbol(moduleInfo, functionInfo, name);
  if (!symbol) {
    pushDiagnosticIfNew(
      moduleInfo,
      makeDiagnostic(line, chain.startCharacter, chain.endCharacter, `unknown name \`${name}\``)
    );
  }
}

function diagnoseMemberChain(moduleInfo, functionInfo, line, chain) {
  const parts = chain.text.split(".");
  const base = parts[0];
  const baseSymbol = resolveIdentifierSymbol(moduleInfo, functionInfo, base);
  if (!baseSymbol) {
    pushDiagnosticIfNew(
      moduleInfo,
      makeDiagnostic(
        line,
        chain.startCharacter,
        chain.startCharacter + base.length,
        `unknown name \`${base}\``
      )
    );
    return;
  }

  let currentType = baseSymbol.type || baseSymbol.returnType || baseSymbol.name;
  let offset = chain.startCharacter + base.length + 1;

  for (let index = 1; index < parts.length; index += 1) {
    const memberName = parts[index];
    const memberSymbol = resolveTypeMember(moduleInfo, currentType, memberName);
    if (!memberSymbol) {
      if (isUnresolvedTypeParamType(moduleInfo, currentType)) {
        return;
      }
      pushDiagnosticIfNew(
        moduleInfo,
        makeDiagnostic(
          line,
          offset,
          offset + memberName.length,
          `type \`${baseTypeName(currentType)}\` has no member \`${memberName}\``
        )
      );
      return;
    }
    currentType = memberSymbol.type || memberSymbol.returnType || currentType;
    offset += memberName.length + 1;
  }
}

function pushDiagnosticIfNew(moduleInfo, diagnostic) {
  const exists = moduleInfo.diagnostics.some(
    (existing) =>
      existing.line === diagnostic.line &&
      existing.startCharacter === diagnostic.startCharacter &&
      existing.endCharacter === diagnostic.endCharacter &&
      existing.message === diagnostic.message
  );
  if (!exists) {
    moduleInfo.diagnostics.push(diagnostic);
  }
}

function collectIdentifierChains(expression, baseCharacter) {
  const chains = [];
  let index = 0;

  while (index < expression.length) {
    const ch = expression[index];

    if (ch === "f" && expression[index + 1] === '"') {
      index = skipStringLiteral(expression, index + 2);
      continue;
    }

    if (ch === '"') {
      index = skipStringLiteral(expression, index + 1);
      continue;
    }

    if (isIdentifierStart(ch)) {
      if (index > 0 && /\d/.test(expression[index - 1])) {
        let end = index + 1;
        while (end < expression.length && isIdentifierContinue(expression[end])) {
          end += 1;
        }
        index = end;
        continue;
      }

      const start = index;
      let end = index + 1;
      while (end < expression.length && isIdentifierContinue(expression[end])) {
        end += 1;
      }

      while (expression[end] === "." && isIdentifierStart(expression[end + 1])) {
        end += 1;
        while (end < expression.length && isIdentifierContinue(expression[end])) {
          end += 1;
        }
      }

      const text = expression.slice(start, end);
      if (!isKeywordArgument(expression, start, end)) {
        chains.push({
          text,
          startCharacter: baseCharacter + start,
          endCharacter: baseCharacter + end
        });
      }
      index = end;
      continue;
    }

    index += 1;
  }

  return chains;
}

function isKeywordArgument(expression, start, end) {
  let next = end;
  while (next < expression.length && /\s/.test(expression[next])) {
    next += 1;
  }
  if (expression[next] !== "=" || expression[next + 1] === "=") {
    return false;
  }

  let previous = start - 1;
  while (previous >= 0 && /\s/.test(expression[previous])) {
    previous -= 1;
  }
  return previous < 0 || expression[previous] === "(" || expression[previous] === ",";
}

function skipStringLiteral(expression, index) {
  let current = index;
  while (current < expression.length) {
    if (expression[current] === "\\") {
      current += 2;
      continue;
    }
    if (expression[current] === '"') {
      return current + 1;
    }
    current += 1;
  }
  return current;
}

function completionsForDocument(text, line, character, triggerCharacter) {
  const moduleInfo = analyzeDocument(text);
  const lineText = moduleInfo.lines[line] || "";
  const functionInfo = findEnclosingFunction(moduleInfo, line);

  if (triggerCharacter === ".") {
    const receiver = extractReceiverBeforeDot(lineText, character);
    if (!receiver) {
      return [];
    }
    return memberCompletions(receiver, moduleInfo, functionInfo);
  }

  const completions = [];
  for (const keyword of KEYWORDS) {
    completions.push({
      name: keyword,
      kind: "keyword",
      detail: "Aurora keyword"
    });
  }
  for (const classInfo of moduleInfo.classes.values()) {
    completions.push({
      name: classInfo.name,
      kind: "class",
      detail: "Aurora class"
    });
  }
  for (const enumInfo of moduleInfo.enums.values()) {
    completions.push({
      name: enumInfo.name,
      kind: "enum",
      detail: "Aurora enum"
    });
  }
  for (const builtinEnum of BUILTIN_ENUMS.values()) {
    completions.push({
      name: builtinEnum.name,
      kind: "enum",
      detail: builtinEnum.detail
    });
  }
  for (const functionInfoItem of moduleInfo.functions.values()) {
    completions.push({
      name: functionInfoItem.name,
      kind: "function",
      detail: functionInfoItem.detail
    });
  }
  for (const builtin of BUILTIN_FUNCTIONS) {
    completions.push(builtin);
  }
  return completions;
}

function memberCompletions(receiver, moduleInfo, functionInfo) {
  const typeName = inferExpressionType(receiver, moduleInfo, functionInfo);
  if (!typeName) {
    return [];
  }

  const completions = [];
  const classInfo = moduleInfo.classes.get(baseTypeName(typeName));
  if (classInfo) {
    for (const field of classInfo.fields) {
      completions.push({
        name: field.name,
        kind: "field",
        detail: field.type
      });
    }
    for (const method of classInfo.methods) {
      completions.push({
        name: method.name,
        kind: "method",
        detail: method.detail
      });
    }
  }

  const enumInfo = moduleInfo.enums.get(baseTypeName(typeName));
  if (enumInfo) {
    for (const variant of enumInfo.variants) {
      completions.push({
        name: variant.name,
        kind: "variant",
        detail: variant.detail
      });
    }
  }

  const builtinEnum = BUILTIN_ENUMS.get(baseTypeName(typeName));
  if (builtinEnum) {
    for (const variant of builtinEnum.variants) {
      completions.push({
        name: variant.name,
        kind: "variant",
        detail: variant.detail
      });
    }
  }

  for (const builtin of BUILTIN_MEMBERS[baseTypeName(typeName)] || []) {
    completions.push(builtin);
  }

  return completions;
}

function documentSymbols(text) {
  const moduleInfo = analyzeDocument(text);
  const symbols = [];

  for (const classInfo of moduleInfo.classes.values()) {
    symbols.push({
      name: classInfo.name,
      kind: "class",
      line: classInfo.line,
      startCharacter: classInfo.startCharacter,
      endCharacter: classInfo.endCharacter,
      children: [
        ...classInfo.fields.map((field) => ({
          name: field.name,
          kind: "field",
          line: field.line,
          startCharacter: field.startCharacter,
          endCharacter: field.endCharacter
        })),
        ...classInfo.methods.map((method) => ({
          name: method.name,
          kind: "method",
          line: method.line,
          startCharacter: method.startCharacter,
          endCharacter: method.endCharacter
        }))
      ]
    });
  }

  for (const enumInfo of moduleInfo.enums.values()) {
    symbols.push({
      name: enumInfo.name,
      kind: "enum",
      line: enumInfo.line,
      startCharacter: enumInfo.startCharacter,
      endCharacter: enumInfo.endCharacter,
      children: enumInfo.variants.map((variant) => ({
        name: variant.name,
        kind: "variant",
        line: variant.line,
        startCharacter: variant.startCharacter,
        endCharacter: variant.endCharacter
      }))
    });
  }

  for (const functionInfo of moduleInfo.functions.values()) {
    symbols.push({
      name: functionInfo.name,
      kind: "function",
      line: functionInfo.line,
      startCharacter: functionInfo.startCharacter,
      endCharacter: functionInfo.endCharacter,
      children: []
    });
  }

  return symbols;
}

function hoverForPosition(text, line, character) {
  const moduleInfo = analyzeDocument(text);
  const symbol = resolveSymbolAtPosition(moduleInfo, line, character);
  if (!symbol || !symbol.hover) {
    return null;
  }

  return {
    range: {
      start: { line: symbol.line, character: symbol.startCharacter },
      end: { line: symbol.line, character: symbol.endCharacter }
    },
    value: symbol.hover
  };
}

function definitionForPosition(text, line, character) {
  const moduleInfo = analyzeDocument(text);
  const symbol = resolveSymbolAtPosition(moduleInfo, line, character);
  if (!symbol || symbol.builtin) {
    return null;
  }

  return {
    line: symbol.line,
    startCharacter: symbol.startCharacter,
    endCharacter: symbol.endCharacter
  };
}

function diagnosticsForDocument(text) {
  return analyzeDocument(text).diagnostics;
}

function resolveSymbolAtPosition(moduleInfo, line, character) {
  const lineText = moduleInfo.lines[line] || "";
  const token = findIdentifierAtPosition(lineText, character);
  if (!token) {
    return null;
  }

  const functionInfo = findEnclosingFunction(moduleInfo, line);
  if (token.receiver) {
    const symbol = resolveMemberSymbol(moduleInfo, functionInfo, token.receiver, token.name);
    if (!symbol) {
      return null;
    }
    return {
      ...symbol,
      line: symbol.line ?? line,
      startCharacter: symbol.startCharacter ?? token.startCharacter,
      endCharacter: symbol.endCharacter ?? token.endCharacter
    };
  }

  const symbol = resolveIdentifierSymbol(moduleInfo, functionInfo, token.name);
  if (!symbol) {
    return null;
  }
  return {
    ...symbol,
    line: symbol.line ?? line,
    startCharacter: symbol.startCharacter ?? token.startCharacter,
    endCharacter: symbol.endCharacter ?? token.endCharacter
  };
}

function resolveIdentifierSymbol(moduleInfo, functionInfo, name) {
  if (functionInfo && functionInfo.locals.has(name)) {
    const symbol = functionInfo.locals.get(name);
    return {
      ...symbol,
      hover: formatHover(symbol.kind, symbol.name, symbol.type || symbol.returnType || "None")
    };
  }

  if (!functionInfo && moduleInfo.topLevelBindings.has(name)) {
    const symbol = moduleInfo.topLevelBindings.get(name);
    return {
      ...symbol,
      hover: formatHover("binding", symbol.name, symbol.type || "None")
    };
  }

  if (moduleInfo.functions.has(name)) {
    const symbol = moduleInfo.functions.get(name);
    return {
      ...symbol,
      hover: formatCallableHover("function", symbol.name, symbol.params.map((param) => `${param.name}: ${param.type}`), symbol.returnType)
    };
  }

  if (moduleInfo.classes.has(name)) {
    const symbol = moduleInfo.classes.get(name);
    return {
      ...symbol,
      type: symbol.name,
      hover: formatClassHover(symbol)
    };
  }

  if (moduleInfo.enums.has(name)) {
    const symbol = moduleInfo.enums.get(name);
    return {
      ...symbol,
      type: symbol.name,
      hover: formatEnumHover(symbol)
    };
  }

  if (BUILTIN_ENUMS.has(name)) {
    const symbol = BUILTIN_ENUMS.get(name);
    return {
      ...symbol,
      type: symbol.name,
      line: 0,
      startCharacter: 0,
      endCharacter: 0,
      builtin: true,
      hover: formatBuiltinEnumHover(symbol)
    };
  }

  if (BUILTIN_FUNCTION_MAP.has(name)) {
    const builtin = BUILTIN_FUNCTION_MAP.get(name);
    return {
      ...builtin,
      line: 0,
      startCharacter: 0,
      endCharacter: 0,
      builtin: true,
      hover: `\`\`\`aurora\n${builtin.detail}\n\`\`\`\n${builtin.documentation}`
    };
  }

  return null;
}

function resolveMemberSymbol(moduleInfo, functionInfo, receiver, memberName) {
  const receiverType = inferExpressionType(receiver, moduleInfo, functionInfo);
  if (!receiverType) {
    return null;
  }

  const member = resolveTypeMember(moduleInfo, receiverType, memberName);
  if (!member) {
    return null;
  }

  const kind = member.kind === "field" ? "field" : "method";
  if (member.kind === "variant") {
    return {
      ...member,
      builtin: false,
      hover: formatVariantHover(member, receiverType)
    };
  }
  const memberType = member.type || member.returnType || parseBuiltinDetailReturnType(member.detail) || "None";
  const builtin = typeof member.line !== "number";
  return {
    ...member,
    builtin,
    hover:
      kind === "field"
        ? formatHover("field", member.name, memberType)
        : formatCallableHover("method", member.name, [], memberType)
  };
}

function resolveTypeMember(moduleInfo, typeName, memberName) {
  const classInfo = moduleInfo.classes.get(baseTypeName(typeName));
  if (classInfo && classInfo.members.has(memberName)) {
    return classInfo.members.get(memberName);
  }

  const enumInfo = moduleInfo.enums.get(baseTypeName(typeName));
  if (enumInfo && enumInfo.members.has(memberName)) {
    return enumInfo.members.get(memberName);
  }

  const builtinEnum = BUILTIN_ENUMS.get(baseTypeName(typeName));
  if (builtinEnum) {
    return builtinEnum.variants.find((variant) => variant.name === memberName) || null;
  }

  return (BUILTIN_MEMBERS[baseTypeName(typeName)] || []).find((item) => item.name === memberName) || null;
}

function inferExpressionType(expression, moduleInfo, functionInfo) {
  const expr = stripOuterParens(expression.trim());
  const borrowMatch = expr.match(/^borrow(?:\s+mut)?\s+(.+)$/);
  if (borrowMatch) {
    return inferExpressionType(borrowMatch[1], moduleInfo, functionInfo);
  }

  const castMatch = expr.match(/^(.+)\s+as\s+([A-Za-z_][A-Za-z0-9_]*(?:\[[^\]]+\])?)$/);
  if (castMatch) {
    return normalizeType(castMatch[2]);
  }

  const tryMatch = expr.match(/^try\s+(.+)$/);
  if (tryMatch) {
    const innerType = inferExpressionType(tryMatch[1], moduleInfo, functionInfo);
    if (innerType) {
      const resultMatch = innerType.match(/^Result\[(.+),\s*(.+)\]$/);
      if (resultMatch) {
        return normalizeType(resultMatch[1]);
      }
    }
  }

  const detachedSpawnMatch = expr.match(/^spawn\s+detached\s+(.+)$/);
  if (detachedSpawnMatch) {
    return "None";
  }

  const spawnMatch = expr.match(/^spawn\s+(.+)$/);
  if (spawnMatch) {
    const innerType = inferExpressionType(spawnMatch[1], moduleInfo, functionInfo);
    if (innerType) {
      return `Task[${innerType}]`;
    }
  }

  if (/^".*"$/.test(expr)) {
    return "String";
  }
  if (/^\d+\.\d+$/.test(expr)) {
    return "float64";
  }
  if (/^\d+(?:ms|s|m)$/.test(expr)) {
    return "Duration";
  }
  if (/^\d+$/.test(expr)) {
    return "int32";
  }
  if (/^(true|false)$/.test(expr)) {
    return "bool";
  }

  const listMatch = expr.match(/^\[(.*)\]$/);
  if (listMatch) {
    const elements = splitTopLevelCommaSeparated(listMatch[1]);
    if (elements.length === 0) {
      return null;
    }
    const elementType = inferExpressionType(elements[0], moduleInfo, functionInfo);
    return elementType ? `Vec[${elementType}]` : null;
  }

  const setMatch = expr.match(/^Set\{(.*)\}$/);
  if (setMatch) {
    const elements = splitTopLevelCommaSeparated(setMatch[1]);
    if (elements.length === 0) {
      return null;
    }
    const elementType = inferExpressionType(elements[0], moduleInfo, functionInfo);
    return elementType ? `Set[${elementType}]` : null;
  }

  const mapLiteralMatch = expr.match(/^\{(.*)\}$/);
  if (mapLiteralMatch && mapLiteralMatch[1].includes(":")) {
    const entries = splitTopLevelCommaSeparated(mapLiteralMatch[1]);
    if (entries.length === 0) {
      return null;
    }
    const [firstKey, firstValue] = splitTopLevelColon(entries[0]);
    if (!firstKey || !firstValue) {
      return null;
    }
    const keyType = inferExpressionType(firstKey, moduleInfo, functionInfo);
    const valueType = inferExpressionType(firstValue, moduleInfo, functionInfo);
    return keyType && valueType ? `Map[${keyType}, ${valueType}]` : null;
  }

  const indexMatch = expr.match(/^(.+)\[(.+)\]$/);
  if (indexMatch) {
    const receiverType = inferExpressionType(indexMatch[1], moduleInfo, functionInfo);
    if (receiverType) {
      const vecMatch = receiverType.match(/^Vec\[(.+)\]$/);
      if (vecMatch) {
        return normalizeType(vecMatch[1]);
      }
      const mapMatch = receiverType.match(/^Map\[(.+),\s*(.+)\]$/);
      if (mapMatch) {
        return normalizeType(mapMatch[2]);
      }
    }
  }

  const specializedConstructorMatch = expr.match(
    /^([A-Z][A-Za-z0-9_]*)\s*(\[[^\]]+\])\s*\(/
  );
  if (specializedConstructorMatch) {
    return normalizeType(`${specializedConstructorMatch[1]}${specializedConstructorMatch[2]}`);
  }

  const constructorMatch = expr.match(/^([A-Z][A-Za-z0-9_]*)\s*\(/);
  if (constructorMatch) {
    return constructorMatch[1];
  }

  const enumVariantMatch = expr.match(/^([A-Z][A-Za-z0-9_]*)\.([A-Z][A-Za-z0-9_]*)\s*(?:\(|$)/);
  if (enumVariantMatch && moduleInfo.enums.has(enumVariantMatch[1])) {
    return enumVariantMatch[1];
  }
  if (enumVariantMatch && BUILTIN_ENUMS.has(enumVariantMatch[1])) {
    return enumVariantMatch[1];
  }

  const functionMatch = expr.match(/^([a-zA-Z_][A-Za-z0-9_]*)\s*\(/);
  if (functionMatch) {
    if (moduleInfo.functions.has(functionMatch[1])) {
      return moduleInfo.functions.get(functionMatch[1]).returnType;
    }
    if (BUILTIN_FUNCTION_MAP.has(functionMatch[1])) {
      const argsMatch = expr.match(/^[a-zA-Z_][A-Za-z0-9_]*\((.*)\)$/);
      const args = argsMatch ? splitTopLevelCommaSeparated(argsMatch[1]) : [];
      if (["abs", "min", "max"].includes(functionMatch[1])) {
        return args.length > 0
          ? inferExpressionType(args[0], moduleInfo, functionInfo)
          : null;
      }
      if (functionMatch[1] === "sqrt") {
        if (args.length === 0) {
          return null;
        }
        const argType = inferExpressionType(args[0], moduleInfo, functionInfo);
        return argType === "float32" ? "float32" : "float64";
      }
      return parseBuiltinDetailReturnType(BUILTIN_FUNCTION_MAP.get(functionMatch[1]).detail);
    }
  }

  const memberType = inferChainType(expr, moduleInfo, functionInfo);
  if (memberType) {
    return memberType;
  }

  const binaryType = inferBinaryExpressionType(expr, moduleInfo, functionInfo);
  if (binaryType) {
    return binaryType;
  }

  return null;
}

function inferBinaryExpressionType(expression, moduleInfo, functionInfo) {
  const match = expression.match(/(.+?)\s*(==|!=|<=|>=|<|>|[+\-*/%])\s*(.+)/);
  if (!match) {
    return null;
  }

  const leftType = inferExpressionType(match[1], moduleInfo, functionInfo);
  const rightType = inferExpressionType(match[3], moduleInfo, functionInfo);
  if (!leftType || !rightType) {
    return null;
  }

  const operator = match[2];
  if (["==", "!=", "<", "<=", ">", ">="].includes(operator)) {
    return "bool";
  }
  if (leftType === "float64" || rightType === "float64") {
    return "float64";
  }
  if (leftType === rightType) {
    return leftType;
  }
  return null;
}

function inferChainType(expression, moduleInfo, functionInfo) {
  const normalized = expression.replace(/\([^()]*\)/g, "");
  const chain = normalized.match(/^[A-Za-z_][A-Za-z0-9_]*(?:\.[A-Za-z_][A-Za-z0-9_]*)*$/);
  if (!chain) {
    return null;
  }

  const parts = normalized.split(".");
  const symbol = resolveIdentifierSymbol(moduleInfo, functionInfo, parts[0]);
  let currentType = symbol ? symbol.type || symbol.returnType || symbol.name : null;

  if (!currentType) {
    return null;
  }

  for (let i = 1; i < parts.length; i += 1) {
    const memberName = parts[i];
    const member = resolveTypeMember(moduleInfo, currentType, memberName);
    if (!member) {
      return null;
    }
    currentType =
      member.kind === "field"
        ? member.type
        : specializeMemberReturnType(currentType, member) ||
          member.returnType ||
          parseBuiltinDetailReturnType(member.detail) ||
          currentType;
  }

  return currentType;
}

function findEnclosingFunction(moduleInfo, line) {
  let current = null;
  for (const functionInfo of allCallableInfos(moduleInfo)) {
    if (functionInfo.line <= line && line <= functionInfo.endLine) {
      current = functionInfo;
    }
  }
  return current;
}

function extractReceiverBeforeDot(lineText, character) {
  return extractReceiverEndingBefore(lineText, Math.max(0, character));
}

function findIdentifierAtPosition(lineText, character) {
  const regex = /[A-Za-z_][A-Za-z0-9_]*/g;
  let match = regex.exec(lineText);
  while (match) {
    const start = match.index;
    const end = start + match[0].length;
    if (start <= character && character <= end) {
      const receiver = extractReceiverBeforeIdentifier(lineText, start);
      return {
        name: match[0],
        startCharacter: start,
        endCharacter: end,
        receiver
      };
    }
    match = regex.exec(lineText);
  }
  return null;
}

function extractReceiverBeforeIdentifier(lineText, identifierStart) {
  return extractReceiverEndingBefore(lineText, identifierStart);
}

function extractReceiverEndingBefore(lineText, endIndexExclusive) {
  let index = endIndexExclusive - 1;
  while (index >= 0 && /\s/.test(lineText[index])) {
    index -= 1;
  }
  if (index < 0 || lineText[index] !== ".") {
    return null;
  }

  index -= 1;
  while (index >= 0 && /\s/.test(lineText[index])) {
    index -= 1;
  }
  if (index < 0) {
    return null;
  }

  const end = index + 1;
  const start = findReceiverStart(lineText, index);
  if (start < 0) {
    return null;
  }

  return lineText.slice(start, end).trim();
}

function findReceiverStart(lineText, index) {
  if (index < 0) {
    return -1;
  }

  if (lineText[index] === ")") {
    let depth = 1;
    let cursor = index - 1;
    while (cursor >= 0) {
      if (lineText[cursor] === ")") {
        depth += 1;
      } else if (lineText[cursor] === "(") {
        depth -= 1;
        if (depth === 0) {
          return cursor;
        }
      }
      cursor -= 1;
    }
    return -1;
  }

  if (isIdentifierContinue(lineText[index])) {
    let cursor = index;
    while (cursor >= 0) {
      const ch = lineText[cursor];
      if (isIdentifierContinue(ch) || ch === ".") {
        cursor -= 1;
        continue;
      }
      break;
    }
    return cursor + 1;
  }

  return -1;
}

function formatHover(kind, name, typeName) {
  return `\`\`\`aurora\n${kind} ${name}: ${typeName}\n\`\`\``;
}

function formatCallableHover(kind, name, params, returnType) {
  const renderedParams = params.join(", ");
  return `\`\`\`aurora\n${kind} ${name}(${renderedParams}) -> ${normalizeType(returnType || "None")}\n\`\`\``;
}

function formatClassHover(classInfo) {
  const fields = classInfo.fields.map((field) => `${field.name}: ${field.type}`).join("\n");
  return `\`\`\`aurora\nclass ${classInfo.name}\n${fields}\n\`\`\``.trim();
}

function formatEnumHover(enumInfo) {
  const variants = enumInfo.variants
    .map((variant) =>
      variant.payloadType ? `${variant.name}(${variant.payloadType})` : variant.name
    )
    .join("\n");
  return `\`\`\`aurora\nenum ${enumInfo.name}\n${variants}\n\`\`\``.trim();
}

function formatBuiltinEnumHover(enumInfo) {
  const variants = enumInfo.variants
    .map((variant) =>
      variant.payloadType ? `${variant.name}(${variant.payloadType})` : variant.name
    )
    .join("\n");
  return `\`\`\`aurora\nenum ${enumInfo.name}\n${variants}\n\`\`\`\n${enumInfo.documentation}`;
}

function formatVariantHover(variant, receiverType) {
  const baseName = baseTypeName(receiverType || variant.returnType || "Unknown");
  if (variant.payloadType) {
    return `\`\`\`aurora\nvariant ${variant.name}(${variant.payloadType}) -> ${baseName}\n\`\`\``;
  }
  return `\`\`\`aurora\nvariant ${variant.name} -> ${baseName}\n\`\`\``;
}

function formatFunctionDetail(name, paramTypes, returnType) {
  return `${name}(${paramTypes.join(", ")}) -> ${normalizeType(returnType || "None")}`;
}

function makeDiagnostic(line, startCharacter, endCharacter, message) {
  return {
    line,
    startCharacter,
    endCharacter,
    message,
    severity: 1
  };
}

function parseBuiltinDetailReturnType(detail) {
  const match = detail.match(/->\s*([A-Za-z_][A-Za-z0-9_,\[\] ]*)/);
  return match ? normalizeType(match[1]) : null;
}

function specializeMemberReturnType(receiverType, member) {
  const base = baseTypeName(receiverType);
  if (base === "Vec") {
    const match = receiverType.match(/^Vec\[(.+)\]$/);
    if (!match) {
      return parseBuiltinDetailReturnType(member.detail);
    }
    const inner = normalizeType(match[1]);
    if (member.name === "clone") {
      return receiverType;
    }
    if (["pop", "get", "set", "remove"].includes(member.name)) {
      return `Option[${inner}]`;
    }
    return parseBuiltinDetailReturnType(member.detail);
  }

  if (base === "Map") {
    const match = receiverType.match(/^Map\[(.+),\s*(.+)\]$/);
    if (!match) {
      return parseBuiltinDetailReturnType(member.detail);
    }
    const keyType = normalizeType(match[1]);
    const valueType = normalizeType(match[2]);
    if (member.name === "clone") {
      return receiverType;
    }
    if (["get", "set", "remove"].includes(member.name)) {
      return `Option[${valueType}]`;
    }
    if (member.name === "keys") {
      return `Vec[${keyType}]`;
    }
    if (member.name === "values") {
      return `Vec[${valueType}]`;
    }
    if (member.name === "items" || member.name === "entries") {
      return `Vec[MapEntry[${keyType}, ${valueType}]]`;
    }
    return parseBuiltinDetailReturnType(member.detail);
  }

  if (base === "Set") {
    const match = receiverType.match(/^Set\[(.+)\]$/);
    if (!match) {
      return parseBuiltinDetailReturnType(member.detail);
    }
    const inner = normalizeType(match[1]);
    if (member.name === "clone") {
      return receiverType;
    }
    if (["contains", "insert", "remove"].includes(member.name)) {
      return "bool";
    }
    return member.name === "len" ? "int32" : parseBuiltinDetailReturnType(member.detail) || inner;
  }

  if (base === "MapEntry") {
    const match = receiverType.match(/^MapEntry\[(.+),\s*(.+)\]$/);
    if (!match) {
      return member.type || parseBuiltinDetailReturnType(member.detail);
    }
    if (member.name === "key") {
      return normalizeType(match[1]);
    }
    if (member.name === "value") {
      return normalizeType(match[2]);
    }
  }

  if (base === "Queue") {
    const match = receiverType.match(/^Queue\[(.+)\]$/);
    if (!match) {
      return parseBuiltinDetailReturnType(member.detail);
    }
    const inner = normalizeType(match[1]);
    if (member.name === "get") {
      return `Option[${inner}]`;
    }
    if (member.name === "put") {
      return `Result[None, SendError[${inner}]]`;
    }
    return "None";
  }

  if (base === "Task") {
    const match = receiverType.match(/^Task\[(.+)\]$/);
    if (!match) {
      return parseBuiltinDetailReturnType(member.detail);
    }
    if (member.name === "result") {
      return normalizeType(match[1]);
    }
  }

  if (base === "TaskGroup") {
    if (member.name === "cancel") {
      return "None";
    }
    return parseBuiltinDetailReturnType(member.detail);
  }

  return parseBuiltinDetailReturnType(member.detail);
}

function normalizeType(rawType) {
  return rawType.trim().replace(/\s+/g, " ");
}

function baseTypeName(typeName) {
  return typeName.replace(/\[.*\]$/, "").trim();
}

function isUnresolvedTypeParamType(moduleInfo, typeName) {
  const base = baseTypeName(typeName);
  if (!/^[A-Z][A-Za-z0-9_]*$/.test(base)) {
    return false;
  }
  if (PRIMITIVE_TYPES.has(base) || BUILTIN_ENUMS.has(base) || BUILTIN_MEMBERS[base]) {
    return false;
  }
  return !moduleInfo.classes.has(base) && !moduleInfo.enums.has(base);
}

function inferCaseBindingType(trimmed, moduleInfo) {
  const match = trimmed.match(
    /^case\s+(?:([A-Z][A-Za-z0-9_]*)\.)?([A-Z][A-Za-z0-9_]*)\([a-zA-Z_][A-Za-z0-9_]*\)\s*:/
  );
  if (!match) {
    return null;
  }
  if (match[1]) {
    const enumInfo = moduleInfo.enums.get(match[1]);
    if (enumInfo) {
      const variant = enumInfo.members.get(match[2]);
      return variant ? variant.payloadType : null;
    }
    const builtinEnum = BUILTIN_ENUMS.get(match[1]);
    if (!builtinEnum) {
      return null;
    }
    const variant = builtinEnum.variants.find((item) => item.name === match[2]);
    return variant ? variant.payloadType : null;
  }

  for (const enumInfo of moduleInfo.enums.values()) {
    const variant = enumInfo.members.get(match[2]);
    if (variant && variant.payloadType) {
      return variant.payloadType;
    }
  }
  for (const builtinEnum of BUILTIN_ENUMS.values()) {
    const variant = builtinEnum.variants.find((item) => item.name === match[2]);
    if (variant && variant.payloadType) {
      return variant.payloadType;
    }
  }
  return null;
}

function inferForBindingType(iterableExpression, moduleInfo, functionInfo) {
  const iterableType = inferExpressionType(iterableExpression, moduleInfo, functionInfo);
  if (iterableType === "Range") {
    return "int32";
  }
  const vecMatch = iterableType ? iterableType.match(/^Vec\[(.+)\]$/) : null;
  if (vecMatch) {
    return normalizeType(vecMatch[1]);
  }
  const queueMatch = iterableType ? iterableType.match(/^Queue\[(.+)\]$/) : null;
  if (queueMatch) {
    return normalizeType(queueMatch[1]);
  }
  const setMatch = iterableType ? iterableType.match(/^Set\[(.+)\]$/) : null;
  if (setMatch) {
    return normalizeType(setMatch[1]);
  }
  return null;
}

function stripOuterParens(expression) {
  if (expression.startsWith("(") && expression.endsWith(")")) {
    return expression.slice(1, -1).trim();
  }
  return expression;
}

function splitTopLevelCommaSeparated(text) {
  return splitTopLevelCommaSegments(text).map((segment) => segment.text.trim());
}

function splitTopLevelCommaSegments(text) {
  const parts = [];
  let current = "";
  let parenDepth = 0;
  let bracketDepth = 0;
  let braceDepth = 0;
  let inString = false;
  let segmentStart = 0;

  for (let index = 0; index < text.length; index += 1) {
    const ch = text[index];
    if (inString) {
      current += ch;
      if (ch === "\\") {
        index += 1;
        if (index < text.length) {
          current += text[index];
        }
        continue;
      }
      if (ch === '"') {
        inString = false;
      }
      continue;
    }

    if (ch === '"') {
      inString = true;
      current += ch;
      continue;
    }
    if (ch === "(") {
      parenDepth += 1;
      current += ch;
      continue;
    }
    if (ch === ")") {
      parenDepth = Math.max(0, parenDepth - 1);
      current += ch;
      continue;
    }
    if (ch === "[") {
      bracketDepth += 1;
      current += ch;
      continue;
    }
    if (ch === "]") {
      bracketDepth = Math.max(0, bracketDepth - 1);
      current += ch;
      continue;
    }
    if (ch === "{") {
      braceDepth += 1;
      current += ch;
      continue;
    }
    if (ch === "}") {
      braceDepth = Math.max(0, braceDepth - 1);
      current += ch;
      continue;
    }
    if (ch === "," && parenDepth === 0 && bracketDepth === 0 && braceDepth === 0) {
      const trimmed = current.trim();
      if (trimmed) {
        parts.push({ text: trimmed, start: segmentStart });
      }
      current = "";
      segmentStart = index + 1;
      continue;
    }
    current += ch;
  }

  const trimmed = current.trim();
  if (trimmed) {
    parts.push({ text: trimmed, start: segmentStart });
  }
  return parts;
}

function stripTopLevelDefaultValue(text) {
  let parenDepth = 0;
  let bracketDepth = 0;
  let braceDepth = 0;
  let inString = false;

  for (let index = 0; index < text.length; index += 1) {
    const ch = text[index];
    if (inString) {
      if (ch === "\\") {
        index += 1;
        continue;
      }
      if (ch === '"') {
        inString = false;
      }
      continue;
    }

    if (ch === '"') {
      inString = true;
      continue;
    }
    if (ch === "(") {
      parenDepth += 1;
      continue;
    }
    if (ch === ")") {
      parenDepth = Math.max(0, parenDepth - 1);
      continue;
    }
    if (ch === "[") {
      bracketDepth += 1;
      continue;
    }
    if (ch === "]") {
      bracketDepth = Math.max(0, bracketDepth - 1);
      continue;
    }
    if (ch === "{") {
      braceDepth += 1;
      continue;
    }
    if (ch === "}") {
      braceDepth = Math.max(0, braceDepth - 1);
      continue;
    }
    if (ch === "=" && parenDepth === 0 && bracketDepth === 0 && braceDepth === 0) {
      return text.slice(0, index).trim();
    }
  }

  return text.trim();
}

function splitTopLevelColon(text) {
  let parenDepth = 0;
  let bracketDepth = 0;
  let braceDepth = 0;
  let inString = false;

  for (let index = 0; index < text.length; index += 1) {
    const ch = text[index];
    if (inString) {
      if (ch === "\\") {
        index += 1;
        continue;
      }
      if (ch === '"') {
        inString = false;
      }
      continue;
    }

    if (ch === '"') {
      inString = true;
      continue;
    }
    if (ch === "(") {
      parenDepth += 1;
      continue;
    }
    if (ch === ")") {
      parenDepth = Math.max(0, parenDepth - 1);
      continue;
    }
    if (ch === "[") {
      bracketDepth += 1;
      continue;
    }
    if (ch === "]") {
      bracketDepth = Math.max(0, bracketDepth - 1);
      continue;
    }
    if (ch === "{") {
      braceDepth += 1;
      continue;
    }
    if (ch === "}") {
      braceDepth = Math.max(0, braceDepth - 1);
      continue;
    }
    if (ch === ":" && parenDepth === 0 && bracketDepth === 0 && braceDepth === 0) {
      return [text.slice(0, index).trim(), text.slice(index + 1).trim()];
    }
  }

  return [null, null];
}

function countIndent(line) {
  let count = 0;
  while (count < line.length && line[count] === " ") {
    count += 1;
  }
  return count;
}

function allCallableInfos(moduleInfo) {
  return [...moduleInfo.functions.values(), ...moduleInfo.methods];
}

function isIdentifierStart(ch) {
  return /[A-Za-z_]/.test(ch);
}

function isIdentifierContinue(ch) {
  return /[A-Za-z0-9_]/.test(ch);
}

module.exports = {
  KEYWORDS,
  _testing: {
    allCallableInfos,
    baseTypeName,
    builtinEnums: () => [...BUILTIN_ENUMS.values()],
    builtinFunctions: () => [...BUILTIN_FUNCTIONS],
    builtinMembersFor: (typeName) => [...(BUILTIN_MEMBERS[typeName] || [])],
    countIndent,
    extractReceiverEndingBefore,
    findReceiverStart,
    formatVariantHover,
    inferCaseBindingType,
    inferForBindingType,
    isUnresolvedTypeParamType,
    isIdentifierContinue,
    isIdentifierStart,
    parseBuiltinDetailReturnType,
    parseParamTypes,
    pushDiagnosticIfNew,
    specializeMemberReturnType,
    stripTopLevelDefaultValue,
    splitTopLevelColon,
    splitTopLevelCommaSeparated
  },
  analyzeDocument,
  completionsForDocument,
  definitionForPosition,
  diagnosticsForDocument,
  documentSymbols,
  hoverForPosition
};
