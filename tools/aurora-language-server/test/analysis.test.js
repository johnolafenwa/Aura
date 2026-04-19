"use strict";

const test = require("node:test");
const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");

const {
  _testing,
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
  path.join(__dirname, "../../../examples/concurrency/queues_spawn.au"),
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
const queueIterationSource = fs.readFileSync(
  path.join(__dirname, "../../../examples/concurrency/queue_iteration.au"),
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

test("string member completion exposes the maintained String method surface", () => {
  const source =
    "def main() -> int32:\n    text = \"  aurora repo  \"\n    text.\n    return 0\n";
  const lineIndex = source.split("\n").findIndex((line) => line.includes("text."));
  const lineText = source.split("\n")[lineIndex];
  const character = lineText.indexOf(".") + 1;
  const items = completionsForDocument(source, lineIndex, character, ".");
  const names = new Set(items.map((item) => item.name));

  assert.ok(names.has("len"));
  assert.ok(names.has("contains"));
  assert.ok(names.has("starts_with"));
  assert.ok(names.has("ends_with"));
  assert.ok(names.has("trim"));
  assert.ok(names.has("split"));
  assert.ok(names.has("replace"));
  assert.ok(names.has("to_lower"));
  assert.ok(names.has("to_upper"));
  assert.ok(names.has("strip_prefix"));
  assert.ok(names.has("strip_suffix"));
  assert.ok(names.has("clone"));
  assert.ok(names.has("join"));
  assert.ok(!names.has("as_str"));
});

test("collection member completion exposes Vec methods", () => {
  const source =
    "def main() -> int32:\n    mut values = [1, 2, 3]\n    values.push(4)\n    values.\n    return 0\n";
  const lineIndex = source.split("\n").findIndex((line) => line.includes("values."));
  const lineText = source.split("\n")[lineIndex];
  const character = lineText.indexOf(".") + 1;
  const items = completionsForDocument(source, lineIndex, character, ".");
  const names = new Set(items.map((item) => item.name));

  assert.ok(names.has("len"));
  assert.ok(names.has("is_empty"));
  assert.ok(names.has("push"));
  assert.ok(names.has("pop"));
  assert.ok(names.has("get"));
  assert.ok(names.has("set"));
  assert.ok(names.has("remove"));
  assert.ok(names.has("swap"));
  assert.ok(names.has("contains"));
  assert.ok(names.has("extend"));
  assert.ok(names.has("insert"));
  assert.ok(names.has("clear"));
  assert.ok(names.has("reverse"));
});

test("collection member completion exposes Map methods", () => {
  const source =
    "def main() -> int32:\n    mut counts = Map[String, int32]()\n    counts.\n    return 0\n";
  const lineIndex = source.split("\n").findIndex((line) => line.includes("counts."));
  const lineText = source.split("\n")[lineIndex];
  const character = lineText.indexOf(".") + 1;
  const items = completionsForDocument(source, lineIndex, character, ".");
  const names = new Set(items.map((item) => item.name));

  assert.ok(names.has("len"));
  assert.ok(names.has("is_empty"));
  assert.ok(names.has("clone"));
  assert.ok(names.has("get"));
  assert.ok(names.has("set"));
  assert.ok(names.has("remove"));
  assert.ok(names.has("contains_key"));
  assert.ok(names.has("keys"));
  assert.ok(names.has("values"));
  assert.ok(names.has("items"));
  assert.ok(names.has("entries"));
  assert.ok(names.has("clear"));
  assert.ok(names.has("extend"));
});

test("collection member completion exposes Set methods", () => {
  const source =
    "def main() -> int32:\n    mut seen = Set{1, 2, 3}\n    seen.\n    return 0\n";
  const lineIndex = source.split("\n").findIndex((line) => line.includes("seen."));
  const lineText = source.split("\n")[lineIndex];
  const character = lineText.indexOf(".") + 1;
  const items = completionsForDocument(source, lineIndex, character, ".");
  const names = new Set(items.map((item) => item.name));

  assert.ok(names.has("len"));
  assert.ok(names.has("is_empty"));
  assert.ok(names.has("clone"));
  assert.ok(names.has("contains"));
  assert.ok(names.has("insert"));
  assert.ok(names.has("remove"));
});

test("member completion exposes builtin MapEntry fields", () => {
  const source = [
    "def main() -> int32:",
    "    counts = {\"a\": 1, \"b\": 2}",
    "    entries = counts.items()",
    "    entry = entries[0]",
    "    entry.",
    "    return 0"
  ].join("\n");
  const lineIndex = source.split("\n").findIndex((line) => line.includes("entry."));
  const lineText = source.split("\n")[lineIndex];
  const character = lineText.indexOf(".") + 1;
  const items = completionsForDocument(source, lineIndex, character, ".");
  const names = new Set(items.map((item) => item.name));

  assert.ok(names.has("key"));
  assert.ok(names.has("value"));
});

test("fallback analysis understands builtin io/fs/net module imports", () => {
  const source = [
    "import io",
    "import fs",
    "import net",
    "",
    "def main() -> int32:",
    "    write_result = io.write(\"hello\")",
    "    file_result = fs.open(\"demo.txt\")",
    "    listener_result = net.listen(\"127.0.0.1:0\")",
    "    return 0"
  ].join("\n");

  const diagnostics = diagnosticsForDocument(source);
  assert.ok(!diagnostics.some((diagnostic) => /unknown name `io`/.test(diagnostic.message)));
  assert.ok(!diagnostics.some((diagnostic) => /unknown name `fs`/.test(diagnostic.message)));
  assert.ok(!diagnostics.some((diagnostic) => /unknown name `net`/.test(diagnostic.message)));
  assert.ok(!diagnostics.some((diagnostic) => /type `io` has no member `write`/.test(diagnostic.message)));
  assert.ok(!diagnostics.some((diagnostic) => /type `fs` has no member `open`/.test(diagnostic.message)));
  assert.ok(!diagnostics.some((diagnostic) => /type `net` has no member `listen`/.test(diagnostic.message)));
});

test("fallback member completion exposes builtin module namespaces", () => {
  const source = ["import fs", "", "def main() -> int32:", "    fs.", "    return 0"].join("\n");
  const lineIndex = source.split("\n").findIndex((line) => line.includes("fs."));
  const lineText = source.split("\n")[lineIndex];
  const character = lineText.indexOf(".") + 1;
  const items = completionsForDocument(source, lineIndex, character, ".");
  const names = new Set(items.map((item) => item.name));

  assert.ok(names.has("exists"));
  assert.ok(names.has("read_to_string"));
  assert.ok(names.has("read_bytes"));
  assert.ok(names.has("write_string"));
  assert.ok(names.has("write_bytes"));
  assert.ok(names.has("append_string"));
  assert.ok(names.has("append_bytes"));
  assert.ok(names.has("create_dir"));
  assert.ok(names.has("read_dir"));
  assert.ok(names.has("remove_file"));
  assert.ok(names.has("open"));
  assert.ok(names.has("create"));
  assert.ok(names.has("append"));
  assert.ok(names.has("File"));
});

test("fallback member completion exposes builtin file and socket resource methods", () => {
  const source = [
    "def inspect(file: fs.File, listener: net.TcpListener, stream: net.TcpStream) -> int32:",
    "    file.read_all()",
    "    listener.local_addr()",
    "    stream.peer_addr()",
    "    stream.",
    "    return 0"
  ].join("\n");
  const lineIndex = source.split("\n").findIndex((line) => line.includes("stream."));
  const lineText = source.split("\n")[lineIndex];
  const character = lineText.indexOf(".") + 1;
  const items = completionsForDocument(source, lineIndex, character, ".");
  const names = new Set(items.map((item) => item.name));

  assert.ok(names.has("read_all"));
  assert.ok(names.has("read_line"));
  assert.ok(names.has("read_bytes"));
  assert.ok(names.has("read_exact"));
  assert.ok(names.has("write_all"));
  assert.ok(names.has("write_bytes"));
  assert.ok(names.has("flush"));
  assert.ok(names.has("local_addr"));
  assert.ok(names.has("peer_addr"));
  assert.ok(names.has("shutdown_read"));
  assert.ok(names.has("shutdown_write"));
  assert.ok(names.has("shutdown_both"));
  assert.ok(names.has("close"));
});

test("fallback completion exposes advanced fs/net modules and resource methods", () => {
  const source = [
    "import net",
    "",
    "def inspect(file: fs.File, udp: net.UdpSocket, packet: net.UdpDatagram, http_listener: net.HttpListener, exchange: net.HttpExchange, response: net.HttpResponse, ws_listener: net.WebSocketListener, socket: net.WebSocket, unix_listener: net.UnixListener, unix_stream: net.UnixStream, tls_listener: net.TlsListener, tls_stream: net.TlsStream) -> int32:",
    "    net.",
    "    file.",
    "    udp.",
    "    packet.",
    "    http_listener.",
    "    exchange.",
    "    response.",
    "    ws_listener.",
    "    socket.",
    "    unix_listener.",
    "    unix_stream.",
    "    tls_listener.",
    "    tls_stream.",
    "    return 0"
  ].join("\n");

  const lines = source.split("\n");
  const completeNames = (lineMarker) => {
    const lineIndex = lines.findIndex((line) => line === lineMarker);
    const lineText = lines[lineIndex];
    const character = lineText.indexOf(".") + 1;
    return new Set(
      completionsForDocument(source, lineIndex, character, ".").map((item) => item.name)
    );
  };

  const netNames = completeNames("    net.");
  assert.ok(netNames.has("connect_timeout"));
  assert.ok(netNames.has("udp_bind"));
  assert.ok(netNames.has("http_listen"));
  assert.ok(netNames.has("http_request_text"));
  assert.ok(netNames.has("http_request_text_timeout"));
  assert.ok(netNames.has("http_request_bytes"));
  assert.ok(netNames.has("http_request_bytes_timeout"));
  assert.ok(netNames.has("websocket_listen"));
  assert.ok(netNames.has("websocket_connect"));
  assert.ok(netNames.has("websocket_connect_timeout"));
  assert.ok(netNames.has("unix_listen"));
  assert.ok(netNames.has("unix_connect"));
  assert.ok(netNames.has("unix_connect_timeout"));
  assert.ok(netNames.has("tls_listen"));
  assert.ok(netNames.has("tls_connect"));
  assert.ok(netNames.has("tls_connect_timeout"));
  assert.ok(netNames.has("UdpSocket"));
  assert.ok(netNames.has("HttpResponse"));
  assert.ok(netNames.has("TlsStream"));

  const fileNames = completeNames("    file.");
  assert.ok(fileNames.has("read_bytes"));
  assert.ok(fileNames.has("write_bytes"));

  const udpNames = completeNames("    udp.");
  assert.ok(udpNames.has("send_text"));
  assert.ok(udpNames.has("send_bytes"));
  assert.ok(udpNames.has("recv"));
  assert.ok(udpNames.has("recv_from"));
  assert.ok(udpNames.has("local_addr"));
  assert.ok(udpNames.has("peer_addr"));

  const packetNames = completeNames("    packet.");
  assert.ok(packetNames.has("address"));
  assert.ok(packetNames.has("bytes"));
  assert.ok(packetNames.has("text"));

  const httpListenerNames = completeNames("    http_listener.");
  assert.ok(httpListenerNames.has("accept"));
  assert.ok(httpListenerNames.has("local_addr"));
  assert.ok(httpListenerNames.has("close"));

  const exchangeNames = completeNames("    exchange.");
  assert.ok(exchangeNames.has("method"));
  assert.ok(exchangeNames.has("path"));
  assert.ok(exchangeNames.has("headers"));
  assert.ok(exchangeNames.has("body_text"));
  assert.ok(exchangeNames.has("body_bytes"));
  assert.ok(exchangeNames.has("respond_text"));
  assert.ok(exchangeNames.has("respond_bytes"));

  const responseNames = completeNames("    response.");
  assert.ok(responseNames.has("status"));
  assert.ok(responseNames.has("reason"));
  assert.ok(responseNames.has("headers"));
  assert.ok(responseNames.has("text"));
  assert.ok(responseNames.has("bytes"));

  const wsListenerNames = completeNames("    ws_listener.");
  assert.ok(wsListenerNames.has("accept"));
  assert.ok(wsListenerNames.has("local_addr"));

  const socketNames = completeNames("    socket.");
  assert.ok(socketNames.has("send_text"));
  assert.ok(socketNames.has("send_bytes"));
  assert.ok(socketNames.has("recv_text"));
  assert.ok(socketNames.has("recv_bytes"));

  const unixListenerNames = completeNames("    unix_listener.");
  assert.ok(unixListenerNames.has("accept"));

  const unixStreamNames = completeNames("    unix_stream.");
  assert.ok(unixStreamNames.has("read_line"));
  assert.ok(unixStreamNames.has("read_exact"));
  assert.ok(unixStreamNames.has("write_all"));

  const tlsListenerNames = completeNames("    tls_listener.");
  assert.ok(tlsListenerNames.has("accept"));
  assert.ok(tlsListenerNames.has("local_addr"));

  const tlsStreamNames = completeNames("    tls_stream.");
  assert.ok(tlsStreamNames.has("read_line"));
  assert.ok(tlsStreamNames.has("read_exact"));
  assert.ok(tlsStreamNames.has("write_all"));
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
  assert.ok(names.has("abs"));
  assert.ok(names.has("min"));
  assert.ok(names.has("max"));
  assert.ok(names.has("sqrt"));
  assert.ok(names.has("parse_int32"));
  assert.ok(names.has("parse_int64"));
  assert.ok(names.has("parse_float64"));
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

test("literal match patterns do not report false diagnostics", () => {
  const source = [
    "def describe(flag: bool, value: int32, name: String) -> String:",
    "    match flag:",
    "        case true:",
    "            print(value)",
    "        case false:",
    "            pass",
    "    match value:",
    "        case 0:",
    "            return \"zero\"",
    "        case _:",
    "            pass",
    "    match borrow name:",
    "        case \"aurora\":",
    "            return \"repo\"",
    "        case _:",
    "            return \"other\""
  ].join("\n");

  const diagnostics = diagnosticsForDocument(source);
  assert.equal(diagnostics.length, 0);
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

test("queue and task builtins appear in completions and diagnostics", () => {
  const diagnostics = diagnosticsForDocument(concurrencySource);
  assert.ok(!diagnostics.some((diagnostic) => /unknown name `queue`/.test(diagnostic.message)));
  assert.ok(!diagnostics.some((diagnostic) => /unknown name `spawn`/.test(diagnostic.message)));
  assert.ok(!diagnostics.some((diagnostic) => /unknown name `task`/.test(diagnostic.message)));
  assert.ok(!diagnostics.some((diagnostic) => /unknown name `jobs`/.test(diagnostic.message)));

  const channelLine = concurrencySource.split("\n").findIndex((line) => line.includes("match jobs.get():"));
  const channelText = concurrencySource.split("\n")[channelLine];
  const channelCharacter = channelText.indexOf(".") + 1;
  const channelItems = completionsForDocument(concurrencySource, channelLine, channelCharacter, ".");
  const channelNames = new Set(channelItems.map((item) => item.name));
  assert.ok(channelNames.has("put"));
  assert.ok(channelNames.has("get"));
  assert.ok(channelNames.has("close"));

  const taskLine = concurrencySource.split("\n").findIndex((line) => line.includes("task.result()"));
  const taskText = concurrencySource.split("\n")[taskLine];
  const taskCharacter = taskText.indexOf(".") + 1;
  const taskItems = completionsForDocument(concurrencySource, taskLine, taskCharacter, ".");
  const taskNames = new Set(taskItems.map((item) => item.name));
  assert.ok(taskNames.has("result"));
});

test("structured concurrency bindings and builtins do not report false diagnostics", () => {
  const diagnostics = diagnosticsForDocument(structuredConcurrencySource);
  assert.ok(!diagnostics.some((diagnostic) => /unknown name `tasks`/.test(diagnostic.message)));
  assert.ok(!diagnostics.some((diagnostic) => /unknown name `group`/.test(diagnostic.message)));
  assert.ok(!diagnostics.some((diagnostic) => /unknown name `value`/.test(diagnostic.message)));
  assert.ok(!diagnostics.some((diagnostic) => /unknown name `cancelled`/.test(diagnostic.message)));
});

test("task-group member completion suggests structured concurrency methods", () => {
  const lineIndex = structuredConcurrencySource
    .split("\n")
    .findIndex((line) => line.includes("group.start(worker, out)"));
  const lineText = structuredConcurrencySource.split("\n")[lineIndex];
  const character = lineText.indexOf(".") + 1;
  const items = completionsForDocument(structuredConcurrencySource, lineIndex, character, ".");
  const names = new Set(items.map((item) => item.name));

  assert.ok(names.has("start"));
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

  assert.ok(names.has("tasks"));
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

test("fallback analysis accepts f-strings, copy classes, borrowed match, and queue iteration", () => {
  assert.equal(diagnosticsForDocument(fStringsSource).length, 0);
  assert.equal(diagnosticsForDocument(copyClassSource).length, 0);
  assert.equal(diagnosticsForDocument(matchBorrowSource).length, 0);
  assert.equal(diagnosticsForDocument(queueIterationSource).length, 0);
});

test("fallback analysis helpers split top-level colons through nested delimiters", () => {
  assert.deepEqual(
    _testing.splitTopLevelColon("\"outer\": {\"nested\": [1, 2, {\"name\": \"aurora\"}]}"),
    ["\"outer\"", "{\"nested\": [1, 2, {\"name\": \"aurora\"}]}"]
  );
  assert.deepEqual(
    _testing.splitTopLevelColon("call(coords[0]): value"),
    ["call(coords[0])", "value"]
  );
  assert.deepEqual(
    _testing.splitTopLevelColon("{\"nested\": [1, 2]}: value"),
    ["{\"nested\": [1, 2]}", "value"]
  );
  assert.deepEqual(_testing.splitTopLevelColon("{\"nested\": 1}"), [null, null]);
  assert.deepEqual(
    _testing.splitTopLevelColon("\"escaped\\\"text\" : value"),
    ["\"escaped\\\"text\"", "value"]
  );
});

test("fallback analysis helper splits top-level comma-separated segments through nested brackets", () => {
  assert.deepEqual(
    _testing.splitTopLevelCommaSeparated("call(items[0]), values[1], plain"),
    ["call(items[0])", "values[1]", "plain"]
  );
  assert.deepEqual(
    _testing.splitTopLevelCommaSeparated("\"a\\\",b\", plain, nested[items[0]]"),
    ["\"a\\\",b\"", "plain", "nested[items[0]]"]
  );
});

test("fallback analysis infers nested map literals and tolerates malformed nested-only entries", () => {
  const source = [
    "def main():",
    "    nested = {\"config\": {\"enabled\": true}}",
    "    print(nested)"
  ].join("\n");
  const analysis = analyzeDocument(source);

  assert.equal(analysis.diagnostics.length, 0);
  const lineIndex = 2;
  const character = source.split("\n")[lineIndex].indexOf("nested");
  const hover = hoverForPosition(source, lineIndex, character);
  assert.ok(hover);
  assert.match(hover.value, /local nested: Map\[String, Map\[String, bool\]\]/);
});

test("fallback analysis helper parses callable parameter types through borrow and generics", () => {
  assert.deepEqual(
    _testing.parseParamTypes(
      "count: int32, text: borrow String, values: borrow mut Vec[int32], cfg: Map[String, Vec[int32]]"
    ),
    ["int32", "String", "Vec[int32]", "Map[String, Vec[int32]]"]
  );
  assert.deepEqual(
    _testing.parseParamTypes(
      "left: borrow[shared] String, right: borrow mut[shared] Vec[int32]"
    ),
    ["String", "Vec[int32]"]
  );
  assert.deepEqual(
    _testing.parseParamTypes(
      "items: Vec[int32] = build(defaults[0], {\"seed\": 1}), enabled: bool = true"
    ),
    ["Vec[int32]", "bool"]
  );
  assert.equal(
    _testing.stripTopLevelDefaultValue("Vec[int32] = build(defaults[0], {\"seed\": 1})"),
    "Vec[int32]"
  );
  assert.equal(
    _testing.stripTopLevelDefaultValue("Map[String, int32]) = call()"),
    "Map[String, int32])"
  );
  assert.equal(
    _testing.stripTopLevelDefaultValue("Result[int32, String]} = wrap()"),
    "Result[int32, String]}"
  );
  assert.equal(
    _testing.stripTopLevelDefaultValue("Tuple{value} = make()"),
    "Tuple{value}"
  );
  assert.equal(
    _testing.stripTopLevelDefaultValue("String = say(\"aurora\\\"repo\")"),
    "String"
  );
  assert.equal(
    _testing.stripTopLevelDefaultValue("Vec[String] = wrap((\"a\", \"b\"), [\"c\"])"),
    "Vec[String]"
  );
  assert.equal(
    _testing.stripTopLevelDefaultValue("Call(\"aurora\\\\repo\")"),
    "Call(\"aurora\\\\repo\")"
  );
  assert.equal(
    _testing.stripTopLevelDefaultValue("Call(\"aurora\\\"repo\")"),
    "Call(\"aurora\\\"repo\")"
  );
  assert.equal(
    _testing.stripTopLevelDefaultValue("Wrapper(call(\"aurora\"), Vec[String])"),
    "Wrapper(call(\"aurora\"), Vec[String])"
  );
});

test("fallback analysis helper deduplicates identical diagnostics", () => {
  const moduleInfo = { diagnostics: [] };
  const diagnostic = {
    line: 3,
    startCharacter: 4,
    endCharacter: 9,
    message: "unknown name `value`"
  };

  _testing.pushDiagnosticIfNew(moduleInfo, diagnostic);
  _testing.pushDiagnosticIfNew(moduleInfo, { ...diagnostic });
  _testing.pushDiagnosticIfNew(moduleInfo, { ...diagnostic, endCharacter: 10 });

  assert.deepEqual(moduleInfo.diagnostics, [
    diagnostic,
    { ...diagnostic, endCharacter: 10 }
  ]);
});

test("fallback analysis helper infers loop binding types for vec, set, range, and unknown iterables", () => {
  const source = [
    "def main():",
    "    values = [1, 2, 3]",
    "    seen = Set{true, false}",
    "    for value in values:",
    "        print(value)",
    "    for flag in seen:",
    "        print(flag)"
  ].join("\n");
  const moduleInfo = analyzeDocument(source);
  const functionInfo = moduleInfo.functions.get("main");

  assert.equal(_testing.inferForBindingType("values", moduleInfo, functionInfo), "int32");
  assert.equal(_testing.inferForBindingType("seen", moduleInfo, functionInfo), "bool");
  assert.equal(_testing.inferForBindingType("range(3)", moduleInfo, functionInfo), "int32");
  assert.equal(_testing.inferForBindingType("missing", moduleInfo, functionInfo), null);
});

test("fallback analysis helper infers enum payload bindings from qualified and unqualified match arms", () => {
  const moduleInfo = analyzeDocument([
    "enum Status:",
    "    Ready(code: int32)",
    "    Waiting",
    "",
    "def main(status: Status):",
    "    match status:",
    "        case Status.Ready(code):",
    "            print(code)",
    "        case Ready(other):",
    "            print(other)",
    "        case Option.Some(value):",
    "            print(value)",
    "        case Nope(value):",
    "            print(value)"
  ].join("\n"));

  assert.equal(
    _testing.inferCaseBindingType("case Status.Ready(code):", moduleInfo),
    "code: int32"
  );
  assert.equal(
    _testing.inferCaseBindingType("case Ready(other):", moduleInfo),
    "code: int32"
  );
  assert.equal(
    _testing.inferCaseBindingType("case Option.Some(value):", moduleInfo),
    "T"
  );
  assert.equal(
    _testing.inferCaseBindingType("case Nope(value):", moduleInfo),
    null
  );
  assert.equal(
    _testing.inferCaseBindingType("case Missing.Some(value):", moduleInfo),
    null
  );
  assert.equal(
    _testing.inferCaseBindingType("case _:", moduleInfo),
    null
  );
});

test("fallback analysis helper specializes builtin member return types and unresolved type params", () => {
  const moduleInfo = analyzeDocument([
    "class Widget:",
    "    value: int32",
    "",
    "enum Status:",
    "    Ready",
    "",
    "def main():",
    "    pass"
  ].join("\n"));
  const byName = (group) =>
    new Map(_testing.builtinMembersFor(group).map((item) => [item.name, item]));

  const queueMembers = byName("Queue");
  const mapMembers = byName("Map");
  const vecMembers = byName("Vec");
  const fileMembers = byName("fs.File");
  const tcpStreamMembers = byName("net.TcpStream");
  const udpMembers = byName("net.UdpSocket");
  const datagramMembers = byName("net.UdpDatagram");
  const httpResponseMembers = byName("net.HttpResponse");
  const wsMembers = byName("net.WebSocket");
  assert.equal(
    _testing.specializeMemberReturnType("Vec[String]", vecMembers.get("clone")),
    "Vec[String]"
  );
  assert.equal(
    _testing.specializeMemberReturnType("Vec[String]", vecMembers.get("get")),
    "Option[String]"
  );
  assert.equal(
    _testing.specializeMemberReturnType("Vec[String]", { name: "push", detail: "push(value) -> None" }),
    "None"
  );
  assert.equal(
    _testing.specializeMemberReturnType("Vec", vecMembers.get("remove")),
    "Option[T]"
  );
  assert.equal(
    _testing.specializeMemberReturnType("Map[String, int32]", mapMembers.get("keys")),
    "Vec[String]"
  );
  assert.equal(
    _testing.specializeMemberReturnType("Map[String, int32]", mapMembers.get("values")),
    "Vec[int32]"
  );
  assert.equal(
    _testing.specializeMemberReturnType("Map[String, int32]", mapMembers.get("items")),
    "Vec[MapEntry[String, int32]]"
  );
  assert.equal(
    _testing.specializeMemberReturnType("Map[String, int32]", mapMembers.get("clone")),
    "Map[String, int32]"
  );
  assert.equal(
    _testing.specializeMemberReturnType("Map[String, int32]", mapMembers.get("get")),
    "Option[int32]"
  );
  assert.equal(
    _testing.specializeMemberReturnType("Map[String, int32]", mapMembers.get("set")),
    "Option[int32]"
  );
  assert.equal(
    _testing.specializeMemberReturnType("Map[String, int32]", mapMembers.get("remove")),
    "Option[int32]"
  );
  assert.equal(
    _testing.specializeMemberReturnType("Map", mapMembers.get("clone")),
    "Map[K, V]"
  );
  assert.equal(
    _testing.specializeMemberReturnType("Map[String, int32]", { name: "mystery", detail: "mystery() -> bool" }),
    "bool"
  );

  const setMembers = byName("Set");
  assert.equal(
    _testing.specializeMemberReturnType("Set[String]", setMembers.get("clone")),
    "Set[String]"
  );
  assert.equal(
    _testing.specializeMemberReturnType("Set[String]", setMembers.get("contains")),
    "bool"
  );
  assert.equal(
    _testing.specializeMemberReturnType("Set[String]", setMembers.get("len")),
    "int32"
  );
  assert.equal(
    _testing.specializeMemberReturnType("Set[String]", setMembers.get("remove")),
    "bool"
  );
  assert.equal(
    _testing.specializeMemberReturnType("Set", setMembers.get("contains")),
    "bool"
  );
  assert.equal(
    _testing.specializeMemberReturnType("Set", { name: "mystery", detail: "mystery() -> Option[String]" }),
    "Option[String]"
  );

  const mapEntryMembers = byName("MapEntry");
  assert.equal(
    _testing.specializeMemberReturnType("MapEntry[String, int32]", mapEntryMembers.get("key")),
    "String"
  );
  assert.equal(
    _testing.specializeMemberReturnType("MapEntry[String, int32]", mapEntryMembers.get("value")),
    "int32"
  );
  assert.equal(
    _testing.specializeMemberReturnType("MapEntry", { name: "key", detail: "key: K", type: "K" }),
    "K"
  );
  assert.equal(
    _testing.specializeMemberReturnType(
      "MapEntry[String, int32]",
      { name: "debug", detail: "debug() -> bool", type: "bool" }
    ),
    "bool"
  );

  assert.equal(
    _testing.specializeMemberReturnType("Queue[int32]", queueMembers.get("put")),
    "Result[None, SendError[int32]]"
  );
  assert.equal(
    _testing.specializeMemberReturnType("Queue[int32]", queueMembers.get("close")),
    "None"
  );
  assert.equal(
    _testing.specializeMemberReturnType("Queue", queueMembers.get("get")),
    "Option[T]"
  );
  assert.equal(
    _testing.specializeMemberReturnType("Queue[int32]", queueMembers.get("get")),
    "Option[int32]"
  );
  assert.equal(
    _testing.specializeMemberReturnType("Queue[int32]", { name: "close", detail: "close() -> None" }),
    "None"
  );

  const taskMembers = byName("Task");
  assert.equal(
    _testing.specializeMemberReturnType("Task[int32]", taskMembers.get("result")),
    "int32"
  );
  assert.equal(
    _testing.specializeMemberReturnType("Task", taskMembers.get("result")),
    "T"
  );

  const taskGroupMembers = byName("TaskGroup");
  assert.equal(
    _testing.specializeMemberReturnType("TaskGroup", taskGroupMembers.get("cancel")),
    "None"
  );
  assert.equal(
    _testing.specializeMemberReturnType("TaskGroup", taskGroupMembers.get("start")),
    "Task[T]"
  );
  assert.equal(
    _testing.specializeMemberReturnType("fs.File", fileMembers.get("read_bytes")),
    "Result[Vec[uint8], io.Error]"
  );
  assert.equal(
    _testing.specializeMemberReturnType("net.TcpStream", tcpStreamMembers.get("read_bytes")),
    "Result[Option[Vec[uint8]], io.Error]"
  );
  assert.equal(
    _testing.specializeMemberReturnType("net.UdpSocket", udpMembers.get("recv_from")),
    "Result[Option[net.UdpDatagram], io.Error]"
  );
  assert.equal(
    _testing.specializeMemberReturnType("net.UdpDatagram", datagramMembers.get("bytes")),
    "Vec[uint8]"
  );
  assert.equal(
    _testing.specializeMemberReturnType("net.HttpResponse", httpResponseMembers.get("status")),
    "int32"
  );
  assert.equal(
    _testing.specializeMemberReturnType("net.HttpResponse", httpResponseMembers.get("headers")),
    "Map[String, String]"
  );
  assert.equal(
    _testing.specializeMemberReturnType("net.WebSocket", wsMembers.get("recv_bytes")),
    "Result[Option[Vec[uint8]], io.Error]"
  );
  assert.equal(
    _testing.specializeMemberReturnType("UnknownType", { name: "value", detail: "value() -> Result[int32, String]" }),
    "Result[int32, String]"
  );

  assert.equal(_testing.baseTypeName("Map[String, int32]"), "Map");
  assert.equal(_testing.isUnresolvedTypeParamType(moduleInfo, "T"), true);
  assert.equal(_testing.isUnresolvedTypeParamType(moduleInfo, "int32"), false);
  assert.equal(_testing.isUnresolvedTypeParamType(moduleInfo, "Widget"), false);
  assert.equal(_testing.isUnresolvedTypeParamType(moduleInfo, "Status"), false);
  assert.equal(_testing.isUnresolvedTypeParamType(moduleInfo, "widget"), false);
  assert.equal(
    _testing.parseBuiltinDetailReturnType("spawn(function, ...) -> Task[T]"),
    "Task[T]"
  );
});

test("fallback analysis testing helpers expose builtin metadata and utility helpers", () => {
  const builtinMemberGroups = [
    "float64",
    "String",
    "Vec",
    "Map",
    "Set",
    "MapEntry",
    "fs.File",
    "net.TcpListener",
    "net.TcpStream",
    "net.UdpSocket",
    "net.UdpDatagram",
    "net.HttpListener",
    "net.HttpExchange",
    "net.HttpResponse",
    "net.WebSocketListener",
    "net.WebSocket",
    "net.UnixListener",
    "net.UnixStream",
    "net.TlsListener",
    "net.TlsStream",
    "Queue",
    "Task",
    "TaskGroup"
  ];
  for (const group of builtinMemberGroups) {
    const items = _testing.builtinMembersFor(group);
    assert.ok(items.length > 0, `expected builtin members for ${group}`);
    for (const item of items) {
      assert.equal(typeof item.name, "string");
      assert.equal(typeof item.detail, "string");
      assert.equal(typeof item.documentation, "string");
      assert.ok(item.documentation.length > 0);
    }
  }

  for (const item of _testing.builtinFunctions()) {
    assert.equal(typeof item.name, "string");
    assert.equal(typeof item.detail, "string");
    assert.equal(typeof item.documentation, "string");
  }

  for (const enumInfo of _testing.builtinEnums()) {
    assert.equal(typeof enumInfo.name, "string");
    assert.equal(typeof enumInfo.detail, "string");
    assert.equal(typeof enumInfo.documentation, "string");
    assert.ok(Array.isArray(enumInfo.variants));
    assert.ok(enumInfo.variants.length > 0);
  }

  assert.equal(_testing.countIndent("    value"), 4);
  assert.equal(_testing.countIndent("value"), 0);
  assert.equal(_testing.isIdentifierStart("a"), true);
  assert.equal(_testing.isIdentifierStart("_"), true);
  assert.equal(_testing.isIdentifierStart("1"), false);
  assert.equal(_testing.isIdentifierContinue("a"), true);
  assert.equal(_testing.isIdentifierContinue("1"), true);
  assert.equal(_testing.isIdentifierContinue("-"), false);
  assert.equal(_testing.findReceiverStart("value", -1), -1);
  assert.equal(_testing.findReceiverStart("call(value)", "call(value)".length - 1), 4);
  assert.equal(
    _testing.findReceiverStart("outer(inner(value))", "outer(inner(value))".length - 1),
    5
  );
  assert.equal(_testing.findReceiverStart("value)", "value)".length - 1), -1);
  assert.equal(_testing.findReceiverStart("value+", "value+".length - 1), -1);
  assert.equal(_testing.extractReceiverEndingBefore("value).", "value).".length), null);
  assert.equal(
    _testing.formatVariantHover({ name: "Ready", payloadType: null, returnType: "Status" }, "Status"),
    "```aurora\nvariant Ready -> Status\n```"
  );

  const callableModule = analyzeDocument([
    "def helper(value: int32) -> int32:",
    "    return value",
    "",
    "class Box:",
    "    value: int32",
    "    def read(borrow self) -> int32:",
    "        return self.value"
  ].join("\n"));
  assert.equal(_testing.allCallableInfos(callableModule).length, 2);
});
