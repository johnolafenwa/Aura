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
  "public",
  "mut",
  "borrow",
  "indirect",
  "copy",
  "break",
  "continue"
];

const BUILTIN_MEMBERS = {
  float64: [{ name: "sqrt", kind: "method", detail: "sqrt() -> float64" }],
  String: [
    { name: "clone", kind: "method", detail: "clone() -> String" },
    { name: "as_str", kind: "method", detail: "as_str() -> borrow str" }
  ],
  Vec: [
    { name: "len", kind: "method", detail: "len() -> uintsize" },
    { name: "clone", kind: "method", detail: "clone() -> Vec[T]" }
  ]
};

const BUILTIN_FUNCTIONS = [
  { name: "print", kind: "function", detail: "print(value) -> None" }
];

function analyzeDocument(text) {
  const lines = text.replace(/\r\n/g, "\n").split("\n");
  const moduleInfo = {
    classes: new Map(),
    functions: new Map(),
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

    const classMatch = trimmed.match(/^class\s+([A-Z][A-Za-z0-9_]*)\s*:/);
    if (classMatch) {
      const parsed = parseClass(lines, i, indent);
      moduleInfo.classes.set(parsed.classInfo.name, parsed.classInfo);
      i = parsed.endLine;
      continue;
    }

    const functionMatch = trimmed.match(/^def\s+([a-zA-Z_][A-Za-z0-9_]*)\s*\(/);
    if (functionMatch) {
      const parsed = parseFunctionSignature(lines, i, indent);
      moduleInfo.functions.set(parsed.functionInfo.name, parsed.functionInfo);
      i = parsed.endLine;
    }
  }

  for (const functionInfo of moduleInfo.functions.values()) {
    populateFunctionLocals(functionInfo, lines, moduleInfo);
  }

  return moduleInfo;
}

function parseClass(lines, startLine, indent) {
  const line = lines[startLine].trim();
  const headerMatch = line.match(/^class\s+([A-Z][A-Za-z0-9_]*)\s*:/);
  const classInfo = {
    kind: "class",
    name: headerMatch[1],
    line: startLine,
    fields: [],
    methods: [],
    members: new Map()
  };

  let i = startLine + 1;
  for (; i < lines.length; i += 1) {
    const rawLine = lines[i];
    const trimmed = rawLine.trim();
    if (!trimmed || trimmed.startsWith("#")) {
      continue;
    }

    const currentIndent = countIndent(rawLine);
    if (currentIndent <= indent) {
      break;
    }

    if (currentIndent !== indent + 4) {
      continue;
    }

    const methodMatch = trimmed.match(
      /^def\s+([a-zA-Z_][A-Za-z0-9_]*)\s*\((.*)\)(?:\s*->\s*([^:]+))?\s*:/
    );
    if (methodMatch) {
      const method = {
        kind: "method",
        name: methodMatch[1],
        returnType: normalizeType(methodMatch[3] || "None"),
        line: i
      };
      classInfo.methods.push(method);
      classInfo.members.set(method.name, method);
      continue;
    }

    const fieldMatch = trimmed.match(
      /^(?:public\s+)?([a-zA-Z_][A-Za-z0-9_]*)\s*:\s*([^=]+?)(?:\s*=\s*.+)?$/
    );
    if (fieldMatch) {
      const field = {
        kind: "field",
        name: fieldMatch[1],
        type: normalizeType(fieldMatch[2]),
        line: i
      };
      classInfo.fields.push(field);
      classInfo.members.set(field.name, field);
    }
  }

  return { classInfo, endLine: i - 1 };
}

function parseFunctionSignature(lines, startLine, indent) {
  const line = lines[startLine].trim();
  const headerMatch = line.match(
    /^def\s+([a-zA-Z_][A-Za-z0-9_]*)\s*\((.*)\)(?:\s*->\s*([^:]+))?\s*:/
  );
  const functionInfo = {
    kind: "function",
    name: headerMatch[1],
    params: parseParams(headerMatch[2]),
    returnType: normalizeType(headerMatch[3] || "None"),
    locals: new Map(),
    line: startLine,
    endLine: startLine
  };

  for (const param of functionInfo.params) {
    functionInfo.locals.set(param.name, param.type);
  }

  let i = startLine + 1;
  for (; i < lines.length; i += 1) {
    const rawLine = lines[i];
    const trimmed = rawLine.trim();
    if (!trimmed || trimmed.startsWith("#")) {
      continue;
    }

    const currentIndent = countIndent(rawLine);
    if (currentIndent <= indent) {
      break;
    }
  }

  functionInfo.endLine = i - 1;
  return { functionInfo, endLine: i - 1 };
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

    const assignMatch = trimmed.match(
      /^(?:mut\s+)?([a-zA-Z_][A-Za-z0-9_]*)(?:\s*:\s*([^=]+))?\s*=\s*(.+)$/
    );
    if (!assignMatch) {
      continue;
    }

    const name = assignMatch[1];
    const annotation = assignMatch[2] ? normalizeType(assignMatch[2]) : null;
    const expression = assignMatch[3].trim();
    const inferredType = annotation || inferExpressionType(expression, moduleInfo, functionInfo);
    if (inferredType) {
      functionInfo.locals.set(name, inferredType);
    }
  }
}

function parseParams(rawParams) {
  if (!rawParams.trim()) {
    return [];
  }

  return rawParams
    .split(",")
    .map((part) => part.trim())
    .filter(Boolean)
    .map((part) => {
      const match = part.match(/^([a-zA-Z_][A-Za-z0-9_]*)\s*:\s*(.+)$/);
      if (!match) {
        return null;
      }
      return {
        name: match[1],
        type: normalizeType(match[2])
      };
    })
    .filter(Boolean);
}

function inferExpressionType(expression, moduleInfo, functionInfo) {
  const expr = stripOuterParens(expression.trim());

  if (/^\d+\.\d+$/.test(expr)) {
    return "float64";
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

  const functionMatch = expr.match(/^([a-zA-Z_][A-Za-z0-9_]*)\s*\(/);
  if (functionMatch && moduleInfo.functions.has(functionMatch[1])) {
    return moduleInfo.functions.get(functionMatch[1]).returnType;
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
  const match = expression.match(/(.+)\s*([+\-*/])\s*(.+)/);
  if (!match) {
    return null;
  }

  const leftType = inferExpressionType(match[1], moduleInfo, functionInfo);
  const rightType = inferExpressionType(match[3], moduleInfo, functionInfo);
  if (!leftType || !rightType) {
    return null;
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
  const chain = expression.match(/^[A-Za-z_][A-Za-z0-9_]*(?:\.[A-Za-z_][A-Za-z0-9_]*)*$/);
  if (!chain) {
    return null;
  }

  const parts = expression.split(".");
  let currentType = functionInfo.locals.get(parts[0]) || null;

  if (!currentType && moduleInfo.classes.has(parts[0])) {
    currentType = parts[0];
  }

  if (!currentType) {
    return null;
  }

  for (let i = 1; i < parts.length; i += 1) {
    const memberName = parts[i];
    const classInfo = moduleInfo.classes.get(baseTypeName(currentType));
    if (classInfo && classInfo.members.has(memberName)) {
      const member = classInfo.members.get(memberName);
      currentType = member.kind === "field" ? member.type : member.returnType;
      continue;
    }

    const builtinMember = (BUILTIN_MEMBERS[baseTypeName(currentType)] || []).find(
      (item) => item.name === memberName
    );
    if (builtinMember) {
      currentType = parseBuiltinDetailReturnType(builtinMember.detail);
      continue;
    }

    return null;
  }

  return currentType;
}

function completionsForDocument(text, line, character, triggerCharacter) {
  const moduleInfo = analyzeDocument(text);
  const lineText = moduleInfo.lines[line] || "";
  const functionInfo = findEnclosingFunction(moduleInfo, line);

  if (triggerCharacter === ".") {
    const receiver = extractReceiverBeforeDot(lineText, character);
    if (!receiver || !functionInfo) {
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
  for (const functionInfoItem of moduleInfo.functions.values()) {
    completions.push({
      name: functionInfoItem.name,
      kind: "function",
      detail: `${functionInfoItem.name}() -> ${functionInfoItem.returnType}`
    });
  }
  for (const builtin of BUILTIN_FUNCTIONS) {
    completions.push(builtin);
  }
  return completions;
}

function memberCompletions(receiver, moduleInfo, functionInfo) {
  const typeName = inferChainType(receiver, moduleInfo, functionInfo);
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
        detail: `${method.name}() -> ${method.returnType}`
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
      children: [
        ...classInfo.fields.map((field) => ({
          name: field.name,
          kind: "field",
          line: field.line
        })),
        ...classInfo.methods.map((method) => ({
          name: method.name,
          kind: "method",
          line: method.line
        }))
      ]
    });
  }

  for (const functionInfo of moduleInfo.functions.values()) {
    symbols.push({
      name: functionInfo.name,
      kind: "function",
      line: functionInfo.line,
      children: []
    });
  }

  return symbols;
}

function findEnclosingFunction(moduleInfo, line) {
  let current = null;
  for (const functionInfo of moduleInfo.functions.values()) {
    if (functionInfo.line <= line && line <= functionInfo.endLine) {
      current = functionInfo;
    }
  }
  return current;
}

function extractReceiverBeforeDot(lineText, character) {
  const prefix = lineText.slice(0, Math.max(0, character - 1));
  const match = prefix.match(/([A-Za-z_][A-Za-z0-9_]*(?:\.[A-Za-z_][A-Za-z0-9_]*)*)\s*$/);
  return match ? match[1] : null;
}

function parseBuiltinDetailReturnType(detail) {
  const match = detail.match(/->\s*([A-Za-z_][A-Za-z0-9_\[\] ]*)/);
  return match ? normalizeType(match[1]) : null;
}

function normalizeType(rawType) {
  return rawType.trim().replace(/\s+/g, " ");
}

function baseTypeName(typeName) {
  return typeName.replace(/\[.*\]$/, "").trim();
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

module.exports = {
  KEYWORDS,
  analyzeDocument,
  completionsForDocument,
  documentSymbols
};
