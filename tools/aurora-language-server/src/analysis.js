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
  "Channel",
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
      detail: "len() -> uintsize",
      documentation: "Returns the number of items in the vector."
    },
    {
      name: "clone",
      kind: "method",
      detail: "clone() -> Vec[T]",
      documentation: "Creates a new vector with cloned contents."
    }
  ],
  Channel: [
    {
      name: "clone",
      kind: "method",
      detail: "clone() -> Channel[T]",
      documentation: "Creates another handle to the same underlying channel."
    },
    {
      name: "send",
      kind: "method",
      detail: "send(value) -> Result[None, SendError[T]]",
      documentation: "Sends a value to the channel or returns `SendError.Closed(value)` if the channel is closed."
    },
    {
      name: "recv",
      kind: "method",
      detail: "recv() -> Option[T]",
      documentation: "Receives the next value from the channel, or `Option.None` when closed."
    },
    {
      name: "close",
      kind: "method",
      detail: "close() -> None",
      documentation: "Closes the channel and wakes blocked receivers."
    }
  ],
  Task: [
    {
      name: "clone",
      kind: "method",
      detail: "clone() -> Task[T]",
      documentation: "Creates another handle to the same spawned task."
    },
    {
      name: "join",
      kind: "method",
      detail: "join() -> T",
      documentation: "Waits for the spawned task to finish and returns its value."
    }
  ],
  TaskGroup: [
    {
      name: "spawn",
      kind: "method",
      detail: "spawn(function, ...) -> Task[T]",
      documentation: "Spawns a child task in the current task group."
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
    name: "channel",
    kind: "function",
    detail: "channel() -> Channel[T]",
    documentation: "Creates a typed channel when the surrounding annotation or expectation provides `T`."
  },
  {
    name: "task_group",
    kind: "function",
    detail: "task_group() -> TaskGroup",
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
      documentation: "Channel send failures that preserve the unsent value.",
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
  const receiverMatch = rawParams.match(/^\s*(?:borrow\s+(?:mut\s+)?)?self(?:\s*,\s*|$)/);
  if (receiverMatch && selfType) {
    params.push({
      name: "self",
      type: selfType,
      line,
      startCharacter: paramsOffset + receiverMatch[0].indexOf("self"),
      endCharacter: paramsOffset + receiverMatch[0].indexOf("self") + 4
    });
  }
  const pattern = /(?:borrow\s+(?:mut\s+)?)?([a-zA-Z_][A-Za-z0-9_]*)\s*:\s*((?:borrow\s+(?:mut\s+)?)?[^=,\)]+(?:\[[^\]]+\])?)/g;
  let match = pattern.exec(rawParams);
  while (match) {
    if (match[1] === "self" && selfType) {
      match = pattern.exec(rawParams);
      continue;
    }
    params.push({
      name: match[1],
      type: normalizeParamType(match[2]),
      line,
      startCharacter: paramsOffset + match.index,
      endCharacter: paramsOffset + match.index + match[1].length
    });
    match = pattern.exec(rawParams);
  }
  return params;
}

function parseParamTypes(rawParams) {
  return parseCallableParams(`(${rawParams})`, rawParams, 0, null).map((param) => param.type);
}

function normalizeParamType(rawType) {
  return normalizeType(rawType).replace(/^borrow(?: mut)?\s+/, "");
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
      /^case\s+[A-Z][A-Za-z0-9_]*\.[A-Z][A-Za-z0-9_]*\(([a-zA-Z_][A-Za-z0-9_]*)\)\s*:/
    );
    if (caseBindingMatch) {
      const bindingName = caseBindingMatch[1];
      if (!functionInfo.locals.has(bindingName)) {
        functionInfo.locals.set(bindingName, {
          kind: "local",
          name: bindingName,
          type: inferCaseBindingType(trimmed, moduleInfo) || "Unknown",
          detail: `${bindingName}: ${inferCaseBindingType(trimmed, moduleInfo) || "Unknown"}`,
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
  if (selectExprMatch && !/^[A-Z][A-Za-z0-9_]*\.[A-Z][A-Za-z0-9_]*(?:\(|$)/.test(selectExprMatch[1])) {
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
  if (base === "Channel") {
    const match = receiverType.match(/^Channel\[(.+)\]$/);
    if (!match) {
      return parseBuiltinDetailReturnType(member.detail);
    }
    const inner = normalizeType(match[1]);
    if (member.name === "clone") {
      return receiverType;
    }
    if (member.name === "recv") {
      return `Option[${inner}]`;
    }
    if (member.name === "send") {
      return `Result[None, SendError[${inner}]]`;
    }
    return "None";
  }

  if (base === "Task") {
    const match = receiverType.match(/^Task\[(.+)\]$/);
    if (!match) {
      return parseBuiltinDetailReturnType(member.detail);
    }
    if (member.name === "clone") {
      return receiverType;
    }
    if (member.name === "join") {
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

function inferCaseBindingType(trimmed, moduleInfo) {
  const match = trimmed.match(/^case\s+([A-Z][A-Za-z0-9_]*)\.([A-Z][A-Za-z0-9_]*)\([a-zA-Z_][A-Za-z0-9_]*\)\s*:/);
  if (!match) {
    return null;
  }
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

function inferForBindingType(iterableExpression, moduleInfo, functionInfo) {
  const iterableType = inferExpressionType(iterableExpression, moduleInfo, functionInfo);
  if (iterableType === "Range") {
    return "int32";
  }
  return null;
}

function stripOuterParens(expression) {
  if (expression.startsWith("(") && expression.endsWith(")")) {
    return expression.slice(1, -1).trim();
  }
  return expression;
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
  analyzeDocument,
  completionsForDocument,
  definitionForPosition,
  diagnosticsForDocument,
  documentSymbols,
  hoverForPosition
};
