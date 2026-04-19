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
  "return",
  "try",
  "as",
  "and",
  "or",
  "not",
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
  "io.Error",
  "fs.File",
  "process.Child",
  "process.Pipe",
  "process.Completed",
  "process.ExitStatus",
  "process.Wait",
  "process.Stdio",
  "process.Error",
  "process.Supervisor",
  "process.RestartPolicy",
  "process.SupervisorEvent",
  "process.SupervisorWait",
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
  "QueueReceive",
  "SendError",
  "Task",
  "TaskResult",
  "WaitAny",
  "WaitAll",
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
  io: [
    {
      name: "write",
      kind: "method",
      detail: "write(text: String) -> Result[None, io.Error]",
      documentation: "Writes `text` to standard output without appending a newline."
    },
    {
      name: "flush",
      kind: "method",
      detail: "flush() -> Result[None, io.Error]",
      documentation: "Flushes standard output."
    },
    {
      name: "read_line",
      kind: "method",
      detail: "read_line() -> Result[Option[String], io.Error]",
      documentation: "Reads the next UTF-8 line from standard input without the trailing newline."
    },
    {
      name: "Error",
      kind: "field",
      detail: "Error: io.Error",
      type: "io.Error",
      documentation: "The builtin I/O error enum."
    }
  ],
  fs: [
    {
      name: "exists",
      kind: "method",
      detail: "exists(path: String) -> bool",
      documentation: "Returns true when `path` exists on disk."
    },
    {
      name: "read_to_string",
      kind: "method",
      detail: "read_to_string(path: String) -> Result[String, io.Error]",
      documentation: "Reads the entire file at `path` as UTF-8 text."
    },
    {
      name: "read_bytes",
      kind: "method",
      detail: "read_bytes(path: String) -> Result[Vec[uint8], io.Error]",
      documentation: "Reads the entire file at `path` as raw bytes."
    },
    {
      name: "write_string",
      kind: "method",
      detail: "write_string(path: String, text: String) -> Result[None, io.Error]",
      documentation: "Creates or truncates `path` and writes `text` to it."
    },
    {
      name: "write_bytes",
      kind: "method",
      detail: "write_bytes(path: String, bytes: Vec[uint8]) -> Result[None, io.Error]",
      documentation: "Creates or truncates `path` and writes `bytes` to it."
    },
    {
      name: "append_string",
      kind: "method",
      detail: "append_string(path: String, text: String) -> Result[None, io.Error]",
      documentation: "Appends `text` to the file at `path`."
    },
    {
      name: "append_bytes",
      kind: "method",
      detail: "append_bytes(path: String, bytes: Vec[uint8]) -> Result[None, io.Error]",
      documentation: "Appends `bytes` to the file at `path`."
    },
    {
      name: "create_dir",
      kind: "method",
      detail: "create_dir(path: String) -> Result[None, io.Error]",
      documentation: "Creates the directory at `path`."
    },
    {
      name: "read_dir",
      kind: "method",
      detail: "read_dir(path: String) -> Result[Vec[String], io.Error]",
      documentation: "Returns the entry names in the directory at `path`."
    },
    {
      name: "remove_file",
      kind: "method",
      detail: "remove_file(path: String) -> Result[None, io.Error]",
      documentation: "Removes the file at `path`."
    },
    {
      name: "open",
      kind: "method",
      detail: "open(path: String) -> Result[fs.File, io.Error]",
      documentation: "Opens `path` for reading."
    },
    {
      name: "create",
      kind: "method",
      detail: "create(path: String) -> Result[fs.File, io.Error]",
      documentation: "Creates or truncates `path` and returns a writable file handle."
    },
    {
      name: "append",
      kind: "method",
      detail: "append(path: String) -> Result[fs.File, io.Error]",
      documentation: "Opens `path` for appending and returns a writable file handle."
    },
    {
      name: "File",
      kind: "field",
      detail: "File: fs.File",
      type: "fs.File",
      documentation: "The builtin file-handle resource type."
    }
  ],
  "fs.File": [
    {
      name: "read_all",
      kind: "method",
      detail: "read_all() -> Result[String, io.Error]",
      documentation: "Reads the remaining contents of the file as UTF-8 text."
    },
    {
      name: "read_bytes",
      kind: "method",
      detail: "read_bytes() -> Result[Vec[uint8], io.Error]",
      documentation: "Reads the remaining contents of the file as raw bytes."
    },
    {
      name: "write_all",
      kind: "method",
      detail: "write_all(text: String) -> Result[None, io.Error]",
      documentation: "Writes all of `text` to the file."
    },
    {
      name: "write_bytes",
      kind: "method",
      detail: "write_bytes(bytes: Vec[uint8]) -> Result[None, io.Error]",
      documentation: "Writes all of `bytes` to the file."
    },
    {
      name: "flush",
      kind: "method",
      detail: "flush() -> Result[None, io.Error]",
      documentation: "Flushes buffered file output."
    },
    {
      name: "close",
      kind: "method",
      detail: "close() -> Result[None, io.Error]",
      documentation: "Closes the file handle."
    }
  ],
  process: [
    {
      name: "supervisor",
      kind: "method",
      detail: "supervisor() -> process.Supervisor",
      documentation: "Creates a process supervisor for named child processes with restart policies and backoff."
    },
    {
      name: "start",
      kind: "method",
      detail: "start(command: Vec[String], cwd: Option[String] = Option.None, env: Map[String, String] = {}, stdin: process.Stdio = process.null(), stdout: process.Stdio = process.inherit(), stderr: process.Stdio = process.inherit(), group: bool = false) -> Result[process.Child, process.Error]",
      documentation: "Starts a child process without waiting for it to complete. When `group=true`, the child starts in its own process group and lifecycle operations apply to that group."
    },
    {
      name: "run",
      kind: "method",
      detail: "run(command: Vec[String], cwd: Option[String] = Option.None, env: Map[String, String] = {}, stdin: process.Stdio = process.null(), stdout: process.Stdio = process.pipe(), stderr: process.Stdio = process.pipe(), timeout: Duration = ..., group: bool = false) -> Result[process.Completed, process.Error]",
      documentation: "Runs a child process to completion, optionally capturing stdout and stderr. When `group=true`, timeout and cleanup behavior apply to the full child process group."
    },
    {
      name: "inherit",
      kind: "method",
      detail: "inherit() -> process.Stdio",
      documentation: "Uses the parent process stdio stream directly."
    },
    {
      name: "null",
      kind: "method",
      detail: "null() -> process.Stdio",
      documentation: "Connects the child stdio stream to the null device."
    },
    {
      name: "pipe",
      kind: "method",
      detail: "pipe() -> process.Stdio",
      documentation: "Creates a pipe-backed child stdio stream."
    },
    {
      name: "Child",
      kind: "field",
      detail: "Child: process.Child",
      type: "process.Child",
      documentation: "The builtin running child-process resource type."
    },
    {
      name: "Pipe",
      kind: "field",
      detail: "Pipe: process.Pipe",
      type: "process.Pipe",
      documentation: "The builtin child-process pipe resource type."
    },
    {
      name: "Completed",
      kind: "field",
      detail: "Completed: process.Completed",
      type: "process.Completed",
      documentation: "The builtin completed-process capture value type."
    },
    {
      name: "ExitStatus",
      kind: "field",
      detail: "ExitStatus: process.ExitStatus",
      type: "process.ExitStatus",
      documentation: "The builtin process exit-status enum."
    },
    {
      name: "Wait",
      kind: "field",
      detail: "Wait: process.Wait",
      type: "process.Wait",
      documentation: "The builtin child wait-result enum."
    },
    {
      name: "Stdio",
      kind: "field",
      detail: "Stdio: process.Stdio",
      type: "process.Stdio",
      documentation: "The builtin child stdio configuration enum."
    },
    {
      name: "Error",
      kind: "field",
      detail: "Error: process.Error",
      type: "process.Error",
      documentation: "The builtin process error enum."
    },
    {
      name: "Supervisor",
      kind: "field",
      detail: "Supervisor: process.Supervisor",
      type: "process.Supervisor",
      documentation: "The builtin process supervisor resource type."
    },
    {
      name: "RestartPolicy",
      kind: "field",
      detail: "RestartPolicy: process.RestartPolicy",
      type: "process.RestartPolicy",
      documentation: "The builtin process supervisor restart-policy enum."
    },
    {
      name: "SupervisorEvent",
      kind: "field",
      detail: "SupervisorEvent: process.SupervisorEvent",
      type: "process.SupervisorEvent",
      documentation: "The builtin process supervisor event enum."
    },
    {
      name: "SupervisorWait",
      kind: "field",
      detail: "SupervisorWait: process.SupervisorWait",
      type: "process.SupervisorWait",
      documentation: "The builtin process supervisor wait-result enum."
    }
  ],
  "process.Child": [
    {
      name: "stdin",
      kind: "method",
      detail: "stdin() -> Option[process.Pipe]",
      documentation: "Returns the child stdin pipe when one was requested."
    },
    {
      name: "stdout",
      kind: "method",
      detail: "stdout() -> Option[process.Pipe]",
      documentation: "Returns the child stdout pipe when one was requested."
    },
    {
      name: "stderr",
      kind: "method",
      detail: "stderr() -> Option[process.Pipe]",
      documentation: "Returns the child stderr pipe when one was requested."
    },
    {
      name: "wait",
      kind: "method",
      detail: "wait(timeout: Duration = ...) -> process.Wait",
      documentation: "Waits for the child process to finish, optionally timing out."
    },
    {
      name: "wait_or_none",
      kind: "method",
      detail: "wait_or_none(timeout: Duration = ...) -> Result[Option[process.ExitStatus], process.Error]",
      documentation: "Waits for the child to finish and returns `Option.None` on timeout."
    },
    {
      name: "wait_ok",
      kind: "method",
      detail: "wait_ok(timeout: Duration = ...) -> Result[process.ExitStatus, process.Error]",
      documentation:
        "Waits for the child to exit successfully, treating timeouts, cancellation, and non-zero exits as `process.Error`."
    },
    {
      name: "kill",
      kind: "method",
      detail: "kill() -> Result[None, process.Error]",
      documentation: "Forcefully terminates the child process, or the full child process group when the child was started with `group=true`."
    },
    {
      name: "terminate",
      kind: "method",
      detail: "terminate() -> Result[None, process.Error]",
      documentation: "Requests graceful child termination, or graceful termination of the full child process group when the child was started with `group=true`."
    },
    {
      name: "close",
      kind: "method",
      detail: "close() -> None",
      documentation: "Closes the child handle and remaining attached pipes, terminating the full child process group first when the child was started with `group=true`."
    }
  ],
  "process.Pipe": [
    {
      name: "read_all",
      kind: "method",
      detail: "read_all() -> Result[String, process.Error]",
      documentation: "Reads the remaining pipe contents as UTF-8 text."
    },
    {
      name: "read_line",
      kind: "method",
      detail: "read_line(timeout: Duration = ...) -> Result[Option[String], process.Error]",
      documentation: "Reads the next UTF-8 line from the pipe."
    },
    {
      name: "read_bytes",
      kind: "method",
      detail: "read_bytes(max_bytes: int32, timeout: Duration = ...) -> Result[Option[Vec[uint8]], process.Error]",
      documentation: "Reads up to `max_bytes` raw bytes from the pipe."
    },
    {
      name: "write_all",
      kind: "method",
      detail: "write_all(text: String, timeout: Duration = ...) -> Result[None, process.Error]",
      documentation: "Writes all of `text` to the pipe."
    },
    {
      name: "write_bytes",
      kind: "method",
      detail: "write_bytes(bytes: Vec[uint8], timeout: Duration = ...) -> Result[None, process.Error]",
      documentation: "Writes all of `bytes` to the pipe."
    },
    {
      name: "flush",
      kind: "method",
      detail: "flush() -> Result[None, process.Error]",
      documentation: "Flushes buffered pipe output."
    },
    {
      name: "close",
      kind: "method",
      detail: "close() -> None",
      documentation: "Closes the pipe handle."
    }
  ],
  "process.Completed": [
    {
      name: "status",
      kind: "method",
      detail: "status() -> process.ExitStatus",
      documentation: "Returns the completed child exit status."
    },
    {
      name: "success",
      kind: "method",
      detail: "success() -> bool",
      documentation: "Returns true when the child exited with code 0."
    },
    {
      name: "stdout",
      kind: "method",
      detail: "stdout() -> String",
      documentation: "Returns the captured stdout text."
    },
    {
      name: "stderr",
      kind: "method",
      detail: "stderr() -> String",
      documentation: "Returns the captured stderr text."
    },
    {
      name: "check",
      kind: "method",
      detail: "check() -> Result[None, process.Error]",
      documentation:
        "Returns `Result.Ok(None)` when the child exited successfully and `Result.Err(process.Error)` otherwise."
    }
  ],
  "process.Supervisor": [
    {
      name: "start",
      kind: "method",
      detail:
        "start(name: String, command: Vec[String], cwd: Option[String] = Option.None, env: Map[String, String] = {}, stdin: process.Stdio = process.null(), stdout: process.Stdio = process.inherit(), stderr: process.Stdio = process.inherit(), restart: process.RestartPolicy = process.RestartPolicy.OnFailure, backoff: Duration = 100ms, max_restarts: int32 = -1, group: bool = true) -> Result[None, process.Error]",
      documentation:
        "Starts a named supervised child process. Supervisor children default to `group=true`, and restart policy, backoff, and restart count control automatic restart behavior."
    },
    {
      name: "wait",
      kind: "method",
      detail: "wait(timeout: Duration = ...) -> process.SupervisorWait",
      documentation: "Waits for the next supervisor event, timeout, or cancellation outcome."
    },
    {
      name: "wait_or_none",
      kind: "method",
      detail: "wait_or_none(timeout: Duration = ...) -> Result[Option[process.SupervisorEvent], process.Error]",
      documentation: "Waits for the next supervisor event and returns `Option.None` on timeout."
    },
    {
      name: "stop",
      kind: "method",
      detail: "stop() -> Result[None, process.Error]",
      documentation: "Stops every supervised child process and clears the supervisor."
    },
    {
      name: "is_empty",
      kind: "method",
      detail: "is_empty() -> bool",
      documentation: "Returns true when the supervisor currently has no managed services."
    },
    {
      name: "close",
      kind: "method",
      detail: "close() -> None",
      documentation: "Closes the supervisor and stops every remaining supervised child process."
    }
  ],
  net: [
    {
      name: "connect",
      kind: "method",
      detail: "connect(address: String) -> Result[net.TcpStream, io.Error]",
      documentation: "Connects to a TCP server at `address`."
    },
    {
      name: "connect_timeout",
      kind: "method",
      detail: "connect_timeout(address: String, timeout: Duration) -> Result[net.TcpStream, io.Error]",
      documentation: "Connects to a TCP server at `address`, failing when the timeout expires."
    },
    {
      name: "listen",
      kind: "method",
      detail: "listen(address: String) -> Result[net.TcpListener, io.Error]",
      documentation: "Starts a TCP listener bound to `address`."
    },
    {
      name: "udp_bind",
      kind: "method",
      detail: "udp_bind(address: String) -> Result[net.UdpSocket, io.Error]",
      documentation: "Binds a UDP socket to `address`."
    },
    {
      name: "http_listen",
      kind: "method",
      detail: "http_listen(address: String) -> Result[net.HttpListener, io.Error]",
      documentation: "Starts a blocking HTTP listener bound to `address`."
    },
    {
      name: "http_request_text",
      kind: "method",
      detail: "http_request_text(method: String, url: String, body: String, headers: Map[String, String]) -> Result[net.HttpResponse, io.Error]",
      documentation: "Performs an HTTP request with a UTF-8 request body."
    },
    {
      name: "http_request_text_timeout",
      kind: "method",
      detail: "http_request_text_timeout(method: String, url: String, body: String, headers: Map[String, String], timeout: Duration) -> Result[net.HttpResponse, io.Error]",
      documentation: "Performs an HTTP request with a UTF-8 request body and explicit timeout."
    },
    {
      name: "http_request_bytes",
      kind: "method",
      detail: "http_request_bytes(method: String, url: String, bytes: Vec[uint8], headers: Map[String, String]) -> Result[net.HttpResponse, io.Error]",
      documentation: "Performs an HTTP request with a binary request body."
    },
    {
      name: "http_request_bytes_timeout",
      kind: "method",
      detail: "http_request_bytes_timeout(method: String, url: String, bytes: Vec[uint8], headers: Map[String, String], timeout: Duration) -> Result[net.HttpResponse, io.Error]",
      documentation: "Performs an HTTP request with a binary request body and explicit timeout."
    },
    {
      name: "websocket_listen",
      kind: "method",
      detail: "websocket_listen(address: String) -> Result[net.WebSocketListener, io.Error]",
      documentation: "Starts a blocking WebSocket listener bound to `address`."
    },
    {
      name: "websocket_connect",
      kind: "method",
      detail: "websocket_connect(url: String) -> Result[net.WebSocket, io.Error]",
      documentation: "Connects to a WebSocket server."
    },
    {
      name: "websocket_connect_timeout",
      kind: "method",
      detail: "websocket_connect_timeout(url: String, timeout: Duration) -> Result[net.WebSocket, io.Error]",
      documentation: "Connects to a WebSocket server with an explicit timeout."
    },
    {
      name: "unix_listen",
      kind: "method",
      detail: "unix_listen(path: String) -> Result[net.UnixListener, io.Error]",
      documentation: "Starts a Unix domain stream listener at `path`."
    },
    {
      name: "unix_connect",
      kind: "method",
      detail: "unix_connect(path: String) -> Result[net.UnixStream, io.Error]",
      documentation: "Connects to a Unix domain stream socket."
    },
    {
      name: "unix_connect_timeout",
      kind: "method",
      detail: "unix_connect_timeout(path: String, timeout: Duration) -> Result[net.UnixStream, io.Error]",
      documentation: "Connects to a Unix domain stream socket with an explicit timeout."
    },
    {
      name: "tls_listen",
      kind: "method",
      detail: "tls_listen(address: String, cert_pem_path: String, key_pem_path: String) -> Result[net.TlsListener, io.Error]",
      documentation: "Starts a TLS listener using PEM certificate and key files."
    },
    {
      name: "tls_connect",
      kind: "method",
      detail: "tls_connect(address: String, server_name: String, ca_pem_path: String) -> Result[net.TlsStream, io.Error]",
      documentation: "Connects to a TLS server using a PEM certificate authority bundle."
    },
    {
      name: "tls_connect_timeout",
      kind: "method",
      detail: "tls_connect_timeout(address: String, server_name: String, ca_pem_path: String, timeout: Duration) -> Result[net.TlsStream, io.Error]",
      documentation: "Connects to a TLS server using a PEM certificate authority bundle and explicit timeout."
    },
    {
      name: "TcpStream",
      kind: "field",
      detail: "TcpStream: net.TcpStream",
      type: "net.TcpStream",
      documentation: "The builtin TCP stream resource type."
    },
    {
      name: "TcpListener",
      kind: "field",
      detail: "TcpListener: net.TcpListener",
      type: "net.TcpListener",
      documentation: "The builtin TCP listener resource type."
    },
    {
      name: "UdpSocket",
      kind: "field",
      detail: "UdpSocket: net.UdpSocket",
      type: "net.UdpSocket",
      documentation: "The builtin UDP socket resource type."
    },
    {
      name: "UdpDatagram",
      kind: "field",
      detail: "UdpDatagram: net.UdpDatagram",
      type: "net.UdpDatagram",
      documentation: "The builtin received UDP datagram value type."
    },
    {
      name: "HttpListener",
      kind: "field",
      detail: "HttpListener: net.HttpListener",
      type: "net.HttpListener",
      documentation: "The builtin blocking HTTP listener resource type."
    },
    {
      name: "HttpExchange",
      kind: "field",
      detail: "HttpExchange: net.HttpExchange",
      type: "net.HttpExchange",
      documentation: "The builtin HTTP request/response exchange resource type."
    },
    {
      name: "HttpResponse",
      kind: "field",
      detail: "HttpResponse: net.HttpResponse",
      type: "net.HttpResponse",
      documentation: "The builtin HTTP response resource type."
    },
    {
      name: "WebSocketListener",
      kind: "field",
      detail: "WebSocketListener: net.WebSocketListener",
      type: "net.WebSocketListener",
      documentation: "The builtin blocking WebSocket listener resource type."
    },
    {
      name: "WebSocket",
      kind: "field",
      detail: "WebSocket: net.WebSocket",
      type: "net.WebSocket",
      documentation: "The builtin WebSocket connection resource type."
    },
    {
      name: "UnixListener",
      kind: "field",
      detail: "UnixListener: net.UnixListener",
      type: "net.UnixListener",
      documentation: "The builtin Unix domain stream listener resource type."
    },
    {
      name: "UnixStream",
      kind: "field",
      detail: "UnixStream: net.UnixStream",
      type: "net.UnixStream",
      documentation: "The builtin Unix domain stream connection resource type."
    },
    {
      name: "TlsListener",
      kind: "field",
      detail: "TlsListener: net.TlsListener",
      type: "net.TlsListener",
      documentation: "The builtin TLS listener resource type."
    },
    {
      name: "TlsStream",
      kind: "field",
      detail: "TlsStream: net.TlsStream",
      type: "net.TlsStream",
      documentation: "The builtin TLS connection resource type."
    }
  ],
  "net.TcpListener": [
    {
      name: "accept",
      kind: "method",
      detail: "accept(timeout: Duration = ...) -> Result[net.TcpStream, io.Error]",
      documentation: "Accepts the next incoming TCP connection, optionally timing out."
    },
    {
      name: "local_addr",
      kind: "method",
      detail: "local_addr() -> Result[String, io.Error]",
      documentation: "Returns the bound local listener address."
    },
    {
      name: "close",
      kind: "method",
      detail: "close() -> None",
      documentation: "Closes the TCP listener."
    }
  ],
  "net.TcpStream": [
    {
      name: "read_all",
      kind: "method",
      detail: "read_all(timeout: Duration = ...) -> Result[String, io.Error]",
      documentation: "Reads the remaining stream contents as UTF-8 text until the peer closes."
    },
    {
      name: "read_line",
      kind: "method",
      detail: "read_line(timeout: Duration = ...) -> Result[Option[String], io.Error]",
      documentation: "Reads the next UTF-8 line from the stream without the trailing newline."
    },
    {
      name: "read_bytes",
      kind: "method",
      detail: "read_bytes(max_bytes: int32, timeout: Duration = ...) -> Result[Option[Vec[uint8]], io.Error]",
      documentation: "Reads up to `max_bytes` raw bytes from the TCP stream."
    },
    {
      name: "read_exact",
      kind: "method",
      detail: "read_exact(count: int32, timeout: Duration = ...) -> Result[Vec[uint8], io.Error]",
      documentation: "Reads exactly `count` raw bytes from the TCP stream."
    },
    {
      name: "write_all",
      kind: "method",
      detail: "write_all(text: String, timeout: Duration = ...) -> Result[None, io.Error]",
      documentation: "Writes all of `text` to the stream."
    },
    {
      name: "write_bytes",
      kind: "method",
      detail: "write_bytes(bytes: Vec[uint8], timeout: Duration = ...) -> Result[None, io.Error]",
      documentation: "Writes all of `bytes` to the stream."
    },
    {
      name: "flush",
      kind: "method",
      detail: "flush() -> Result[None, io.Error]",
      documentation: "Flushes buffered stream output."
    },
    {
      name: "local_addr",
      kind: "method",
      detail: "local_addr() -> Result[String, io.Error]",
      documentation: "Returns the local socket address."
    },
    {
      name: "peer_addr",
      kind: "method",
      detail: "peer_addr() -> Result[String, io.Error]",
      documentation: "Returns the peer socket address."
    },
    {
      name: "shutdown_read",
      kind: "method",
      detail: "shutdown_read() -> Result[None, io.Error]",
      documentation: "Shuts down the read half of the TCP stream."
    },
    {
      name: "shutdown_write",
      kind: "method",
      detail: "shutdown_write() -> Result[None, io.Error]",
      documentation: "Shuts down the write half of the TCP stream."
    },
    {
      name: "shutdown_both",
      kind: "method",
      detail: "shutdown_both() -> Result[None, io.Error]",
      documentation: "Shuts down both halves of the TCP stream."
    },
    {
      name: "close",
      kind: "method",
      detail: "close() -> None",
      documentation: "Closes the TCP stream."
    }
  ],
  "net.UdpSocket": [
    {
      name: "send_text",
      kind: "method",
      detail: "send_text(address: String, text: String, timeout: Duration = ...) -> Result[None, io.Error]",
      documentation: "Sends UTF-8 text to a UDP address."
    },
    {
      name: "send_bytes",
      kind: "method",
      detail: "send_bytes(address: String, bytes: Vec[uint8], timeout: Duration = ...) -> Result[None, io.Error]",
      documentation: "Sends raw bytes to a UDP address."
    },
    {
      name: "recv",
      kind: "method",
      detail: "recv(max_bytes: int32, timeout: Duration = ...) -> Result[Option[Vec[uint8]], io.Error]",
      documentation: "Receives raw bytes from a connected UDP socket."
    },
    {
      name: "recv_from",
      kind: "method",
      detail: "recv_from(max_bytes: int32, timeout: Duration = ...) -> Result[Option[net.UdpDatagram], io.Error]",
      documentation: "Receives a datagram and source address from a UDP socket."
    },
    {
      name: "local_addr",
      kind: "method",
      detail: "local_addr() -> Result[String, io.Error]",
      documentation: "Returns the local address for the UDP socket."
    },
    {
      name: "peer_addr",
      kind: "method",
      detail: "peer_addr() -> Result[String, io.Error]",
      documentation: "Returns the connected peer address for the UDP socket."
    },
    {
      name: "close",
      kind: "method",
      detail: "close() -> None",
      documentation: "Closes the UDP socket handle."
    }
  ],
  "net.UdpDatagram": [
    {
      name: "address",
      kind: "method",
      detail: "address() -> String",
      documentation: "Returns the source address for the UDP datagram."
    },
    {
      name: "bytes",
      kind: "method",
      detail: "bytes() -> Vec[uint8]",
      documentation: "Returns the datagram payload as raw bytes."
    },
    {
      name: "text",
      kind: "method",
      detail: "text() -> Result[String, io.Error]",
      documentation: "Decodes the datagram payload as UTF-8 text."
    }
  ],
  "net.HttpListener": [
    {
      name: "accept",
      kind: "method",
      detail: "accept(timeout: Duration = ...) -> Result[net.HttpExchange, io.Error]",
      documentation: "Accepts the next incoming HTTP request."
    },
    {
      name: "local_addr",
      kind: "method",
      detail: "local_addr() -> Result[String, io.Error]",
      documentation: "Returns the bound local address for the HTTP listener."
    },
    {
      name: "close",
      kind: "method",
      detail: "close() -> None",
      documentation: "Closes the HTTP listener handle."
    }
  ],
  "net.HttpExchange": [
    {
      name: "method",
      kind: "method",
      detail: "method() -> String",
      documentation: "Returns the HTTP request method."
    },
    {
      name: "path",
      kind: "method",
      detail: "path() -> String",
      documentation: "Returns the HTTP request path."
    },
    {
      name: "headers",
      kind: "method",
      detail: "headers() -> Map[String, String]",
      documentation: "Returns the HTTP request headers as a map."
    },
    {
      name: "body_text",
      kind: "method",
      detail: "body_text() -> Result[String, io.Error]",
      documentation: "Returns the HTTP request body decoded as UTF-8."
    },
    {
      name: "body_bytes",
      kind: "method",
      detail: "body_bytes() -> Vec[uint8]",
      documentation: "Returns the HTTP request body as raw bytes."
    },
    {
      name: "respond_text",
      kind: "method",
      detail: "respond_text(status: int32, text: String, headers: Map[String, String]) -> Result[None, io.Error]",
      documentation: "Sends a text HTTP response for the current request."
    },
    {
      name: "respond_bytes",
      kind: "method",
      detail: "respond_bytes(status: int32, bytes: Vec[uint8], headers: Map[String, String]) -> Result[None, io.Error]",
      documentation: "Sends a binary HTTP response for the current request."
    },
    {
      name: "close",
      kind: "method",
      detail: "close() -> None",
      documentation: "Closes the HTTP exchange handle."
    }
  ],
  "net.HttpResponse": [
    {
      name: "status",
      kind: "method",
      detail: "status() -> int32",
      documentation: "Returns the HTTP response status code."
    },
    {
      name: "reason",
      kind: "method",
      detail: "reason() -> String",
      documentation: "Returns the HTTP response reason phrase."
    },
    {
      name: "headers",
      kind: "method",
      detail: "headers() -> Map[String, String]",
      documentation: "Returns the HTTP response headers as a map."
    },
    {
      name: "text",
      kind: "method",
      detail: "text() -> Result[String, io.Error]",
      documentation: "Returns the HTTP response body decoded as UTF-8."
    },
    {
      name: "bytes",
      kind: "method",
      detail: "bytes() -> Vec[uint8]",
      documentation: "Returns the HTTP response body as raw bytes."
    },
    {
      name: "close",
      kind: "method",
      detail: "close() -> None",
      documentation: "Closes the HTTP response handle."
    }
  ],
  "net.WebSocketListener": [
    {
      name: "accept",
      kind: "method",
      detail: "accept(timeout: Duration = ...) -> Result[net.WebSocket, io.Error]",
      documentation: "Accepts the next incoming WebSocket connection."
    },
    {
      name: "local_addr",
      kind: "method",
      detail: "local_addr() -> Result[String, io.Error]",
      documentation: "Returns the bound local address for the WebSocket listener."
    },
    {
      name: "close",
      kind: "method",
      detail: "close() -> None",
      documentation: "Closes the WebSocket listener handle."
    }
  ],
  "net.WebSocket": [
    {
      name: "send_text",
      kind: "method",
      detail: "send_text(text: String, timeout: Duration = ...) -> Result[None, io.Error]",
      documentation: "Sends a text WebSocket frame."
    },
    {
      name: "send_bytes",
      kind: "method",
      detail: "send_bytes(bytes: Vec[uint8], timeout: Duration = ...) -> Result[None, io.Error]",
      documentation: "Sends a binary WebSocket frame."
    },
    {
      name: "recv_text",
      kind: "method",
      detail: "recv_text(timeout: Duration = ...) -> Result[Option[String], io.Error]",
      documentation: "Receives the next text WebSocket frame."
    },
    {
      name: "recv_bytes",
      kind: "method",
      detail: "recv_bytes(timeout: Duration = ...) -> Result[Option[Vec[uint8]], io.Error]",
      documentation: "Receives the next binary WebSocket frame."
    },
    {
      name: "close",
      kind: "method",
      detail: "close() -> None",
      documentation: "Closes the WebSocket connection."
    }
  ],
  "net.UnixListener": [
    {
      name: "accept",
      kind: "method",
      detail: "accept(timeout: Duration = ...) -> Result[net.UnixStream, io.Error]",
      documentation: "Accepts the next incoming Unix domain stream connection."
    },
    {
      name: "close",
      kind: "method",
      detail: "close() -> None",
      documentation: "Closes the Unix listener handle."
    }
  ],
  "net.UnixStream": [
    {
      name: "read_line",
      kind: "method",
      detail: "read_line(timeout: Duration = ...) -> Result[Option[String], io.Error]",
      documentation: "Reads a UTF-8 line from the Unix stream."
    },
    {
      name: "read_exact",
      kind: "method",
      detail: "read_exact(count: int32, timeout: Duration = ...) -> Result[Vec[uint8], io.Error]",
      documentation: "Reads exactly `count` bytes from the Unix stream."
    },
    {
      name: "write_all",
      kind: "method",
      detail: "write_all(text: String, timeout: Duration = ...) -> Result[None, io.Error]",
      documentation: "Writes all of `text` to the Unix stream."
    },
    {
      name: "close",
      kind: "method",
      detail: "close() -> None",
      documentation: "Closes the Unix stream handle."
    }
  ],
  "net.TlsListener": [
    {
      name: "accept",
      kind: "method",
      detail: "accept(timeout: Duration = ...) -> Result[net.TlsStream, io.Error]",
      documentation: "Accepts the next incoming TLS connection."
    },
    {
      name: "local_addr",
      kind: "method",
      detail: "local_addr() -> Result[String, io.Error]",
      documentation: "Returns the bound local address for the TLS listener."
    },
    {
      name: "close",
      kind: "method",
      detail: "close() -> None",
      documentation: "Closes the TLS listener handle."
    }
  ],
  "net.TlsStream": [
    {
      name: "read_line",
      kind: "method",
      detail: "read_line(timeout: Duration = ...) -> Result[Option[String], io.Error]",
      documentation: "Reads a UTF-8 line from the TLS stream."
    },
    {
      name: "read_exact",
      kind: "method",
      detail: "read_exact(count: int32, timeout: Duration = ...) -> Result[Vec[uint8], io.Error]",
      documentation: "Reads exactly `count` bytes from the TLS stream."
    },
    {
      name: "write_all",
      kind: "method",
      detail: "write_all(text: String, timeout: Duration = ...) -> Result[None, io.Error]",
      documentation: "Writes all of `text` to the TLS stream."
    },
    {
      name: "close",
      kind: "method",
      detail: "close() -> None",
      documentation: "Closes the TLS stream handle."
    }
  ],
  Queue: [
    {
      name: "put",
      kind: "method",
      detail: "put(value, timeout: Duration = ...) -> Result[None, SendError[T]]",
      documentation:
        "Puts a value into the queue, waiting for free capacity when needed, or returns `SendError.Closed(value)`, `SendError.Cancelled(value)`, `SendError.TimedOut(value)`, or `SendError.Full(value)` if the send cannot complete."
    },
    {
      name: "try_put",
      kind: "method",
      detail: "try_put(value) -> Result[None, SendError[T]]",
      documentation: "Attempts a non-blocking queue send and returns the unsent value on failure."
    },
    {
      name: "get",
      kind: "method",
      detail: "get(timeout: Duration = ...) -> QueueReceive[T]",
      documentation:
        "Receives the next value from the queue and reports `QueueReceive.Item(value)`, `QueueReceive.Closed`, `QueueReceive.TimedOut`, or `QueueReceive.Cancelled`."
    },
    {
      name: "get_or_none",
      kind: "method",
      detail: "get_or_none(timeout: Duration = ...) -> Option[T]",
      documentation:
        "Receives the next value from the queue, returning `Option.None` when the queue times out, is cancelled, or is closed and empty."
    },
    {
      name: "get_or",
      kind: "method",
      detail: "get_or(default: T, timeout: Duration = ...) -> T",
      documentation:
        "Receives the next value from the queue or returns `default` when the queue times out, is cancelled, or is closed and empty."
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
      detail: "result(timeout: Duration = ...) -> TaskResult[T]",
      documentation:
        "Waits for the task to finish and reports `TaskResult.Ready(value)`, `TaskResult.TimedOut`, or `TaskResult.Cancelled`."
    },
    {
      name: "result_or_none",
      kind: "method",
      detail: "result_or_none(timeout: Duration = ...) -> Option[T]",
      documentation:
        "Waits for the task result and returns `Option.None` when the task times out or is cancelled."
    },
    {
      name: "result_or",
      kind: "method",
      detail: "result_or(default: T, timeout: Duration = ...) -> T",
      documentation:
        "Waits for the task result or returns `default` when the task times out or is cancelled."
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
      name: "start_soon",
      kind: "method",
      detail: "start_soon(function, ...) -> None",
      documentation: "Starts a child task in the current task group without returning a task handle."
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
    name: "Queue",
    kind: "function",
    detail: "Queue(capacity: int32 = ...) -> Queue[T]",
    documentation: "Creates a typed queue, optionally with bounded capacity."
  },
  {
    name: "TaskGroup",
    kind: "function",
    detail: "TaskGroup() -> TaskGroup",
    documentation: "Creates a managed structured-concurrency task group for use with `with`."
  },
  {
    name: "cancelled",
    kind: "function",
    detail: "cancelled() -> bool",
    documentation: "Returns true when the current task has been cancelled."
  },
  {
    name: "wait_any",
    kind: "function",
    detail: "wait_any(tasks: Vec[Task[T]], timeout: Duration = ...) -> WaitAny[T]",
    documentation: "Waits for the first task in a task list to finish, or returns a timeout/cancelled result."
  },
  {
    name: "wait_all",
    kind: "function",
    detail: "wait_all(tasks: Vec[Task[T]], timeout: Duration = ...) -> WaitAll[T]",
    documentation: "Waits for every task in a task list to finish, or returns a timeout/cancelled result."
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
const BUILTIN_MODULE_NAMES = new Set(["io", "fs", "process", "net"]);
const BUILTIN_ENUMS = new Map([
  [
    "QueueReceive",
    {
      kind: "enum",
      name: "QueueReceive",
      detail: "enum QueueReceive[T]",
      documentation: "Queue receive outcomes that distinguish values, closure, timeouts, and cancellation.",
      variants: [
        {
          kind: "variant",
          name: "Item",
          returnType: "QueueReceive",
          payloadType: "T",
          detail: "Item(T) -> QueueReceive"
        },
        {
          kind: "variant",
          name: "Closed",
          returnType: "QueueReceive",
          payloadType: null,
          detail: "Closed -> QueueReceive"
        },
        {
          kind: "variant",
          name: "TimedOut",
          returnType: "QueueReceive",
          payloadType: null,
          detail: "TimedOut -> QueueReceive"
        },
        {
          kind: "variant",
          name: "Cancelled",
          returnType: "QueueReceive",
          payloadType: null,
          detail: "Cancelled -> QueueReceive"
        }
      ]
    }
  ],
  [
    "Stdio",
    {
      kind: "enum",
      name: "Stdio",
      detail: "enum Stdio",
      documentation: "Process stdio configuration values.",
      variants: [
        { kind: "variant", name: "Inherit", returnType: "Stdio", payloadType: null, detail: "Inherit -> Stdio" },
        { kind: "variant", name: "Null", returnType: "Stdio", payloadType: null, detail: "Null -> Stdio" },
        { kind: "variant", name: "Pipe", returnType: "Stdio", payloadType: null, detail: "Pipe -> Stdio" }
      ]
    }
  ],
  [
    "ExitStatus",
    {
      kind: "enum",
      name: "ExitStatus",
      detail: "enum ExitStatus",
      documentation: "Process exit results for normal exits and signal termination.",
      variants: [
        { kind: "variant", name: "Exited", returnType: "ExitStatus", payloadType: "int32", detail: "Exited(int32) -> ExitStatus" },
        { kind: "variant", name: "Signaled", returnType: "ExitStatus", payloadType: "int32", detail: "Signaled(int32) -> ExitStatus" }
      ]
    }
  ],
  [
    "Wait",
    {
      kind: "enum",
      name: "Wait",
      detail: "enum Wait",
      documentation: "Child-process wait outcomes.",
      variants: [
        { kind: "variant", name: "Exited", returnType: "Wait", payloadType: "process.ExitStatus", detail: "Exited(process.ExitStatus) -> Wait" },
        { kind: "variant", name: "TimedOut", returnType: "Wait", payloadType: null, detail: "TimedOut -> Wait" },
        { kind: "variant", name: "Cancelled", returnType: "Wait", payloadType: null, detail: "Cancelled -> Wait" },
        { kind: "variant", name: "Failed", returnType: "Wait", payloadType: "process.Error", detail: "Failed(process.Error) -> Wait" }
      ]
    }
  ],
  [
    "Error",
    {
      kind: "enum",
      name: "Error",
      detail: "enum Error",
      documentation: "Process API errors.",
      variants: [
        { kind: "variant", name: "NoCommand", returnType: "Error", payloadType: null, detail: "NoCommand -> Error" },
        { kind: "variant", name: "TimedOut", returnType: "Error", payloadType: null, detail: "TimedOut -> Error" },
        { kind: "variant", name: "Cancelled", returnType: "Error", payloadType: null, detail: "Cancelled -> Error" },
        { kind: "variant", name: "Io", returnType: "Error", payloadType: "io.Error", detail: "Io(io.Error) -> Error" },
        { kind: "variant", name: "Spawn", returnType: "Error", payloadType: "String", detail: "Spawn(String) -> Error" },
        { kind: "variant", name: "Other", returnType: "Error", payloadType: "String", detail: "Other(String) -> Error" }
      ]
    }
  ],
  [
    "RestartPolicy",
    {
      kind: "enum",
      name: "RestartPolicy",
      detail: "enum RestartPolicy",
      documentation: "Supervisor restart policies for managed child processes.",
      variants: [
        { kind: "variant", name: "Never", returnType: "RestartPolicy", payloadType: null, detail: "Never -> RestartPolicy" },
        {
          kind: "variant",
          name: "OnFailure",
          returnType: "RestartPolicy",
          payloadType: null,
          detail: "OnFailure -> RestartPolicy"
        },
        { kind: "variant", name: "Always", returnType: "RestartPolicy", payloadType: null, detail: "Always -> RestartPolicy" }
      ]
    }
  ],
  [
    "SupervisorEvent",
    {
      kind: "enum",
      name: "SupervisorEvent",
      detail: "enum SupervisorEvent",
      documentation: "Supervisor lifecycle events for exited, restarted, and failed services.",
      variants: [
        {
          kind: "variant",
          name: "Exited",
          returnType: "SupervisorEvent",
          payloadType: "String, process.ExitStatus, int32",
          detail: "Exited(String, process.ExitStatus, int32) -> SupervisorEvent"
        },
        {
          kind: "variant",
          name: "Restarted",
          returnType: "SupervisorEvent",
          payloadType: "String, process.ExitStatus, int32",
          detail: "Restarted(String, process.ExitStatus, int32) -> SupervisorEvent"
        },
        {
          kind: "variant",
          name: "Failed",
          returnType: "SupervisorEvent",
          payloadType: "String, process.Error, int32",
          detail: "Failed(String, process.Error, int32) -> SupervisorEvent"
        }
      ]
    }
  ],
  [
    "SupervisorWait",
    {
      kind: "enum",
      name: "SupervisorWait",
      detail: "enum SupervisorWait",
      documentation: "Supervisor wait outcomes.",
      variants: [
        {
          kind: "variant",
          name: "Event",
          returnType: "SupervisorWait",
          payloadType: "process.SupervisorEvent",
          detail: "Event(process.SupervisorEvent) -> SupervisorWait"
        },
        { kind: "variant", name: "TimedOut", returnType: "SupervisorWait", payloadType: null, detail: "TimedOut -> SupervisorWait" },
        { kind: "variant", name: "Cancelled", returnType: "SupervisorWait", payloadType: null, detail: "Cancelled -> SupervisorWait" }
      ]
    }
  ],
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
        },
        {
          kind: "variant",
          name: "Cancelled",
          returnType: "SendError",
          payloadType: "T",
          detail: "Cancelled(T) -> SendError"
        },
        {
          kind: "variant",
          name: "TimedOut",
          returnType: "SendError",
          payloadType: "T",
          detail: "TimedOut(T) -> SendError"
        },
        {
          kind: "variant",
          name: "Full",
          returnType: "SendError",
          payloadType: "T",
          detail: "Full(T) -> SendError"
        }
      ]
    }
  ],
  [
    "TaskResult",
    {
      kind: "enum",
      name: "TaskResult",
      detail: "enum TaskResult[T]",
      documentation: "Task completion outcomes for structured task waits.",
      variants: [
        {
          kind: "variant",
          name: "Ready",
          returnType: "TaskResult",
          payloadType: "T",
          detail: "Ready(T) -> TaskResult"
        },
        {
          kind: "variant",
          name: "TimedOut",
          returnType: "TaskResult",
          payloadType: null,
          detail: "TimedOut -> TaskResult"
        },
        {
          kind: "variant",
          name: "Cancelled",
          returnType: "TaskResult",
          payloadType: null,
          detail: "Cancelled -> TaskResult"
        }
      ]
    }
  ],
  [
    "WaitAny",
    {
      kind: "enum",
      name: "WaitAny",
      detail: "enum WaitAny[T]",
      documentation: "Waits for the first task in a list to finish.",
      variants: [
        {
          kind: "variant",
          name: "Ready",
          returnType: "WaitAny",
          payloadType: "int32, T",
          detail: "Ready(int32, T) -> WaitAny"
        },
        {
          kind: "variant",
          name: "TimedOut",
          returnType: "WaitAny",
          payloadType: null,
          detail: "TimedOut -> WaitAny"
        },
        {
          kind: "variant",
          name: "Cancelled",
          returnType: "WaitAny",
          payloadType: null,
          detail: "Cancelled -> WaitAny"
        }
      ]
    }
  ],
  [
    "WaitAll",
    {
      kind: "enum",
      name: "WaitAll",
      detail: "enum WaitAll[T]",
      documentation: "Waits for every task in a list to finish.",
      variants: [
        {
          kind: "variant",
          name: "Ready",
          returnType: "WaitAll",
          payloadType: "Vec[T]",
          detail: "Ready(Vec[T]) -> WaitAll"
        },
        {
          kind: "variant",
          name: "TimedOut",
          returnType: "WaitAll",
          payloadType: null,
          detail: "TimedOut -> WaitAll"
        },
        {
          kind: "variant",
          name: "Cancelled",
          returnType: "WaitAll",
          payloadType: null,
          detail: "Cancelled -> WaitAll"
        }
      ]
    }
  ],
  [
    "io.Error",
    {
      kind: "enum",
      name: "io.Error",
      detail: "enum io.Error",
      documentation: "Built-in I/O and network error values.",
      variants: [
        { kind: "variant", name: "NotFound", returnType: "io.Error", payloadType: null, detail: "NotFound -> io.Error" },
        { kind: "variant", name: "PermissionDenied", returnType: "io.Error", payloadType: null, detail: "PermissionDenied -> io.Error" },
        { kind: "variant", name: "AlreadyExists", returnType: "io.Error", payloadType: null, detail: "AlreadyExists -> io.Error" },
        { kind: "variant", name: "ConnectionRefused", returnType: "io.Error", payloadType: null, detail: "ConnectionRefused -> io.Error" },
        { kind: "variant", name: "ConnectionReset", returnType: "io.Error", payloadType: null, detail: "ConnectionReset -> io.Error" },
        { kind: "variant", name: "ConnectionAborted", returnType: "io.Error", payloadType: null, detail: "ConnectionAborted -> io.Error" },
        { kind: "variant", name: "NotConnected", returnType: "io.Error", payloadType: null, detail: "NotConnected -> io.Error" },
        { kind: "variant", name: "AddrInUse", returnType: "io.Error", payloadType: null, detail: "AddrInUse -> io.Error" },
        { kind: "variant", name: "AddrNotAvailable", returnType: "io.Error", payloadType: null, detail: "AddrNotAvailable -> io.Error" },
        { kind: "variant", name: "BrokenPipe", returnType: "io.Error", payloadType: null, detail: "BrokenPipe -> io.Error" },
        { kind: "variant", name: "TimedOut", returnType: "io.Error", payloadType: null, detail: "TimedOut -> io.Error" },
        { kind: "variant", name: "WouldBlock", returnType: "io.Error", payloadType: null, detail: "WouldBlock -> io.Error" },
        { kind: "variant", name: "UnexpectedEof", returnType: "io.Error", payloadType: null, detail: "UnexpectedEof -> io.Error" },
        { kind: "variant", name: "InvalidInput", returnType: "io.Error", payloadType: null, detail: "InvalidInput -> io.Error" },
        { kind: "variant", name: "InvalidData", returnType: "io.Error", payloadType: null, detail: "InvalidData -> io.Error" },
        { kind: "variant", name: "Closed", returnType: "io.Error", payloadType: null, detail: "Closed -> io.Error" },
        { kind: "variant", name: "Other", returnType: "io.Error", payloadType: "String", detail: "Other(String) -> io.Error" }
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

    if (registerBuiltinImport(moduleInfo, rawLine, trimmed, i)) {
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

function registerBuiltinImport(moduleInfo, rawLine, trimmed, line) {
  const importMatch = trimmed.match(/^import\s+([a-z][A-Za-z0-9_.]*)$/);
  if (importMatch && BUILTIN_MODULE_NAMES.has(importMatch[1])) {
    const name = importMatch[1];
    moduleInfo.topLevelBindings.set(name, {
      kind: "module",
      name,
      type: name,
      detail: `module ${name}`,
      moduleScoped: true,
      line,
      startCharacter: rawLine.indexOf(name),
      endCharacter: rawLine.indexOf(name) + name.length
    });
    return true;
  }

  const fromImportMatch = trimmed.match(/^from\s+([a-z][A-Za-z0-9_.]*)\s+import\s+(.+)$/);
  if (!fromImportMatch || !BUILTIN_MODULE_NAMES.has(fromImportMatch[1])) {
    return false;
  }

  const moduleName = fromImportMatch[1];
  const names = splitTopLevelCommaSeparated(fromImportMatch[2]);
  for (const importedName of names) {
    const name = importedName.trim();
    const exportSymbol = (BUILTIN_MEMBERS[moduleName] || []).find((item) => item.name === name);
    if (!name || !exportSymbol) {
      continue;
    }

    const startCharacter = rawLine.indexOf(name);
    if (exportSymbol.kind === "method") {
      moduleInfo.topLevelBindings.set(name, {
        kind: "function",
        name,
        detail: exportSymbol.detail,
        returnType: parseBuiltinDetailReturnType(exportSymbol.detail) || "None",
        documentation: exportSymbol.documentation,
        moduleScoped: true,
        line,
        startCharacter,
        endCharacter: startCharacter + name.length
      });
      continue;
    }

    moduleInfo.topLevelBindings.set(name, {
      kind: "binding",
      name,
      type: exportSymbol.type || "None",
      detail: exportSymbol.detail,
      documentation: exportSymbol.documentation,
      moduleScoped: true,
      line,
      startCharacter,
      endCharacter: startCharacter + name.length
    });
  }
  return true;
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
        const inferredType = inferCaseBindingType(trimmed, moduleInfo, functionInfo, lines, i) || "Unknown";
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
  if (
    /^(?:copy\s+)?class\b/.test(trimmed) ||
    /^enum\b/.test(trimmed) ||
    /^def\b/.test(trimmed) ||
    /^import\b/.test(trimmed) ||
    /^from\b/.test(trimmed)
  ) {
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
  for (const binding of moduleInfo.topLevelBindings.values()) {
    completions.push({
      name: binding.name,
      kind: binding.kind || "binding",
      detail: binding.detail || binding.type || "Aurora binding"
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

  if (moduleInfo.topLevelBindings.has(name)) {
    const symbol = moduleInfo.topLevelBindings.get(name);
    if (functionInfo && !symbol.moduleScoped) {
      return null;
    }
    if (symbol.kind === "function") {
      return {
        ...symbol,
        hover: formatCallableHover("function", symbol.name, [], symbol.returnType || "None")
      };
    }
    return {
      ...symbol,
      hover: formatHover(symbol.kind || "binding", symbol.name, symbol.type || "None")
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
    const importedBinding = moduleInfo.topLevelBindings.get(functionMatch[1]);
    if (importedBinding && importedBinding.kind === "function") {
      return importedBinding.returnType || parseBuiltinDetailReturnType(importedBinding.detail);
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
  const match = detail.match(/->\s*([A-Za-z_][A-Za-z0-9_.,\[\] ]*)/);
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
      return `QueueReceive[${inner}]`;
    }
    if (member.name === "get_or_none") {
      return `Option[${inner}]`;
    }
    if (member.name === "get_or") {
      return inner;
    }
    if (member.name === "put" || member.name === "try_put") {
      return `Result[None, SendError[${inner}]]`;
    }
    return "None";
  }

  if (base === "Task") {
    const match = receiverType.match(/^Task\[(.+)\]$/);
    if (!match) {
      return parseBuiltinDetailReturnType(member.detail);
    }
    const inner = normalizeType(match[1]);
    if (member.name === "result") {
      return `TaskResult[${inner}]`;
    }
    if (member.name === "result_or_none") {
      return `Option[${inner}]`;
    }
    if (member.name === "result_or") {
      return inner;
    }
    return parseBuiltinDetailReturnType(member.detail);
  }

  if (base === "process.Child") {
    if (member.name === "wait_or_none") {
      return "Result[Option[process.ExitStatus], process.Error]";
    }
    if (member.name === "wait_ok") {
      return "Result[process.ExitStatus, process.Error]";
    }
    return parseBuiltinDetailReturnType(member.detail);
  }

  if (base === "process.Completed") {
    if (member.name === "check") {
      return "Result[None, process.Error]";
    }
    return parseBuiltinDetailReturnType(member.detail);
  }

  if (base === "process.Supervisor") {
    if (member.name === "start") {
      return "Result[None, process.Error]";
    }
    if (member.name === "wait") {
      return "process.SupervisorWait";
    }
    if (member.name === "wait_or_none") {
      return "Result[Option[process.SupervisorEvent], process.Error]";
    }
    if (member.name === "stop") {
      return "Result[None, process.Error]";
    }
    if (member.name === "is_empty") {
      return "bool";
    }
    if (member.name === "close") {
      return "None";
    }
    return parseBuiltinDetailReturnType(member.detail);
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

function inferCaseBindingType(trimmed, moduleInfo, functionInfo, lines, lineIndex) {
  const match = trimmed.match(
    /^case\s+(?:([A-Z][A-Za-z0-9_]*)\.)?([A-Z][A-Za-z0-9_]*)\([a-zA-Z_][A-Za-z0-9_]*\)\s*:/
  );
  if (!match) {
    return null;
  }
  const enclosingMatchType = inferEnclosingMatchType(lines, lineIndex, moduleInfo, functionInfo);
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
    return variant
      ? specializeBuiltinEnumPayloadType(match[1], variant.payloadType, enclosingMatchType)
      : null;
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
      return specializeBuiltinEnumPayloadType(builtinEnum.name, variant.payloadType, enclosingMatchType);
    }
  }
  return null;
}

function inferEnclosingMatchType(lines, lineIndex, moduleInfo, functionInfo) {
  if (!Array.isArray(lines) || typeof lineIndex !== "number" || lineIndex < 0) {
    return null;
  }
  const currentLine = lines[lineIndex] || "";
  const currentIndent = currentLine.match(/^\s*/)[0].length;
  for (let i = lineIndex - 1; i >= 0; i -= 1) {
    const line = lines[i] || "";
    const trimmed = line.trim();
    if (!trimmed) {
      continue;
    }
    const indent = line.match(/^\s*/)[0].length;
    if (indent >= currentIndent) {
      continue;
    }
    const match = trimmed.match(/^match\s+(.+)\s*:\s*$/);
    if (!match) {
      continue;
    }
    return inferExpressionType(match[1], moduleInfo, functionInfo);
  }
  return null;
}

function builtinEnumTypeParamMap(enumName, matchedType) {
  const normalized = matchedType ? normalizeType(matchedType) : null;
  if (!normalized) {
    return null;
  }
  const names = {
    Option: ["T"],
    Result: ["T", "E"],
    SendError: ["T"],
    QueueReceive: ["T"],
    TaskResult: ["T"],
    WaitAny: ["T"],
    WaitAll: ["T"]
  };
  const params = names[enumName];
  if (!params) {
    return null;
  }
  const match = normalized.match(/^([A-Za-z0-9_.]+)\[(.+)\]$/);
  if (!match || match[1] !== enumName) {
    return null;
  }
  const args = splitTopLevelCommaSeparated(match[2]).map(normalizeType);
  if (args.length < params.length) {
    return null;
  }
  const replacements = new Map();
  params.forEach((param, index) => {
    replacements.set(param, args[index]);
  });
  return replacements;
}

function specializeBuiltinEnumPayloadType(enumName, payloadType, matchedType) {
  if (!payloadType) {
    return null;
  }
  const replacements = builtinEnumTypeParamMap(enumName, matchedType);
  if (!replacements) {
    return payloadType;
  }
  let specialized = payloadType;
  for (const [param, value] of replacements.entries()) {
    specialized = specialized.replace(new RegExp(`\\b${param}\\b`, "g"), value);
  }
  return specialized;
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
