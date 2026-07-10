use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use aurora_compiler::{check_path_with_source, run_path_with_source};
#[cfg(unix)]
use rcgen::generate_simple_self_signed;

struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn new(prefix: &str) -> Self {
        let unique = format!(
            "{}-{}-{}",
            prefix,
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("system time should be after unix epoch")
                .as_nanos()
        );
        let path = std::env::temp_dir().join(unique);
        fs::create_dir_all(&path).expect("failed to create temp dir");
        Self { path }
    }

    fn path(&self) -> &PathBuf {
        &self.path
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

#[test]
fn builtin_io_modules_type_check_from_path_context() {
    let temp = TempDir::new("aurora-io-check");
    let entry = temp.path().join("main.au");
    let source = r#"import io
import fs
import net

def read_file(path: String) -> Result[String, io.Error]:
    with file = try fs.open(path):
        return file.read_all()

def send_line(stream: net.TcpStream, text: String) -> Result[None, io.Error]:
    with socket = stream:
        try socket.write_all(text)
        try socket.flush()
        return Result.Ok(None)

def main() -> int32:
    return 0
"#;

    check_path_with_source(&entry, source).expect("builtin io modules should type-check");
}

#[test]
fn builtin_fs_and_net_modules_run_through_public_api() {
    let temp = TempDir::new("aurora-io-run");
    let entry = temp.path().join("main.au");
    let file_path = temp.path().join("data.txt");
    let source = format!(
        r#"import io
import fs
import net

def serve(listener: net.TcpListener) -> Result[None, io.Error]:
    with server_listener = listener:
        socket = try server_listener.accept()
        with server_stream = socket:
            line = try server_stream.read_line()
            match line:
                case Option.Some(text):
                    try server_stream.write_all("echo:" + text)
                    try server_stream.flush()
                case Option.None:
                    pass
            return Result.Ok(None)

def run() -> Result[None, io.Error]:
    try fs.write_string("{path}", "alpha")
    text = try fs.read_to_string("{path}")
    try io.write(text + "\n")

    with TaskGroup() as group:
        listener = try net.listen("127.0.0.1:0")
        address = try listener.local_addr()
        server = group.start(serve, listener)
        client = try net.connect(address)
        with client_stream = client:
            try client_stream.write_all("ping\n")
            try client_stream.flush()
            response = try client_stream.read_all()
            try io.write(response)
        match server.result():
            case TaskResult.Ready(result):
                try result
            case TaskResult.Error(_message):
                print("server task failed")
                return Result.Ok(None)
            case TaskResult.Cancelled:
                print("server task cancelled")
                return Result.Ok(None)
            case TaskResult.TimedOut:
                print("server task timed out")
                return Result.Ok(None)

    return Result.Ok(None)

def main() -> int32:
    match run():
        case Result.Ok(_):
            return 0
        case Result.Err(error):
            print(error)
            return 1

"#,
        path = file_path.display()
    );

    let output = run_path_with_source(&entry, &source)
        .expect("builtin fs and net modules should run through public API");
    assert_eq!(output.stdout, "alpha\necho:ping");
}

#[test]
fn advanced_io_and_network_surface_type_checks_from_path_context() {
    let temp = TempDir::new("aurora-io-advanced-check");
    let entry = temp.path().join("main.au");
    let source = r#"import io
import fs
import net

def touch_bytes(path: String) -> Result[Vec[uint8], io.Error]:
    seed: Vec[uint8] = [65 as uint8, 66 as uint8]
    try fs.write_bytes(path.clone(), seed)
    try fs.append_bytes(path.clone(), [67 as uint8])
    with file = try fs.append(path.clone()):
        try file.write_bytes([68 as uint8])
        try file.flush()
    return fs.read_bytes(path)

def inspect_udp(socket: net.UdpSocket) -> Result[String, io.Error]:
    with bound = socket:
        match try bound.recv_from(1024, timeout=100ms):
            case Option.Some(packet):
                text = try packet.text()
                bytes = packet.bytes()
                print(packet.address())
                print(bytes.len())
                return Result.Ok(text)
            case Option.None:
                return Result.Ok("none")

def inspect_http(listener: net.HttpListener) -> Result[None, io.Error]:
    with bound = listener:
        exchange = try bound.accept(timeout=100ms)
        with request = exchange:
            method = request.method()
            path = request.path()
            headers = request.headers()
            body_text = try request.body_text()
            body_bytes = request.body_bytes()
            try request.respond_text(200, method + path + body_text, headers.clone())
            try request.respond_bytes(201, body_bytes, headers)
            return Result.Ok(None)

def inspect_http_response(response: net.HttpResponse) -> Result[String, io.Error]:
    with received = response:
        print(received.status())
        print(received.reason())
        print(received.headers()["Content-Type"])
        text = try received.text()
        bytes = received.bytes()
        print(bytes.len())
        return Result.Ok(text)

def inspect_websocket(listener: net.WebSocketListener, client: net.WebSocket) -> Result[None, io.Error]:
    with bound = listener:
        with accepted = try bound.accept(timeout=100ms):
            match try accepted.recv_text(timeout=100ms):
                case Option.Some(text):
                    try accepted.send_text(text, timeout=100ms)
                case Option.None:
                    pass
            match try accepted.recv_bytes(timeout=100ms):
                case Option.Some(bytes):
                    try accepted.send_bytes(bytes, timeout=100ms)
                case Option.None:
                    pass
    with connected = client:
        try connected.send_text("ping", timeout=100ms)
        try connected.send_bytes([1 as uint8, 2 as uint8], timeout=100ms)
        return Result.Ok(None)

def inspect_unix(listener: net.UnixListener, stream: net.UnixStream) -> Result[None, io.Error]:
    with bound = listener:
        accepted = try bound.accept(timeout=100ms)
        with server_stream = accepted:
            match try server_stream.read_line(timeout=100ms):
                case Option.Some(text):
                    try server_stream.write_all(text, timeout=100ms)
                case Option.None:
                    pass
    with client_stream = stream:
        exact = try client_stream.read_exact(2, timeout=100ms)
        print(exact.len())
        try client_stream.write_all("ok\n", timeout=100ms)
        return Result.Ok(None)

def inspect_tls(listener: net.TlsListener, stream: net.TlsStream) -> Result[None, io.Error]:
    with bound = listener:
        print(try bound.local_addr())
        accepted = try bound.accept(timeout=100ms)
        with server_stream = accepted:
            match try server_stream.read_line(timeout=100ms):
                case Option.Some(text):
                    try server_stream.write_all(text, timeout=100ms)
                case Option.None:
                    pass
    with client_stream = stream:
        exact = try client_stream.read_exact(2, timeout=100ms)
        print(exact.len())
        try client_stream.write_all("tls\n", timeout=100ms)
        return Result.Ok(None)

def main() -> int32:
    headers: Map[String, String] = {"Content-Type": "text/plain"}
    print(net.http_request_text("GET", "http://127.0.0.1:1", "", headers.clone()))
    print(net.http_request_text_timeout("GET", "http://127.0.0.1:1", "", headers.clone(), 1s))
    print(net.http_request_bytes("POST", "http://127.0.0.1:1", [1 as uint8], headers.clone()))
    print(net.http_request_bytes_timeout("POST", "http://127.0.0.1:1", [1 as uint8], headers, 1s))
    print(net.connect_timeout("127.0.0.1:1", 1s))
    print(net.udp_bind("127.0.0.1:0"))
    print(net.http_listen("127.0.0.1:0"))
    print(net.websocket_listen("127.0.0.1:0"))
    print(net.websocket_connect("ws://127.0.0.1:1/"))
    print(net.websocket_connect_timeout("ws://127.0.0.1:1/", 1s))
    print(net.unix_listen("/tmp/aurora-check.sock"))
    print(net.unix_connect("/tmp/aurora-check.sock"))
    print(net.unix_connect_timeout("/tmp/aurora-check.sock", 1s))
    print(net.tls_listen("127.0.0.1:0", "cert.pem", "key.pem"))
    print(net.tls_connect("127.0.0.1:1", "localhost", "ca.pem"))
    print(net.tls_connect_timeout("127.0.0.1:1", "localhost", "ca.pem", 1s))
    return 0
"#;

    check_path_with_source(&entry, source)
        .expect("advanced builtin io/net surface should type-check");
}

#[test]
fn advanced_io_and_network_modules_run_through_public_api() {
    let temp = TempDir::new("aurora-io-advanced-run");
    let entry = temp.path().join("main.au");
    let file_path = temp.path().join("data.bin");
    let source = format!(
        r#"import io
import fs
import net

def serve_udp(socket: net.UdpSocket) -> Result[String, io.Error]:
    with server_socket = socket:
        match try server_socket.recv_from(1024, timeout=1s):
            case Option.Some(packet):
                text = try packet.text()
                try server_socket.send_text(packet.address(), "udp:" + text, timeout=1s)
                return Result.Ok(text)
            case Option.None:
                return Result.Ok("missing")

def serve_http(listener: net.HttpListener) -> Result[None, io.Error]:
    with server_listener = listener:
        exchange = try server_listener.accept(timeout=1s)
        with request = exchange:
            body = try request.body_text()
            headers = request.headers()
            try request.respond_text(200, request.method() + ":" + request.path() + ":" + body + ":" + headers["X-Test"], {{"Content-Type": "text/plain"}})
            return Result.Ok(None)

def serve_http_bytes(listener: net.HttpListener) -> Result[None, io.Error]:
    with server_listener = listener:
        exchange = try server_listener.accept(timeout=1s)
        with request = exchange:
            body = request.body_bytes()
            try request.respond_bytes(202, body, {{"Content-Type": "application/octet-stream"}})
            return Result.Ok(None)

def serve_ws(listener: net.WebSocketListener) -> Result[None, io.Error]:
    with server_listener = listener:
        socket = try server_listener.accept(timeout=1s)
        with server_socket = socket:
            match try server_socket.recv_text(timeout=1s):
                case Option.Some(text):
                    try server_socket.send_text("ws:" + text, timeout=1s)
                    return Result.Ok(None)
                case Option.None:
                    return Result.Ok(None)

def run() -> Result[None, io.Error]:
    bytes: Vec[uint8] = [65 as uint8, 66 as uint8]
    try fs.write_bytes("{path}", bytes)
    try fs.append_bytes("{path}", [67 as uint8, 10 as uint8])
    read_back = try fs.read_bytes("{path}")
    print(read_back.len())
    print(read_back[0])
    print(read_back[2])

    with TaskGroup() as group:
        udp_listener = try net.udp_bind("127.0.0.1:0")
        udp_addr = try udp_listener.local_addr()
        udp_task = group.start(serve_udp, udp_listener)
        udp_client = try net.udp_bind("127.0.0.1:0")
        with client_socket = udp_client:
            try client_socket.send_text(udp_addr, "ping", timeout=1s)
            match try client_socket.recv_from(1024, timeout=1s):
                case Option.Some(packet):
                    print(try packet.text())
                case Option.None:
                    return Result.Ok(None)
        match udp_task.result():
            case TaskResult.Ready(result):
                print(try result)
            case TaskResult.Error(_message):
                print("udp task failed")
                return Result.Ok(None)
            case TaskResult.Cancelled:
                print("udp task cancelled")
                return Result.Ok(None)
            case TaskResult.TimedOut:
                print("udp task timed out")
                return Result.Ok(None)

        http_listener = try net.http_listen("127.0.0.1:0")
        http_addr = try http_listener.local_addr()
        http_task = group.start(serve_http, http_listener)
        headers: Map[String, String] = {{"X-Test": "ok"}}
        response = try net.http_request_text("POST", "http://" + http_addr + "/hello", "body", headers.clone())
        with http_response = response:
            print(http_response.status())
            print(try http_response.text())
        match http_task.result():
            case TaskResult.Ready(result):
                try result
            case TaskResult.Error(_message):
                print("http task failed")
                return Result.Ok(None)
            case TaskResult.Cancelled:
                print("http task cancelled")
                return Result.Ok(None)
            case TaskResult.TimedOut:
                print("http task timed out")
                return Result.Ok(None)

        http_bytes_listener = try net.http_listen("127.0.0.1:0")
        http_bytes_addr = try http_bytes_listener.local_addr()
        http_bytes_task = group.start(serve_http_bytes, http_bytes_listener)
        bytes_response = try net.http_request_bytes("POST", "http://" + http_bytes_addr + "/bytes", [1 as uint8, 2 as uint8], headers)
        with received_bytes = bytes_response:
            print(received_bytes.status())
            print(received_bytes.bytes().len())
        match http_bytes_task.result():
            case TaskResult.Ready(result):
                try result
            case TaskResult.Error(_message):
                print("http bytes task failed")
                return Result.Ok(None)
            case TaskResult.Cancelled:
                print("http bytes task cancelled")
                return Result.Ok(None)
            case TaskResult.TimedOut:
                print("http bytes task timed out")
                return Result.Ok(None)

        ws_listener = try net.websocket_listen("127.0.0.1:0")
        ws_addr = try ws_listener.local_addr()
        ws_task = group.start(serve_ws, ws_listener)
        client = try net.websocket_connect_timeout("ws://" + ws_addr + "/", 1s)
        with ws_client = client:
            try ws_client.send_text("hi", timeout=1s)
            match try ws_client.recv_text(timeout=1s):
                case Option.Some(text):
                    print(text)
                case Option.None:
                    return Result.Ok(None)
        match ws_task.result():
            case TaskResult.Ready(result):
                try result
            case TaskResult.Error(_message):
                print("websocket task failed")
                return Result.Ok(None)
            case TaskResult.Cancelled:
                print("websocket task cancelled")
                return Result.Ok(None)
            case TaskResult.TimedOut:
                print("websocket task timed out")
                return Result.Ok(None)

    return Result.Ok(None)

def main() -> int32:
    match run():
        case Result.Ok(_):
            return 0
        case Result.Err(error):
            print(error)
            return 1
"#,
        path = file_path.display()
    );

    let output =
        run_path_with_source(&entry, &source).expect("advanced builtin io/net modules should run");
    assert_eq!(
        output.stdout,
        "4\n65\n67\nudp:ping\nping\n200\nPOST:/hello:body:ok\n202\n2\nws:hi\n"
    );
}

#[cfg(unix)]
#[test]
fn unix_and_tls_modules_run_through_public_api() {
    let temp = TempDir::new("aurora-io-unix-tls-run");
    let entry = temp.path().join("main.au");
    let unix_path = PathBuf::from(format!(
        "/tmp/aurora-io-{}-{}.sock",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time should be after unix epoch")
            .as_nanos()
    ));

    let certificate = generate_simple_self_signed(vec!["localhost".to_string()])
        .expect("should generate self-signed certificate");
    let cert_pem = certificate.cert.pem();
    let key_pem = certificate.key_pair.serialize_pem();

    let cert_path = temp.path().join("cert.pem");
    let key_path = temp.path().join("key.pem");
    fs::write(&cert_path, cert_pem).expect("should write cert pem");
    fs::write(&key_path, key_pem).expect("should write key pem");

    let source = format!(
        r#"import io
import net

def serve_unix(listener: net.UnixListener) -> Result[None, io.Error]:
    with server_listener = listener:
        stream = try server_listener.accept(timeout=1s)
        with server_stream = stream:
            match try server_stream.read_line(timeout=1s):
                case Option.Some(text):
                    try server_stream.write_all("unix:" + text, timeout=1s)
                    return Result.Ok(None)
                case Option.None:
                    return Result.Ok(None)

def serve_tls(listener: net.TlsListener) -> Result[None, io.Error]:
    with server_listener = listener:
        stream = try server_listener.accept(timeout=2s)
        with server_stream = stream:
            match try server_stream.read_line(timeout=2s):
                case Option.Some(text):
                    try server_stream.write_all("tls:" + text + "\n", timeout=2s)
                    return Result.Ok(None)
                case Option.None:
                    return Result.Ok(None)

def run() -> Result[None, io.Error]:
    with TaskGroup() as group:
        unix_listener = try net.unix_listen("{unix_path}")
        unix_task = group.start(serve_unix, unix_listener)
        client = try net.unix_connect_timeout("{unix_path}", 1s)
        with unix_client = client:
            try unix_client.write_all("ping\n", timeout=1s)
            match try unix_client.read_line(timeout=1s):
                case Option.Some(text):
                    print(text)
                case Option.None:
                    return Result.Ok(None)
        match unix_task.result():
            case TaskResult.Ready(result):
                try result
            case TaskResult.Error(_message):
                print("unix task failed")
                return Result.Ok(None)
            case TaskResult.Cancelled:
                print("unix task cancelled")
                return Result.Ok(None)
            case TaskResult.TimedOut:
                print("unix task timed out")
                return Result.Ok(None)

        tls_listener = try net.tls_listen("127.0.0.1:0", "{cert_path}", "{key_path}")
        tls_addr = try tls_listener.local_addr()
        tls_task = group.start(serve_tls, tls_listener)
        stream = try net.tls_connect_timeout(tls_addr, "localhost", "{cert_path}", 2s)
        with tls_client = stream:
            try tls_client.write_all("ping!\n", timeout=2s)
            match try tls_client.read_line(timeout=2s):
                case Option.Some(text):
                    print(text)
                case Option.None:
                    return Result.Ok(None)
        match tls_task.result():
            case TaskResult.Ready(result):
                try result
            case TaskResult.Error(_message):
                print("tls task failed")
                return Result.Ok(None)
            case TaskResult.Cancelled:
                print("tls task cancelled")
                return Result.Ok(None)
            case TaskResult.TimedOut:
                print("tls task timed out")
                return Result.Ok(None)

    return Result.Ok(None)

def main() -> int32:
    match run():
        case Result.Ok(_):
            return 0
        case Result.Err(error):
            print(error)
            return 1
"#,
        unix_path = unix_path.display(),
        cert_path = cert_path.display(),
        key_path = key_path.display()
    );

    let output =
        run_path_with_source(&entry, &source).expect("unix/tls builtin modules should run");
    let _ = fs::remove_file(&unix_path);
    assert_eq!(output.stdout, "unix:ping\ntls:ping!\n");
}
