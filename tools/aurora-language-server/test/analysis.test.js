"use strict";

const test = require("node:test");
const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");

const {
  analyzeDocument,
  completionsForDocument,
  definitionForPosition,
  diagnosticsForDocument,
  documentSymbols,
  hoverForPosition
} = require("../src/analysis");

const pointSource = fs.readFileSync(
  path.join(__dirname, "../../../examples/point.au"),
  "utf8"
);
const basicAdditionSource = fs.readFileSync(
  path.join(__dirname, "../../../examples/basic_addition.au"),
  "utf8"
);
const controlFlowSource = fs.readFileSync(
  path.join(__dirname, "../../../examples/control_flow.au"),
  "utf8"
);
const methodSource = fs.readFileSync(
  path.join(__dirname, "../../../examples/classes/methods.au"),
  "utf8"
);
const enumMatchSource = fs.readFileSync(
  path.join(__dirname, "../../../examples/enums/result_match.au"),
  "utf8"
);
const forRangeSource = fs.readFileSync(
  path.join(__dirname, "../../../examples/control_flow/for_range.au"),
  "utf8"
);
const resultOptionSource = fs.readFileSync(
  path.join(__dirname, "../../../examples/enums/result_option.au"),
  "utf8"
);
const mutatingMethodSource = fs.readFileSync(
  path.join(__dirname, "../../../examples/classes/mutating_methods.au"),
  "utf8"
);
const trySource = fs.readFileSync(
  path.join(__dirname, "../../../examples/error_handling/try_result.au"),
  "utf8"
);
const withSource = fs.readFileSync(
  path.join(__dirname, "../../../examples/resources/with_resource.au"),
  "utf8"
);
const concurrencySource = fs.readFileSync(
  path.join(__dirname, "../../../examples/concurrency/channels_spawn.au"),
  "utf8"
);
const structuredConcurrencySource = fs.readFileSync(
  path.join(__dirname, "../../../examples/concurrency/task_group_select.au"),
  "utf8"
);
const cancellationSource = fs.readFileSync(
  path.join(__dirname, "../../../examples/concurrency/task_group_cancel.au"),
  "utf8"
);
const sendResultSource = fs.readFileSync(
  path.join(__dirname, "../../../examples/concurrency/send_result.au"),
  "utf8"
);
const detachedSource = fs.readFileSync(
  path.join(__dirname, "../../../examples/concurrency/spawn_detached.au"),
  "utf8"
);
const selectSendSource = fs.readFileSync(
  path.join(__dirname, "../../../examples/concurrency/select_send.au"),
  "utf8"
);
const namedBuiltinSource = fs.readFileSync(
  path.join(__dirname, "../../../examples/basics/named_builtin_arguments.au"),
  "utf8"
);
const defaultArgumentsSource = fs.readFileSync(
  path.join(__dirname, "../../../examples/basics/default_arguments.au"),
  "utf8"
);
const passKeywordSource = fs.readFileSync(
  path.join(__dirname, "../../../examples/basics/pass_keyword.au"),
  "utf8"
);
const sleepBuiltinSource = fs.readFileSync(
  path.join(__dirname, "../../../examples/concurrency/sleep_builtin.au"),
  "utf8"
);
const selectTimeoutNamedSource = fs.readFileSync(
  path.join(__dirname, "../../../examples/concurrency/select_timeout_named.au"),
  "utf8"
);
const stringCloneSource = fs.readFileSync(
  path.join(__dirname, "../../../examples/strings/string_clone.au"),
  "utf8"
);
const borrowParametersSource = fs.readFileSync(
  path.join(__dirname, "../../../examples/basics/borrow_parameters.au"),
  "utf8"
);
const fStringsSource = fs.readFileSync(
  path.join(__dirname, "../../../examples/strings/f_strings.au"),
  "utf8"
);
const copyClassSource = fs.readFileSync(
  path.join(__dirname, "../../../examples/classes/copy_class.au"),
  "utf8"
);
const matchBorrowSource = fs.readFileSync(
  path.join(__dirname, "../../../examples/enums/match_borrow.au"),
  "utf8"
);
const channelIterationSource = fs.readFileSync(
  path.join(__dirname, "../../../examples/concurrency/channel_iteration.au"),
  "utf8"
);

test("analyzeDocument finds classes and functions", () => {
  const moduleInfo = analyzeDocument(pointSource);
  assert.ok(moduleInfo.classes.has("Point"));
  assert.ok(moduleInfo.functions.has("distance"));
  assert.ok(moduleInfo.functions.has("main"));
});

test("member completion suggests class fields after dot", () => {
  const lineIndex = pointSource.split("\n").findIndex((line) => line.includes("a.x"));
  const lineText = pointSource.split("\n")[lineIndex];
  const character = lineText.indexOf(".") + 1;
  const items = completionsForDocument(pointSource, lineIndex, character, ".");
  const names = items.map((item) => item.name).sort();
  assert.deepEqual(names, ["x", "y"]);
});

test("member completion suggests user-defined methods", () => {
  const lineIndex = methodSource.split("\n").findIndex((line) => line.includes("counter.read()"));
  const lineText = methodSource.split("\n")[lineIndex];
  const character = lineText.indexOf(".") + 1;
  const items = completionsForDocument(methodSource, lineIndex, character, ".");
  const names = new Set(items.map((item) => item.name));

  assert.ok(names.has("read"));
  assert.ok(names.has("doubled"));
});

test("member completion suggests mutating methods", () => {
  const lineIndex = mutatingMethodSource.split("\n").findIndex((line) => line.includes("counter.bump()"));
  const lineText = mutatingMethodSource.split("\n")[lineIndex];
  const character = lineText.indexOf(".") + 1;
  const items = completionsForDocument(mutatingMethodSource, lineIndex, character, ".");
  const names = new Set(items.map((item) => item.name));

  assert.ok(names.has("bump"));
  assert.ok(names.has("reset"));
});

test("member completion works for parenthesized receiver expressions", () => {
  const lineIndex = pointSource.split("\n").findIndex((line) => line.includes(".sqrt()"));
  const lineText = pointSource.split("\n")[lineIndex];
  const character = lineText.indexOf(".") + 1;
  const items = completionsForDocument(pointSource, lineIndex, character, ".");
  const names = new Set(items.map((item) => item.name));

  assert.ok(names.has("sqrt"));
});

test("string member completion exposes clone but not as_str", () => {
  const lineIndex = stringCloneSource.split("\n").findIndex((line) => line.includes("text.clone()"));
  const lineText = stringCloneSource.split("\n")[lineIndex];
  const character = lineText.indexOf(".") + 1;
  const items = completionsForDocument(stringCloneSource, lineIndex, character, ".");
  const names = new Set(items.map((item) => item.name));

  assert.ok(names.has("clone"));
  assert.ok(!names.has("as_str"));
});

test("top-level completion includes keywords and declarations", () => {
  const items = completionsForDocument(pointSource, 0, 0, null);
  const names = new Set(items.map((item) => item.name));
  assert.ok(names.has("class"));
  assert.ok(names.has("as"));
  assert.ok(names.has("and"));
  assert.ok(names.has("or"));
  assert.ok(names.has("not"));
  assert.ok(names.has("Point"));
  assert.ok(names.has("distance"));
  assert.ok(names.has("print"));
  const range = items.find((item) => item.name === "range");
  assert.ok(range);
  assert.match(range.detail, /range\(start: int32, stop: int32\)/);
});

test("document symbols include class and function entries", () => {
  const symbols = documentSymbols(pointSource);
  assert.ok(symbols.some((symbol) => symbol.kind === "class" && symbol.name === "Point"));
  assert.ok(symbols.some((symbol) => symbol.kind === "function" && symbol.name === "main"));
});

test("method example does not report false diagnostics for self inside method bodies", () => {
  const diagnostics = diagnosticsForDocument(methodSource);
  assert.ok(!diagnostics.some((diagnostic) => /unknown name `self`/.test(diagnostic.message)));
});

test("hover resolves self inside method bodies", () => {
  const lineIndex = methodSource.split("\n").findIndex((line) => line.includes("return self.value"));
  const lineText = methodSource.split("\n")[lineIndex];
  const character = lineText.indexOf("self");
  const hover = hoverForPosition(methodSource, lineIndex, character);

  assert.ok(hover);
  assert.match(hover.value, /param self: Counter/);
});

test("analysis tracks canonical free-function borrowed parameters", () => {
  const moduleInfo = analyzeDocument(borrowParametersSource);
  const readFunction = moduleInfo.functions.get("read");
  const showFunction = moduleInfo.functions.get("show");

  assert.ok(readFunction);
  assert.ok(showFunction);
  assert.equal(readFunction.params[0].type, "Counter");
  assert.equal(showFunction.params[0].type, "Counter");

  const lineIndex = borrowParametersSource
    .split("\n")
    .findIndex((line) => line.includes("return counter.value"));
  const lineText = borrowParametersSource.split("\n")[lineIndex];
  const character = lineText.indexOf("counter");
  const hover = hoverForPosition(borrowParametersSource, lineIndex, character);

  assert.ok(hover);
  assert.match(hover.value, /param counter: Counter/);
});

test("document symbols include enums and variants", () => {
  const symbols = documentSymbols(enumMatchSource);
  const parseResult = symbols.find((symbol) => symbol.kind === "enum" && symbol.name === "ParseResult");
  assert.ok(parseResult);
  assert.ok(parseResult.children.some((child) => child.kind === "variant" && child.name === "Success"));
  assert.ok(parseResult.children.some((child) => child.kind === "variant" && child.name === "Failure"));
});

test("functions without explicit return types still appear in analysis", () => {
  const moduleInfo = analyzeDocument(basicAdditionSource);
  assert.ok(moduleInfo.functions.has("main"));
  assert.equal(moduleInfo.functions.get("main").returnType, "None");
});

test("hover describes local symbols", () => {
  const lineIndex = basicAdditionSource.split("\n").findIndex((line) => line.includes("print(c)"));
  const lineText = basicAdditionSource.split("\n")[lineIndex];
  const character = lineText.indexOf("c");
  const hover = hoverForPosition(basicAdditionSource, lineIndex, character);

  assert.ok(hover);
  assert.match(hover.value, /local c: int32/);
});

test("definition jumps to local bindings", () => {
  const lineIndex = basicAdditionSource.split("\n").findIndex((line) => line.includes("print(c)"));
  const lineText = basicAdditionSource.split("\n")[lineIndex];
  const character = lineText.indexOf("c");
  const definition = definitionForPosition(basicAdditionSource, lineIndex, character);

  assert.deepEqual(definition, {
    line: 4,
    startCharacter: 4,
    endCharacter: 5
  });
});

test("diagnostics report unknown names and duplicate declarations", () => {
  const source = [
    "def main():",
    "    print(total)",
    "",
    "def main():",
    "    print(1)"
  ].join("\n");

  const diagnostics = diagnosticsForDocument(source);
  assert.ok(diagnostics.some((diagnostic) => /unknown name `total`/.test(diagnostic.message)));
  assert.ok(diagnostics.some((diagnostic) => /duplicate function `main`/.test(diagnostic.message)));
});

test("top-level bindings are tracked for diagnostics, hover, and definition", () => {
  const diagnostics = diagnosticsForDocument(controlFlowSource);
  assert.ok(!diagnostics.some((diagnostic) => /unknown name `total`/.test(diagnostic.message)));

  const lineIndex = controlFlowSource.split("\n").findIndex((line) => line.includes("if total == 16:"));
  const lineText = controlFlowSource.split("\n")[lineIndex];
  const character = lineText.indexOf("total");

  const hover = hoverForPosition(controlFlowSource, lineIndex, character);
  assert.ok(hover);
  assert.match(hover.value, /binding total: int32/);

  const definition = definitionForPosition(controlFlowSource, lineIndex, character);
  assert.deepEqual(definition, {
    line: 1,
    startCharacter: 4,
    endCharacter: 9
  });
});

test("hover describes builtin members", () => {
  const source = [
    "def main() -> float64:",
    "    value: float64 = 9.0",
    "    return value.sqrt()"
  ].join("\n");
  const lineIndex = 2;
  const character = source.split("\n")[lineIndex].indexOf("sqrt");
  const hover = hoverForPosition(source, lineIndex, character);

  assert.ok(hover);
  assert.match(hover.value, /method sqrt\(\) -> float64/);
});

test("point example does not report false diagnostics for parenthesized sqrt access", () => {
  const diagnostics = diagnosticsForDocument(pointSource);
  assert.ok(!diagnostics.some((diagnostic) => /unknown name `sqrt`/.test(diagnostic.message)));
});

test("enum example does not report false diagnostics for variants or match payload bindings", () => {
  const diagnostics = diagnosticsForDocument(enumMatchSource);
  assert.ok(!diagnostics.some((diagnostic) => /unknown name `ParseResult`/.test(diagnostic.message)));
  assert.ok(!diagnostics.some((diagnostic) => /unknown name `value`/.test(diagnostic.message)));
  assert.ok(!diagnostics.some((diagnostic) => /unknown name `message`/.test(diagnostic.message)));
});

test("enum names and variants appear in completions and hover", () => {
  const topLevelItems = completionsForDocument(enumMatchSource, 0, 0, null);
  const topLevelNames = new Set(topLevelItems.map((item) => item.name));
  assert.ok(topLevelNames.has("ParseResult"));

  const lineIndex = enumMatchSource.split("\n").findIndex((line) => line.includes("ParseResult.Success(42)"));
  const lineText = enumMatchSource.split("\n")[lineIndex];
  const character = lineText.indexOf(".") + 1;
  const memberItems = completionsForDocument(enumMatchSource, lineIndex, character, ".");
  const memberNames = new Set(memberItems.map((item) => item.name));
  assert.ok(memberNames.has("Success"));
  assert.ok(memberNames.has("Failure"));

  const hoverCharacter = lineText.indexOf("Success");
  const hover = hoverForPosition(enumMatchSource, lineIndex, hoverCharacter);
  assert.ok(hover);
  assert.match(hover.value, /variant Success\(int32\) -> ParseResult/);
});

test("built-in Result and Option do not produce false diagnostics", () => {
  const diagnostics = diagnosticsForDocument(resultOptionSource);
  assert.ok(!diagnostics.some((diagnostic) => /unknown name `Result`/.test(diagnostic.message)));
  assert.ok(!diagnostics.some((diagnostic) => /unknown name `Option`/.test(diagnostic.message)));
  assert.ok(!diagnostics.some((diagnostic) => /unknown name `value`/.test(diagnostic.message)));
  assert.ok(!diagnostics.some((diagnostic) => /unknown name `message`/.test(diagnostic.message)));
});

test("built-in Result and Option expose variant completions and hover", () => {
  const lineIndex = resultOptionSource.split("\n").findIndex((line) => line.includes("Result.Ok(a / b)"));
  const lineText = resultOptionSource.split("\n")[lineIndex];
  const character = lineText.indexOf(".") + 1;
  const items = completionsForDocument(resultOptionSource, lineIndex, character, ".");
  const names = new Set(items.map((item) => item.name));
  assert.ok(names.has("Ok"));
  assert.ok(names.has("Err"));

  const hoverCharacter = lineText.indexOf("Ok");
  const hover = hoverForPosition(resultOptionSource, lineIndex, hoverCharacter);
  assert.ok(hover);
  assert.match(hover.value, /variant Ok\(T\) -> Result/);
});

test("try example does not report false diagnostics for try-bound values", () => {
  const diagnostics = diagnosticsForDocument(trySource);
  assert.ok(!diagnostics.some((diagnostic) => /unknown name `value`/.test(diagnostic.message)));
  assert.ok(!diagnostics.some((diagnostic) => /unknown name `divide`/.test(diagnostic.message)));
});

test("with example tracks scoped bindings for diagnostics and hover", () => {
  const diagnostics = diagnosticsForDocument(withSource);
  assert.ok(!diagnostics.some((diagnostic) => /unknown name `file`/.test(diagnostic.message)));

  const lineIndex = withSource.split("\n").findIndex((line) => line.includes("print(file.read())"));
  const lineText = withSource.split("\n")[lineIndex];
  const character = lineText.indexOf("file");
  const hover = hoverForPosition(withSource, lineIndex, character);
  assert.ok(hover);
  assert.match(hover.value, /local file: FileHandle/);
});

test("channel and task builtins appear in completions and diagnostics", () => {
  const diagnostics = diagnosticsForDocument(concurrencySource);
  assert.ok(!diagnostics.some((diagnostic) => /unknown name `channel`/.test(diagnostic.message)));
  assert.ok(!diagnostics.some((diagnostic) => /unknown name `spawn`/.test(diagnostic.message)));
  assert.ok(!diagnostics.some((diagnostic) => /unknown name `task`/.test(diagnostic.message)));
  assert.ok(!diagnostics.some((diagnostic) => /unknown name `ch`/.test(diagnostic.message)));

  const channelLine = concurrencySource.split("\n").findIndex((line) => line.includes("match ch.recv():"));
  const channelText = concurrencySource.split("\n")[channelLine];
  const channelCharacter = channelText.indexOf(".") + 1;
  const channelItems = completionsForDocument(concurrencySource, channelLine, channelCharacter, ".");
  const channelNames = new Set(channelItems.map((item) => item.name));
  assert.ok(channelNames.has("send"));
  assert.ok(channelNames.has("recv"));
  assert.ok(channelNames.has("close"));

  const taskLine = concurrencySource.split("\n").findIndex((line) => line.includes("task.join()"));
  const taskText = concurrencySource.split("\n")[taskLine];
  const taskCharacter = taskText.indexOf(".") + 1;
  const taskItems = completionsForDocument(concurrencySource, taskLine, taskCharacter, ".");
  const taskNames = new Set(taskItems.map((item) => item.name));
  assert.ok(taskNames.has("join"));
});

test("structured concurrency bindings and builtins do not report false diagnostics", () => {
  const diagnostics = diagnosticsForDocument(structuredConcurrencySource);
  assert.ok(!diagnostics.some((diagnostic) => /unknown name `task_group`/.test(diagnostic.message)));
  assert.ok(!diagnostics.some((diagnostic) => /unknown name `group`/.test(diagnostic.message)));
  assert.ok(!diagnostics.some((diagnostic) => /unknown name `value`/.test(diagnostic.message)));
  assert.ok(!diagnostics.some((diagnostic) => /unknown name `cancelled`/.test(diagnostic.message)));
});

test("task-group member completion suggests structured concurrency methods", () => {
  const lineIndex = structuredConcurrencySource
    .split("\n")
    .findIndex((line) => line.includes("group.spawn(worker, out.clone())"));
  const lineText = structuredConcurrencySource.split("\n")[lineIndex];
  const character = lineText.indexOf(".") + 1;
  const items = completionsForDocument(structuredConcurrencySource, lineIndex, character, ".");
  const names = new Set(items.map((item) => item.name));

  assert.ok(names.has("spawn"));
  assert.ok(names.has("cancel"));
});

test("select arm bindings resolve in hover", () => {
  const lineIndex = structuredConcurrencySource
    .split("\n")
    .findIndex((line) => line.includes("match value:"));
  const lineText = structuredConcurrencySource.split("\n")[lineIndex];
  const character = lineText.indexOf("value");
  const hover = hoverForPosition(structuredConcurrencySource, lineIndex, character);

  assert.ok(hover);
  assert.match(hover.value, /local value: Option\[int32\]/);
});

test("structured concurrency helpers appear in top-level completions", () => {
  const items = completionsForDocument(cancellationSource, 0, 0, null);
  const names = new Set(items.map((item) => item.name));

  assert.ok(names.has("task_group"));
  assert.ok(names.has("cancelled"));
});

test("send-result example infers Result and SendError types without false diagnostics", () => {
  const diagnostics = diagnosticsForDocument(sendResultSource);
  assert.ok(!diagnostics.some((diagnostic) => /unknown name `send_result`/.test(diagnostic.message)));
  assert.ok(!diagnostics.some((diagnostic) => /unknown name `SendError`/.test(diagnostic.message)));
  assert.ok(!diagnostics.some((diagnostic) => /unknown name `err`/.test(diagnostic.message)));

  const lineIndex = sendResultSource.split("\n").findIndex((line) => line.includes("match send_result:"));
  const lineText = sendResultSource.split("\n")[lineIndex];
  const character = lineText.indexOf("send_result");
  const hover = hoverForPosition(sendResultSource, lineIndex, character);
  assert.ok(hover);
  assert.match(hover.value, /local send_result: Result\[None, SendError\[int32\]\]/);
});

test("detached spawn example does not report false diagnostics and exposes the keyword", () => {
  const diagnostics = diagnosticsForDocument(detachedSource);
  assert.ok(!diagnostics.some((diagnostic) => /unknown name `producer`/.test(diagnostic.message)));
  assert.ok(!diagnostics.some((diagnostic) => /unknown name `spawn`/.test(diagnostic.message)));

  const items = completionsForDocument(detachedSource, 0, 0, null);
  const names = new Set(items.map((item) => item.name));
  assert.ok(names.has("detached"));
});

test("select send bindings resolve with result types", () => {
  const diagnostics = diagnosticsForDocument(selectSendSource);
  assert.ok(!diagnostics.some((diagnostic) => /unknown name `send_result`/.test(diagnostic.message)));
  assert.ok(!diagnostics.some((diagnostic) => /unknown name `after`/.test(diagnostic.message)));
  assert.ok(!diagnostics.some((diagnostic) => /unknown name `ms`/.test(diagnostic.message)));

  const lineIndex = selectSendSource.split("\n").findIndex((line) => line.includes("match send_result:"));
  const lineText = selectSendSource.split("\n")[lineIndex];
  const character = lineText.indexOf("send_result");
  const hover = hoverForPosition(selectSendSource, lineIndex, character);
  assert.ok(hover);
  assert.match(hover.value, /local send_result: Result\[None, SendError\[int32\]\]/);
});

test("after duration expressions resolve as builtins without false diagnostics", () => {
  const lineIndex = selectSendSource.split("\n").findIndex((line) => line.includes("case after(5ms):"));
  const lineText = selectSendSource.split("\n")[lineIndex];
  const character = lineText.indexOf("after");

  const hover = hoverForPosition(selectSendSource, lineIndex, character);
  assert.ok(hover);
  assert.match(hover.value, /after\(duration: Duration\) -> Duration/);
});

test("builtin named arguments do not report false diagnostics", () => {
  const diagnostics = diagnosticsForDocument(namedBuiltinSource);
  assert.ok(!diagnostics.some((diagnostic) => /unknown name `range`/.test(diagnostic.message)));
  assert.ok(!diagnostics.some((diagnostic) => /unknown name `print`/.test(diagnostic.message)));
});

test("named after duration expressions resolve as builtins without false diagnostics", () => {
  const diagnostics = diagnosticsForDocument(selectTimeoutNamedSource);
  assert.ok(!diagnostics.some((diagnostic) => /unknown name `after`/.test(diagnostic.message)));

  const lineIndex = selectTimeoutNamedSource
    .split("\n")
    .findIndex((line) => line.includes("case after(duration=5ms):"));
  const lineText = selectTimeoutNamedSource.split("\n")[lineIndex];
  const character = lineText.indexOf("after");

  const hover = hoverForPosition(selectTimeoutNamedSource, lineIndex, character);
  assert.ok(hover);
  assert.match(hover.value, /after\(duration: Duration\) -> Duration/);
});

test("for-range example does not report false diagnostics for loop bindings", () => {
  const diagnostics = diagnosticsForDocument(forRangeSource);
  assert.ok(!diagnostics.some((diagnostic) => /unknown name `value`/.test(diagnostic.message)));
  assert.ok(!diagnostics.some((diagnostic) => /unknown name `range`/.test(diagnostic.message)));
});

test("for-range loop bindings resolve in hover and definition", () => {
  const lineIndex = forRangeSource.split("\n").findIndex((line) => line.includes("if value == 3:"));
  const lineText = forRangeSource.split("\n")[lineIndex];
  const character = lineText.indexOf("value");

  const hover = hoverForPosition(forRangeSource, lineIndex, character);
  assert.ok(hover);
  assert.match(hover.value, /local value: int32/);

  const definition = definitionForPosition(forRangeSource, lineIndex, character);
  assert.deepEqual(definition, {
    line: 3,
    startCharacter: 8,
    endCharacter: 13
  });
});

test("point example hover and definition behavior for builtin sqrt are stable", () => {
  const lineIndex = pointSource.split("\n").findIndex((line) => line.includes(".sqrt()"));
  const lineText = pointSource.split("\n")[lineIndex];
  const character = lineText.indexOf("sqrt");

  const hover = hoverForPosition(pointSource, lineIndex, character);
  assert.ok(hover);
  assert.match(hover.value, /method sqrt\(\) -> float64/);
  assert.deepEqual(hover.range, {
    start: { line: lineIndex, character },
    end: { line: lineIndex, character: character + 4 }
  });

  const definition = definitionForPosition(pointSource, lineIndex, character);
  assert.equal(definition, null);
});

test("fallback analysis infers cast expression result types", () => {
  const source = [
    "def main():",
    "    value = 7.9 as int32",
    "    print(value)"
  ].join("\n");
  const diagnostics = diagnosticsForDocument(source);
  assert.equal(diagnostics.length, 0);

  const lineIndex = 2;
  const character = source.split("\n")[lineIndex].indexOf("value");
  const hover = hoverForPosition(source, lineIndex, character);

  assert.ok(hover);
  assert.match(hover.value, /local value: int32/);
});

test("fallback analysis recognizes user-defined generic declarations", () => {
  const source = [
    "class Box[T]:",
    "    value: T",
    "",
    "    def get(borrow self) -> T:",
    "        return self.value",
    "",
    "enum Wrapper[T]:",
    "    Item(T)",
    "",
    "def identity[T](value: T) -> T:",
    "    return value",
    "",
    "def main() -> int32:",
    "    boxed: Box[int32] = Box(value=identity(7))",
    "    print(boxed.get())",
    "    return 0"
  ].join("\n");

  const analysis = analyzeDocument(source);
  assert.equal(analysis.diagnostics.length, 0);
  assert.ok(analysis.classes.has("Box"));
  assert.ok(analysis.enums.has("Wrapper"));
  assert.ok(analysis.functions.has("identity"));
});

test("fallback analysis accepts default parameter values", () => {
  const analysis = analyzeDocument(defaultArgumentsSource);
  assert.equal(analysis.diagnostics.length, 0);
  assert.ok(analysis.functions.has("greet"));
  assert.ok(analysis.functions.has("scale"));
});

test("fallback analysis accepts pass and sleep builtins", () => {
  assert.equal(analyzeDocument(passKeywordSource).diagnostics.length, 0);
  assert.equal(diagnosticsForDocument(sleepBuiltinSource).length, 0);
  const completions = completionsForDocument("", 0, 0, null);
  assert.ok(completions.some((item) => item.name === "pass"));
  assert.ok(completions.some((item) => item.name === "sleep"));
});

test("fallback analysis accepts f-strings, copy classes, borrowed match, and channel iteration", () => {
  assert.equal(diagnosticsForDocument(fStringsSource).length, 0);
  assert.equal(diagnosticsForDocument(copyClassSource).length, 0);
  assert.equal(diagnosticsForDocument(matchBorrowSource).length, 0);
  assert.equal(diagnosticsForDocument(channelIterationSource).length, 0);
});
