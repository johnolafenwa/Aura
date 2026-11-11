use super::*;
use std::net::{TcpListener as StdListener, TcpStream as StdStream};
use std::thread;

/// One request a test server read.
#[derive(Clone, Debug)]
struct Seen {
    connection: usize,
    head: String,
    body: Vec<u8>,
}

impl Seen {
    fn line(&self) -> &str {
        self.head.lines().next().unwrap_or("")
    }

    fn header(&self, name: &str) -> Option<&str> {
        self.head.lines().skip(1).find_map(|line| {
            let (header, value) = line.split_once(':')?;
            header.eq_ignore_ascii_case(name).then(|| value.trim())
        })
    }
}

enum Reply {
    /// Write the bytes and wait for the next request.
    Send(Vec<u8>),
    /// Write the bytes and close the connection.
    SendAndClose(Vec<u8>),
    /// Write each piece on its own, a few milliseconds apart, then close.
    Pieces(Vec<Vec<u8>>),
    /// Write the bytes, then keep the connection open and silent this long.
    SendAndStall(Vec<u8>, StdDuration),
}

fn read_request(stream: &mut StdStream, connection: usize) -> Option<Seen> {
    let mut bytes = Vec::new();
    let mut byte = [0u8; 1];
    while !bytes.ends_with(b"\r\n\r\n") {
        match stream.read(&mut byte) {
            Ok(1) => bytes.push(byte[0]),
            _ => return None,
        }
    }
    let head = String::from_utf8(bytes).expect("a request head is text");
    let length = head
        .lines()
        .find_map(|line| {
            let (name, value) = line.split_once(':')?;
            name.eq_ignore_ascii_case("content-length")
                .then(|| value.trim().parse::<usize>().expect("a length"))
        })
        .unwrap_or(0);
    let mut body = vec![0u8; length];
    stream.read_exact(&mut body).expect("a request body");
    Some(Seen {
        connection,
        head,
        body,
    })
}

/// A server that accepts `connections` connections one after another and
/// answers each request with `reply`. Joining it returns what it read.
fn serve(
    connections: usize,
    mut reply: impl FnMut(&Seen) -> Reply + Send + 'static,
) -> (String, u16, thread::JoinHandle<Vec<Seen>>) {
    let listener = StdListener::bind("127.0.0.1:0").expect("a test server binds");
    let address = listener.local_addr().expect("a local address");
    let handle = thread::spawn(move || {
        let mut seen = Vec::new();
        for connection in 0..connections {
            let (mut stream, _) = listener.accept().expect("a connection");
            while let Some(request) = read_request(&mut stream, connection) {
                let answer = reply(&request);
                seen.push(request);
                match answer {
                    Reply::Send(bytes) => stream.write_all(&bytes).expect("a reply"),
                    Reply::SendAndClose(bytes) => {
                        stream.write_all(&bytes).expect("a reply");
                        break;
                    }
                    Reply::Pieces(pieces) => {
                        for piece in pieces {
                            stream.write_all(&piece).expect("a piece");
                            stream.flush().expect("a flush");
                            thread::sleep(StdDuration::from_millis(3));
                        }
                        break;
                    }
                    Reply::SendAndStall(bytes, pause) => {
                        stream.write_all(&bytes).expect("a reply");
                        thread::sleep(pause);
                        break;
                    }
                }
            }
        }
        seen
    });
    (format!("http://{address}"), address.port(), handle)
}

fn ok(body: &str) -> Vec<u8> {
    format!(
        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n{body}",
        body.len()
    )
    .into_bytes()
}

fn options() -> HttpRequestOptions {
    HttpRequestOptions {
        timeout: Some(StdDuration::from_secs(5)),
        read_timeout: Some(StdDuration::from_secs(5)),
        ..Default::default()
    }
}

fn get(url: &str) -> io::Result<HttpBody> {
    http_send("GET", url, Vec::new(), b"", &options(), None)
}

fn text(response: &mut HttpBody) -> String {
    String::from_utf8(response.read_all(1 << 20, None).expect("a body")).expect("a text body")
}

#[test]
fn a_body_is_read_in_pieces_and_its_connection_is_kept_for_the_next_request() {
    let (base, port, server) = serve(1, |request| {
        Reply::Send(ok(&format!("body of {}", request.line())))
    });
    let mut first = get(&format!("{base}/a")).expect("the first response");
    assert_eq!(first.status(), 200);
    assert_eq!(first.reason(), "OK");
    assert_eq!(first.url(), format!("{base}/a"));
    assert_eq!(
        first.headers(),
        [("content-length".to_string(), "23".to_string())]
    );
    assert_eq!(first.read_chunk(4, None).unwrap().unwrap(), b"body");
    assert_eq!(first.read_chunk(100, None).unwrap().unwrap(), b" of GET /a HTTP/1.1");
    assert_eq!(first.read_chunk(100, None).unwrap(), None);
    assert_eq!(first.read_chunk(100, None).unwrap(), None);

    // The finished connection carries the second request.
    let mut second = get(&format!("{base}/b")).expect("the second response");
    assert_eq!(text(&mut second), "body of GET /b HTTP/1.1");
    assert_eq!(pool_discard(port), 1);
    let seen = server.join().expect("the server");
    assert_eq!(seen.len(), 2);
    assert!(seen.iter().all(|request| request.connection == 0));
    assert_eq!(seen[0].header("connection"), Some("keep-alive"));
    assert_eq!(seen[0].header("accept-encoding"), Some("gzip, deflate"));
    assert_eq!(seen[0].header("accept"), Some("*/*"));
    assert!(seen[0].header("user-agent").unwrap().starts_with("aura/"));
    assert!(seen[0].header("host").unwrap().starts_with("127.0.0.1:"));
}

#[test]
fn read_chunk_rejects_sizes_outside_its_range() {
    let (base, port, server) = serve(1, |_| Reply::Send(ok("x")));
    let mut response = get(&base).expect("a response");
    for size in [0, -1, MAX_HTTP_BODY_READ_BYTES as i64 + 1] {
        let error = response.read_chunk(size, None).expect_err("a bad size");
        assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
    }
    assert_eq!(response.read_chunk(8, None).unwrap().unwrap(), b"x");
    pool_discard(port);
    server.join().expect("the server");
}

#[test]
fn an_unfinished_or_closed_body_does_not_return_its_connection() {
    // A body larger than one read is still on the connection.
    let (base, port, server) = serve(2, |_| Reply::Send(ok(&"0123456789".repeat(40_000))));
    let mut partial = get(&base).expect("a response");
    assert_eq!(partial.read_chunk(3, None).unwrap().unwrap(), b"012");
    partial.close();
    assert_eq!(pool_discard(port), 0);
    let error = partial.read_chunk(3, None).expect_err("a closed body");
    assert_eq!(error.to_string(), "resource is closed");
    assert!(partial.read_line(None).is_err());
    assert!(partial.read_lines(1, None).is_err());
    assert!(partial.read_event(None).is_err());
    assert!(partial.read_all(10, None).is_err());

    // A dropped response closes its connection as well.
    let unread = get(&base).expect("a second response");
    drop(unread);
    assert_eq!(pool_discard(port), 0);
    let seen = server.join().expect("the server");
    assert_eq!(seen[1].connection, 1);
}

#[test]
fn keep_alive_off_asks_the_server_to_close_and_keeps_nothing() {
    let (base, port, server) = serve(2, |_| Reply::SendAndClose(ok("x")));
    let off = HttpRequestOptions {
        keep_alive: false,
        ..options()
    };
    for _ in 0..2 {
        let mut response = http_send("GET", &base, Vec::new(), b"", &off, None).expect("a reply");
        assert_eq!(text(&mut response), "x");
        assert_eq!(pool_discard(port), 0);
    }
    let seen = server.join().expect("the server");
    assert_eq!(seen[0].header("connection"), Some("close"));
    assert_eq!(seen[1].connection, 1);
}

#[test]
fn a_response_that_says_close_or_speaks_http_1_0_is_not_reused() {
    let (base, port, server) = serve(3, |request| match request.connection {
        0 => Reply::SendAndClose(
            b"HTTP/1.1 200 OK\r\nConnection: close\r\nContent-Length: 1\r\n\r\na".to_vec(),
        ),
        1 => Reply::SendAndClose(b"HTTP/1.0 200 OK\r\nContent-Length: 1\r\n\r\nb".to_vec()),
        _ => Reply::SendAndClose(
            b"HTTP/1.0 200 OK\r\nConnection: keep-alive\r\nContent-Length: 1\r\n\r\nc".to_vec(),
        ),
    });
    for (expected, kept) in [("a", 0), ("b", 0), ("c", 1)] {
        let mut response = get(&base).expect("a response");
        assert_eq!(text(&mut response), expected);
        assert_eq!(pool_discard(port), kept, "after body {expected}");
    }
    server.join().expect("the server");
}

#[test]
fn a_kept_connection_the_server_closed_is_replaced() {
    // The server closes after each response without saying so.
    let (base, port, server) = serve(2, |_| Reply::SendAndClose(ok("x")));
    let mut first = get(&base).expect("the first response");
    assert_eq!(text(&mut first), "x");
    // Give the close time to arrive.
    thread::sleep(StdDuration::from_millis(50));
    let mut second = get(&base).expect("the second response uses a new connection");
    assert_eq!(text(&mut second), "x");
    pool_discard(port);
    let seen = server.join().expect("the server");
    assert_eq!(seen[1].connection, 1);
}

#[test]
fn a_request_that_fails_on_a_kept_connection_is_sent_again_only_when_safe() {
    // The first connection answers once, reads the next request, and
    // closes without answering it.
    let script = |request: &Seen| {
        if request.connection == 0 && request.line().contains("/second") {
            Reply::SendAndClose(Vec::new())
        } else {
            Reply::Send(ok(request.line()))
        }
    };
    let (base, port, server) = serve(2, script);
    let mut first = get(&format!("{base}/first")).expect("the first response");
    text(&mut first);
    let mut second = get(&format!("{base}/second")).expect("a GET is sent again");
    assert_eq!(text(&mut second), "GET /second HTTP/1.1");
    pool_discard(port);
    let seen = server.join().expect("the server");
    assert_eq!(
        seen.iter().map(|request| request.connection).collect::<Vec<_>>(),
        [0, 0, 1]
    );

    let (base, port, server) = serve(1, script);
    let mut first = get(&format!("{base}/first")).expect("the first response");
    text(&mut first);
    let error = http_send(
        "POST",
        &format!("{base}/second"),
        Vec::new(),
        b"once",
        &options(),
        None,
    )
    .err()
    .expect("a POST the server may have acted on is not sent again");
    assert_eq!(error.kind(), io::ErrorKind::UnexpectedEof);
    pool_discard(port);
    assert_eq!(server.join().expect("the server").len(), 2);
}

#[test]
fn chunked_bodies_decode_across_writes_with_extensions_and_trailers() {
    let (base, port, server) = serve(2, |request| {
        if request.connection == 0 {
            Reply::Send(
                b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n4\r\nWiki\r\n5;kind=text\r\npedia\r\n0\r\nX-Finished: yes\r\n\r\n"
                    .to_vec(),
            )
        } else {
            Reply::Pieces(vec![
                b"HTTP/1.1 200 OK\r\nTransfer-Enc".to_vec(),
                b"oding: chunked\r\n\r\n".to_vec(),
                b"A".to_vec(),
                b"\r\n01234".to_vec(),
                b"56789\r".to_vec(),
                b"\n0\r".to_vec(),
                b"\n\r\n".to_vec(),
            ])
        }
    });
    let mut whole = get(&base).expect("a chunked response");
    assert_eq!(text(&mut whole), "Wikipedia");
    // A chunked body that ended cleanly leaves a reusable connection.
    assert_eq!(pool_discard(port), 1);
    let mut split = get(&base).expect("a split chunked response");
    assert_eq!(text(&mut split), "0123456789");
    pool_discard(port);
    server.join().expect("the server");
}

#[test]
fn malformed_and_truncated_chunked_bodies_are_errors() {
    let cases: [(&[u8], io::ErrorKind, &str); 6] = [
        (b"nope\r\n", io::ErrorKind::InvalidData, "not a hex number"),
        (b"\r\n", io::ErrorKind::InvalidData, "not a hex number"),
        (b"+4\r\nabcd\r\n0\r\n\r\n", io::ErrorKind::InvalidData, "not a hex number"),
        (b"4\r\nabcdXX0\r\n\r\n", io::ErrorKind::InvalidData, "does not end with CRLF"),
        (b"4\r\nab", io::ErrorKind::UnexpectedEof, "before the HTTP response body was complete"),
        (b"4\r\nabcd\r\n0\r\nX: y\r\n", io::ErrorKind::UnexpectedEof, "before the HTTP response body was complete"),
    ];
    for (body, kind, message) in cases {
        let mut reply = b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n".to_vec();
        reply.extend_from_slice(body);
        let (base, port, server) = serve(1, move |_| Reply::SendAndClose(reply.clone()));
        let mut response = get(&base).expect("the head arrives");
        let error = response.read_all(1 << 20, None).expect_err("a bad body");
        assert_eq!(error.kind(), kind, "{:?}", String::from_utf8_lossy(body));
        assert!(error.to_string().contains(message), "{error}");
        // A failed body hands out what it decoded before the error and
        // then ends. It keeps no connection.
        while response.read_chunk(8, None).unwrap().is_some() {}
        assert_eq!(pool_discard(port), 0);
        server.join().expect("the server");
    }

    let mut long_line = b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n".to_vec();
    long_line.extend(std::iter::repeat_n(b'1', MAX_CHUNK_LINE_BYTES + READ_BLOCK));
    let (base, _, server) = serve(1, move |_| Reply::SendAndClose(long_line.clone()));
    let mut response = get(&base).expect("the head arrives");
    let error = response.read_all(1 << 20, None).expect_err("an endless size line");
    assert!(error.to_string().contains("a framing line is too long"), "{error}");
    drop(response);
    server.join().expect("the server");
}

#[test]
fn a_body_without_a_length_runs_until_the_server_closes() {
    let (base, port, server) = serve(1, |_| {
        Reply::Pieces(vec![
            b"HTTP/1.1 202 Accepted\r\n\r\nfirst ".to_vec(),
            b"second".to_vec(),
        ])
    });
    let mut response = get(&base).expect("a response");
    assert_eq!(response.status(), 202);
    assert_eq!(text(&mut response), "first second");
    assert_eq!(pool_discard(port), 0);
    server.join().expect("the server");
}

#[test]
fn a_short_body_is_an_unexpected_end() {
    let (base, _, server) = serve(1, |_| {
        Reply::SendAndClose(b"HTTP/1.1 200 OK\r\nContent-Length: 4\r\n\r\nok".to_vec())
    });
    let mut response = get(&base).expect("the head arrives");
    assert_eq!(response.read_chunk(8, None).unwrap().unwrap(), b"ok");
    let error = response.read_chunk(8, None).expect_err("a short body");
    assert_eq!(error.kind(), io::ErrorKind::UnexpectedEof);
    server.join().expect("the server");
}

#[test]
fn responses_without_a_body_end_at_once() {
    let (base, port, server) = serve(1, |request| {
        if request.line().starts_with("HEAD") {
            Reply::Send(b"HTTP/1.1 200 OK\r\nContent-Length: 500\r\n\r\n".to_vec())
        } else if request.line().contains("/empty") {
            Reply::Send(b"HTTP/1.1 204 No Content\r\n\r\n".to_vec())
        } else {
            Reply::Send(b"HTTP/1.1 304 \r\nContent-Length: 9\r\n\r\n".to_vec())
        }
    });
    let mut head = http_send("HEAD", &base, Vec::new(), b"", &options(), None).expect("HEAD");
    assert_eq!(head.headers()[0].1, "500");
    assert_eq!(head.read_chunk(8, None).unwrap(), None);
    let mut empty = get(&format!("{base}/empty")).expect("204");
    assert_eq!(empty.reason(), "No Content");
    assert_eq!(empty.read_line(None).unwrap(), None);
    let mut unchanged = get(&format!("{base}/cached")).expect("304");
    // An empty reason phrase gets the standard one.
    assert_eq!(unchanged.reason(), "Not Modified");
    assert!(unchanged.read_all(8, None).unwrap().is_empty());
    assert_eq!(pool_discard(port), 1);
    let seen = server.join().expect("the server");
    assert!(seen.iter().all(|request| request.connection == 0));
}

#[test]
fn interim_responses_are_skipped_and_a_protocol_switch_is_an_error() {
    let (base, port, server) = serve(2, |request| {
        if request.connection == 0 {
            let mut reply = b"HTTP/1.1 100 Continue\r\n\r\nHTTP/1.1 103 Early Hints\r\nLink: </a>\r\n\r\n".to_vec();
            reply.extend(ok("final"));
            Reply::Send(reply)
        } else {
            Reply::SendAndClose(b"HTTP/1.1 101 Switching Protocols\r\n\r\n".to_vec())
        }
    });
    let mut response = get(&base).expect("the final response");
    assert_eq!(response.status(), 200);
    assert_eq!(text(&mut response), "final");
    pool_discard(port);
    let error = get(&base).err().expect("a protocol switch");
    assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    assert!(error.to_string().contains("switched protocols"));
    server.join().expect("the server");
}

fn gzip(data: &[u8]) -> Vec<u8> {
    crate::gzip_codec::gzip_compress(data, 6).expect("gzip")
}

fn encoded(encoding: &str, body: &[u8]) -> Vec<u8> {
    let mut reply = format!(
        "HTTP/1.1 200 OK\r\nContent-Encoding: {encoding}\r\nContent-Length: {}\r\nX-Kept: yes\r\n\r\n",
        body.len()
    )
    .into_bytes();
    reply.extend_from_slice(body);
    reply
}

#[test]
fn gzip_and_deflate_bodies_are_decoded_as_they_arrive() {
    let plain = "a line of text\n".repeat(4_000);
    let zipped = gzip(plain.as_bytes());
    let mut deflater =
        flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    deflater.write_all(plain.as_bytes()).unwrap();
    let deflated = deflater.finish().unwrap();
    // Two gzip members in one body decode as their concatenation.
    let mut two_members = gzip(b"first,");
    two_members.extend(gzip(b"second"));

    let replies = [
        encoded("gzip", &zipped),
        encoded("x-gzip", &zipped),
        encoded("deflate", &deflated),
        encoded("GZIP", &two_members),
    ];
    let (base, port, server) = serve(1, move |request| {
        let index: usize = request.line().split(['/', ' ']).nth(2).unwrap().parse().unwrap();
        Reply::Send(replies[index].clone())
    });
    for index in 0..3 {
        let mut response = get(&format!("{base}/{index}")).expect("an encoded response");
        // The headers that describe the encoded bytes are gone.
        assert_eq!(
            response.headers(),
            [("x-kept".to_string(), "yes".to_string())]
        );
        let mut lines = 0;
        while let Some(line) = response.read_line(None).expect("a decoded line") {
            assert_eq!(line, "a line of text");
            lines += 1;
        }
        assert_eq!(lines, 4_000);
    }
    let mut response = get(&format!("{base}/3")).expect("a two-member response");
    assert_eq!(text(&mut response), "first,second");
    assert_eq!(pool_discard(port), 1);
    server.join().expect("the server");
}

#[test]
fn a_caller_who_names_the_encoding_or_turns_decoding_off_gets_the_bytes_sent() {
    let zipped = gzip(b"payload");
    let reply = encoded("gzip", &zipped);
    let (base, port, server) = serve(1, move |_| Reply::Send(reply.clone()));
    let mut asked = http_send(
        "GET",
        &base,
        vec![("Accept-Encoding".to_string(), "gzip".to_string())],
        b"",
        &options(),
        None,
    )
    .expect("a response");
    assert_eq!(asked.headers()[0], ("content-encoding".to_string(), "gzip".to_string()));
    assert_eq!(asked.read_all(1 << 20, None).unwrap(), zipped);
    let off = HttpRequestOptions {
        decompress: false,
        ..options()
    };
    let mut raw = http_send("GET", &base, Vec::new(), b"", &off, None).expect("a response");
    assert_eq!(raw.read_all(1 << 20, None).unwrap(), zipped);
    pool_discard(port);
    let seen = server.join().expect("the server");
    assert_eq!(seen[0].header("accept-encoding"), Some("gzip"));
    assert_eq!(seen[1].header("accept-encoding"), None);
}

#[test]
fn corrupt_and_truncated_compressed_bodies_are_invalid_data() {
    let mut corrupt = gzip(&b"some text that compresses ".repeat(50));
    let middle = corrupt.len() / 2;
    corrupt[middle] ^= 0xff;
    corrupt[middle + 1] ^= 0xff;
    let mut truncated = gzip(&b"some text that compresses ".repeat(50));
    truncated.truncate(truncated.len() - 6);
    for body in [corrupt, truncated, b"not gzip at all".to_vec()] {
        let reply = encoded("gzip", &body);
        let (base, port, server) = serve(1, move |_| Reply::Send(reply.clone()));
        let mut response = get(&base).expect("the head arrives");
        let error = response.read_all(1 << 20, None).expect_err("a bad gzip body");
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
        assert!(error.to_string().contains("invalid compressed HTTP response body"));
        pool_discard(port);
        drop(response);
        server.join().expect("the server");
    }
}

#[test]
fn an_empty_compressed_body_is_empty() {
    let (base, port, server) = serve(1, |_| Reply::Send(encoded("gzip", b"")));
    let mut response = get(&base).expect("a response");
    assert!(response.read_all(8, None).unwrap().is_empty());
    pool_discard(port);
    server.join().expect("the server");
}

fn redirect(status: u16, location: &str) -> Vec<u8> {
    format!("HTTP/1.1 {status} Moved\r\nLocation: {location}\r\nContent-Length: 5\r\n\r\nmoved")
        .into_bytes()
}

#[test]
fn redirects_are_followed_with_the_method_each_status_calls_for() {
    let (base, port, server) = serve(1, |request| {
        let path = request.line().split(' ').nth(1).unwrap().to_string();
        if request.line().starts_with("HEAD") {
            let head = if path == "/303" {
                "HTTP/1.1 303 See Other\r\nLocation: /end\r\nContent-Length: 5\r\n\r\n"
            } else {
                "HTTP/1.1 200 OK\r\nContent-Length: 5\r\n\r\n"
            };
            return Reply::Send(head.as_bytes().to_vec());
        }
        match path.as_str() {
            "/301" => Reply::Send(redirect(301, "/end?from=301")),
            "/302" => Reply::Send(redirect(302, "end")),
            "/303" => Reply::Send(redirect(303, "/end")),
            "/307" => Reply::Send(redirect(307, "/end")),
            "/308" => Reply::Send(redirect(308, "/end")),
            _ => Reply::Send(ok(&format!(
                "{} {}",
                request.line(),
                String::from_utf8_lossy(&request.body)
            ))),
        }
    });
    let send = |method: &str, path: &str| {
        let headers = vec![
            ("Content-Type".to_string(), "text/plain".to_string()),
            ("Authorization".to_string(), "token".to_string()),
        ];
        let mut response = http_send(
            method,
            &format!("{base}{path}"),
            headers,
            b"data",
            &options(),
            None,
        )
        .expect("a redirected response");
        (response.url().to_string(), text(&mut response))
    };
    assert_eq!(
        send("POST", "/301"),
        (format!("{base}/end?from=301"), "GET /end?from=301 HTTP/1.1 ".to_string())
    );
    assert_eq!(send("POST", "/302").1, "GET /end HTTP/1.1 ");
    assert_eq!(send("PUT", "/302").1, "PUT /end HTTP/1.1 data");
    assert_eq!(send("PUT", "/303").1, "GET /end HTTP/1.1 ");
    assert_eq!(send("HEAD", "/303").1, "");
    assert_eq!(send("POST", "/307").1, "POST /end HTTP/1.1 data");
    assert_eq!(send("POST", "/308").1, "POST /end HTTP/1.1 data");
    assert_eq!(send("GET", "/301").1, "GET /end?from=301 HTTP/1.1 data");
    pool_discard(port);
    let seen = server.join().expect("the server");
    // Every hop used the one connection: redirect bodies are drained.
    assert!(seen.iter().all(|request| request.connection == 0));
    let after_post_301 = &seen[1];
    assert_eq!(after_post_301.header("content-type"), None);
    assert_eq!(after_post_301.header("content-length"), Some("0"));
    // The same server keeps its credentials.
    assert_eq!(after_post_301.header("authorization"), Some("token"));
}

#[test]
fn a_redirect_to_another_server_does_not_carry_credentials() {
    let (other_base, other_port, other) = serve(1, |request| Reply::Send(ok(request.line())));
    let target = format!("{other_base}/landing");
    let (base, port, server) = serve(1, move |_| Reply::Send(redirect(302, &target)));
    let headers = vec![
        ("Authorization".to_string(), "token".to_string()),
        ("Cookie".to_string(), "session".to_string()),
        ("Proxy-Authorization".to_string(), "proxy".to_string()),
        ("Host".to_string(), "first.example".to_string()),
        ("X-Plain".to_string(), "kept".to_string()),
    ];
    let mut response =
        http_send("GET", &base, headers, b"", &options(), None).expect("a response");
    assert_eq!(response.url(), format!("{other_base}/landing"));
    assert_eq!(text(&mut response), "GET /landing HTTP/1.1");
    pool_discard(port);
    pool_discard(other_port);
    let first = server.join().expect("the first server");
    assert_eq!(first[0].header("authorization"), Some("token"));
    assert_eq!(first[0].header("host"), Some("first.example"));
    let second = other.join().expect("the second server");
    assert_eq!(second[0].header("authorization"), None);
    assert_eq!(second[0].header("cookie"), None);
    assert_eq!(second[0].header("proxy-authorization"), None);
    assert_eq!(second[0].header("x-plain"), Some("kept"));
    assert!(second[0].header("host").unwrap().starts_with("127.0.0.1:"));
}

#[test]
fn redirect_limits_and_bad_locations_are_reported() {
    let (base, port, server) = serve(1, |request| {
        let path = request.line().split(' ').nth(1).unwrap().to_string();
        match path.as_str() {
            "/loop" => Reply::Send(redirect(302, "/loop")),
            "/ftp" => Reply::Send(redirect(302, "ftp://example.test/file")),
            "/bad" => Reply::Send(redirect(302, "http://[broken")),
            "/nowhere" => Reply::Send(
                b"HTTP/1.1 302 Found\r\nContent-Length: 0\r\n\r\n".to_vec(),
            ),
            _ => Reply::Send(ok("end")),
        }
    });
    let three = HttpRequestOptions {
        max_redirects: 3,
        ..options()
    };
    let error = http_send("GET", &format!("{base}/loop"), Vec::new(), b"", &three, None)
        .err()
        .expect("a redirect loop");
    assert_eq!(error.to_string(), "stopped after 3 redirects");

    // Zero hands every redirect response to the caller.
    let none = HttpRequestOptions {
        max_redirects: 0,
        ..options()
    };
    let mut manual = http_send("GET", &format!("{base}/loop"), Vec::new(), b"", &none, None)
        .expect("the redirect response itself");
    assert_eq!(manual.status(), 302);
    assert_eq!(manual.headers()[0], ("location".to_string(), "/loop".to_string()));
    assert_eq!(text(&mut manual), "moved");

    for path in ["/ftp", "/bad"] {
        let error = get(&format!("{base}{path}")).err().expect("a bad location");
        assert_eq!(error.kind(), io::ErrorKind::InvalidData, "{path}");
    }
    // A redirect status without a location is an ordinary response.
    let mut nowhere = get(&format!("{base}/nowhere")).expect("a response");
    assert_eq!(nowhere.status(), 302);
    assert!(text(&mut nowhere).is_empty());
    pool_discard(port);
    server.join().expect("the server");
}

#[test]
fn a_large_redirect_body_closes_its_connection_instead_of_being_read() {
    let big = MAX_REDIRECT_DRAIN_BYTES * 2;
    let (base, port, server) = serve(2, move |request| {
        if request.line().contains("/start") {
            let mut reply = format!(
                "HTTP/1.1 302 Found\r\nLocation: /end\r\nContent-Length: {big}\r\n\r\n"
            )
            .into_bytes();
            reply.extend(std::iter::repeat_n(b'x', big));
            Reply::Send(reply)
        } else {
            Reply::Send(ok("end"))
        }
    });
    let mut response = get(&format!("{base}/start")).expect("the redirect target");
    assert_eq!(text(&mut response), "end");
    pool_discard(port);
    let seen = server.join().expect("the server");
    assert_eq!(seen[1].connection, 1);
}

#[test]
fn lines_are_read_one_at_a_time_or_in_batches() {
    let (base, port, server) = serve(2, |request| {
        if request.connection == 0 {
            Reply::Send(ok("one\r\ntwo\n\nlast"))
        } else {
            Reply::Pieces(vec![
                b"HTTP/1.1 200 OK\r\n\r\na\nb\nc".to_vec(),
                b"\nd\n".to_vec(),
            ])
        }
    });
    let mut response = get(&base).expect("a response");
    let mut lines = Vec::new();
    while let Some(line) = response.read_line(None).expect("a line") {
        lines.push(line);
    }
    assert_eq!(lines, ["one", "two", "", "last"]);
    assert_eq!(response.read_line(None).unwrap(), None);
    assert_eq!(pool_discard(port), 1);

    let mut batched = get(&base).expect("a second response");
    for count in [0, -1, MAX_LINES_PER_CALL + 1] {
        let error = batched.read_lines(count, None).expect_err("a bad count");
        assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
    }
    // A batch holds the lines that have arrived, not a line still coming.
    assert_eq!(batched.read_lines(1, None).unwrap(), ["a"]);
    let mut rest = Vec::new();
    loop {
        let batch = batched.read_lines(100, None).expect("a batch");
        if batch.is_empty() {
            break;
        }
        rest.extend(batch);
    }
    assert_eq!(rest, ["b", "c", "d"]);
    server.join().expect("the server");
}

#[test]
fn a_line_that_is_not_utf8_is_invalid_data_and_reading_continues() {
    let (base, port, server) = serve(1, |_| {
        let mut reply = b"HTTP/1.1 200 OK\r\nContent-Length: 8\r\n\r\n".to_vec();
        reply.extend_from_slice(b"\xff\xfe\nok\nz\n");
        Reply::Send(reply)
    });
    let mut response = get(&base).expect("a response");
    let error = response.read_line(None).expect_err("a bad line");
    assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    assert_eq!(response.read_line(None).unwrap().as_deref(), Some("ok"));
    assert_eq!(response.read_lines(5, None).unwrap(), ["z"]);
    pool_discard(port);
    server.join().expect("the server");
}

fn event_stream(body: &str) -> Vec<Vec<u8>> {
    let mut pieces = vec![b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\n\r\n".to_vec()];
    // Split the stream at awkward places.
    let bytes = body.as_bytes();
    let mut start = 0;
    for size in [1usize, 7, 2, 11, 3].iter().cycle() {
        if start >= bytes.len() {
            break;
        }
        let end = (start + size).min(bytes.len());
        pieces.push(bytes[start..end].to_vec());
        start = end;
    }
    pieces
}

fn events(body: &'static str) -> Vec<HttpServerEvent> {
    let (base, _, server) = serve(1, move |_| Reply::Pieces(event_stream(body)));
    let mut response = get(&base).expect("an event stream");
    let mut found = Vec::new();
    while let Some(event) = response.read_event(None).expect("an event") {
        found.push(event);
    }
    assert_eq!(response.read_event(None).unwrap(), None);
    server.join().expect("the server");
    found
}

fn event(event: &str, data: &str, id: &str, retry_ms: i64) -> HttpServerEvent {
    HttpServerEvent {
        event: event.to_string(),
        data: data.to_string(),
        id: id.to_string(),
        retry_ms,
    }
}

#[test]
fn server_sent_events_follow_the_event_stream_format() {
    assert_eq!(
        events(
            "\u{feff}: a comment\n\nevent: start\ndata: {\"n\": 0}\n\n\
             id: 7\ndata: first\ndata: second\n\n\
             data:no space\ndata:  two spaces\nretry: 1500\n\n\
             event: named\nunknown: ignored\ndata\n\n\
             id\ndata: after an empty id\n\n"
        ),
        [
            event("start", "{\"n\": 0}", "", -1),
            event("message", "first\nsecond", "7", -1),
            event("message", "no space\n two spaces", "7", 1500),
            event("named", "", "7", -1),
            event("message", "after an empty id", "", -1),
        ]
    );
}

#[test]
fn events_without_data_and_an_unfinished_last_event_are_dropped() {
    assert_eq!(
        events(
            "event: ping\n\nretry: 10\n\n\r\ndata: kept\r\n\r\nid: bad\0id\nretry: soon\ndata: x\n\ndata: cut off\n"
        ),
        [event("message", "kept", "", -1), event("message", "x", "", -1)]
    );
}

#[test]
fn a_whole_body_read_enforces_its_limit() {
    let (base, port, server) = serve(4, |request| match request.connection {
        // A declared length over the limit fails before the body is read.
        0 => Reply::SendAndStall(
            b"HTTP/1.1 200 OK\r\nContent-Length: 65\r\n\r\n".to_vec(),
            StdDuration::from_millis(200),
        ),
        1 => {
            let mut reply = b"HTTP/1.1 200 OK\r\n\r\n".to_vec();
            reply.extend(std::iter::repeat_n(b'x', 65));
            Reply::SendAndClose(reply)
        }
        2 => Reply::SendAndClose(encoded("gzip", &gzip(&[b'x'; 65]))),
        _ => Reply::Send(ok(&"x".repeat(64))),
    });
    for _ in 0..3 {
        let mut response = get(&base).expect("a response");
        let error = response.read_all(64, None).expect_err("a body over the limit");
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
        assert_eq!(error.to_string(), "the HTTP response body is larger than 64 bytes");
        pool_discard(port);
    }
    let mut fits = get(&base).expect("a response");
    assert_eq!(fits.read_all(64, None).unwrap().len(), 64);
    for limit in [-1, MAX_HTTP_BODY_READ_BYTES as i64 + 1] {
        let error = fits.read_all(limit, None).expect_err("a bad limit");
        assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
    }
    pool_discard(port);
    server.join().expect("the server");
}

#[test]
fn a_whole_body_read_after_a_partial_read_returns_the_rest() {
    let (base, port, server) = serve(1, |_| Reply::Send(ok("header\nrest of the body")));
    let mut response = get(&base).expect("a response");
    assert_eq!(response.read_line(None).unwrap().as_deref(), Some("header"));
    assert_eq!(text(&mut response), "rest of the body");
    assert!(response.read_all(8, None).unwrap().is_empty());
    pool_discard(port);
    server.join().expect("the server");
}

#[test]
fn invalid_requests_fail_before_anything_is_sent() {
    let cases = [
        ("GET", "not-a-url", io::ErrorKind::InvalidInput),
        ("GET", "ftp://example.test/", io::ErrorKind::InvalidInput),
        ("", "http://127.0.0.1:1/", io::ErrorKind::InvalidInput),
        ("GET /x HTTP/1.1\r\nX: y", "http://127.0.0.1:1/", io::ErrorKind::InvalidInput),
    ];
    for (method, url, kind) in cases {
        let error = http_send(method, url, Vec::new(), b"", &options(), None)
            .err()
            .expect("an invalid request");
        assert_eq!(error.kind(), kind, "{method} {url}");
    }
    for header in [("Bad Name", "x"), ("X-Injected", "a\r\nb: c")] {
        let headers = vec![(header.0.to_string(), header.1.to_string())];
        let error = http_send("GET", "http://127.0.0.1:1/", headers, b"", &options(), None)
            .err()
            .expect("an invalid header");
        assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
    }
}

#[test]
fn a_refused_connection_is_reported() {
    let listener = StdListener::bind("127.0.0.1:0").expect("a port");
    let address = listener.local_addr().expect("an address");
    drop(listener);
    let error = get(&format!("http://{address}/")).err().expect("no server");
    assert_eq!(error.kind(), io::ErrorKind::ConnectionRefused);
}

#[test]
fn the_request_timeout_covers_the_head_and_the_read_timeout_each_body_read() {
    let (base, _, server) = serve(1, |_| {
        Reply::SendAndStall(Vec::new(), StdDuration::from_millis(400))
    });
    let (stalled_base, _, stalled_server) = serve(1, |_| {
        Reply::SendAndStall(
            b"HTTP/1.1 200 OK\r\nContent-Length: 10\r\n\r\nhalf".to_vec(),
            StdDuration::from_millis(400),
        )
    });
    let quick = HttpRequestOptions {
        timeout: Some(StdDuration::from_millis(60)),
        read_timeout: Some(StdDuration::from_millis(60)),
        ..Default::default()
    };
    let error = http_send("GET", &base, Vec::new(), b"", &quick, None)
        .err()
        .expect("a silent server");
    assert_eq!(error.kind(), io::ErrorKind::TimedOut);

    let mut response =
        http_send("GET", &stalled_base, Vec::new(), b"", &quick, None).expect("a head");
    assert_eq!(response.read_chunk(8, None).unwrap().unwrap(), b"half");
    let error = response.read_chunk(8, None).expect_err("a stalled body");
    assert_eq!(error.kind(), io::ErrorKind::TimedOut);
    server.join().expect("the server");
    stalled_server.join().expect("the second server");
}

#[test]
fn a_cancelled_task_stops_a_request() {
    let (base, _, server) = serve(1, |_| {
        Reply::SendAndStall(Vec::new(), StdDuration::from_millis(300))
    });
    let group = TaskGroupValue::new(&CancellationContext::default());
    let cancellation = group.child_cancellation();
    group.cancel();
    let error = http_send("GET", &base, Vec::new(), b"", &options(), Some(&cancellation))
        .err()
        .expect("a cancelled request");
    assert_eq!(error.to_string(), "operation cancelled");
    // The server never sees a request; release it.
    drop(StdStream::connect(base.trim_start_matches("http://")).expect("a connection"));
    server.join().expect("the server");
}

#[test]
fn a_response_head_that_is_malformed_too_large_or_cut_short_is_an_error() {
    let mut huge = b"HTTP/1.1 200 OK\r\nX-Pad: ".to_vec();
    huge.extend(std::iter::repeat_n(b'a', MAX_HEAD_BYTES + READ_BLOCK));
    let cases: [(Vec<u8>, io::ErrorKind, &str); 5] = [
        (b"HTTP/1.1 nope\r\n\r\n".to_vec(), io::ErrorKind::InvalidData, "invalid HTTP response"),
        (b"HTTP/1.1 200".to_vec(), io::ErrorKind::UnexpectedEof, "before a complete HTTP response"),
        (b"HTTP/1.1 200 OK\r\nX-Bad: \xff\r\n\r\n".to_vec(), io::ErrorKind::InvalidData, ""),
        (huge, io::ErrorKind::InvalidData, "head is larger than"),
        (
            b"HTTP/1.1 200 OK\r\nContent-Length: 1\r\nContent-Length: 2\r\n\r\n".to_vec(),
            io::ErrorKind::InvalidData,
            "conflicting",
        ),
    ];
    for (reply, kind, message) in cases {
        let (base, _, server) = serve(1, move |_| Reply::SendAndClose(reply.clone()));
        let error = get(&base).err().expect("a bad head");
        assert_eq!(error.kind(), kind, "{error}");
        assert!(error.to_string().contains(message), "{error}");
        server.join().expect("the server");
    }
}

#[test]
fn the_pool_keeps_a_bounded_number_of_connections_per_host() {
    let total = MAX_IDLE_PER_HOST + 2;
    let listener = StdListener::bind("127.0.0.1:0").expect("a test server binds");
    let address = listener.local_addr().expect("an address");
    let server = thread::spawn(move || {
        let mut streams = Vec::new();
        for connection in 0..total {
            let (mut stream, _) = listener.accept().expect("a connection");
            read_request(&mut stream, connection).expect("a request");
            stream.write_all(&ok("x")).expect("a reply");
            streams.push(stream);
        }
        streams
    });
    let base = format!("http://{address}");
    // Hold every response open so each request needs its own connection.
    let mut open = Vec::new();
    for _ in 0..total {
        open.push(get(&base).expect("a response"));
    }
    for response in &mut open {
        assert_eq!(text(response), "x");
    }
    assert_eq!(pool_discard(address.port()), MAX_IDLE_PER_HOST);
    drop(server.join().expect("the server"));
}

#[test]
fn idle_connections_can_be_counted_and_closed() {
    let (base, port, server) = serve(1, |_| Reply::Send(ok("x")));
    let mut response = get(&base).expect("a response");
    text(&mut response);
    assert!(http_pool_idle_connections() >= 1);
    assert_eq!(pool_discard(port), 1);
    server.join().expect("the server");
    // Clearing the whole pool is covered through the host builtin below,
    // on a connection this test owns.
    let (base, port, server) = serve(1, |_| Reply::Send(ok("x")));
    let mut response = get(&base).expect("a response");
    text(&mut response);
    let pooled = PoolKey {
        tls: false,
        host: "127.0.0.1".to_string(),
        port,
        ca_pem_path: String::new(),
    };
    let connection = pool_take(&pooled).expect("the kept connection");
    assert!(connection.reused);
    assert!(pool_take(&pooled).is_none());
    drop(connection);
    server.join().expect("the server");
}

fn host(name: &str, args: Vec<Value>) -> Result<Value> {
    host_builtins::host_builtin_function(name).expect("an http builtin")(args, None)
}

fn string_map(entries: &[(&str, &str)]) -> Value {
    Value::Map(MapValue {
        key_type: SharedType::named("str"),
        value_type: SharedType::named("str"),
        entries: entries
            .iter()
            .map(|(key, value)| (Value::string(*key), Value::string(*value)))
            .collect(),
    })
}

fn send_args(method: &str, url: &str, text: &str, body: &[u8], flags: i64) -> Vec<Value> {
    vec![
        Value::string(method),
        Value::string(url),
        string_map(&[("X-Test", "yes")]),
        Value::string(text),
        bytes_list_value(body.to_vec()),
        Value::Duration(5_000_000_000),
        Value::Duration(5_000_000_000),
        int_value(10),
        int_value(flags),
        Value::string(""),
    ]
}

fn ok_payload(value: Value) -> Value {
    match value {
        Value::EnumVariant(variant) if variant.variant_name.as_str() == "Ok" => {
            variant.payloads[0].clone()
        }
        other => panic!("expected Result.Ok, found {other:?}"),
    }
}

fn err_variant(value: Value) -> String {
    match value {
        Value::EnumVariant(variant) if variant.variant_name.as_str() == "Err" => {
            match &variant.payloads[0] {
                Value::EnumVariant(error) => error.variant_name.as_str().to_string(),
                other => panic!("expected an io.Error, found {other:?}"),
            }
        }
        other => panic!("expected Result.Err, found {other:?}"),
    }
}

fn tuple(value: Value) -> Vec<Value> {
    match value {
        Value::Tuple(tuple) => tuple.elements,
        other => panic!("expected a tuple, found {other:?}"),
    }
}

#[test]
fn the_host_builtins_send_a_request_and_read_its_body() {
    let (base, port, server) = serve(1, |request| {
        if request.line().contains("/events") {
            Reply::Send(ok("data: one\n\nretry: 5\ndata: two\n\n"))
        } else {
            Reply::Send(ok(&format!(
                "{}|{}\nsecond line\nthird",
                request.header("x-test").unwrap_or("-"),
                String::from_utf8_lossy(&request.body)
            )))
        }
    });
    let sent = tuple(ok_payload(
        host("http::__aura_send", send_args("POST", &format!("{base}/text"), "from text", b"", 15))
            .expect("a send"),
    ));
    assert_eq!(sent[1], int_value(200));
    assert_eq!(sent[2], Value::string("OK"));
    assert_eq!(sent[4], Value::string(format!("{base}/text")));
    assert_eq!(sent[3].render(), "[content-length, 31]");
    let stream = sent[0].clone();
    assert_eq!(stream.render(), "<http-stream>");
    assert_eq!(stream, stream.clone());

    let line = ok_payload(host("http::__aura_read_line", vec![stream.clone()]).expect("a line"));
    assert_eq!(line.render(), "yes|from text");
    let lines = ok_payload(
        host("http::__aura_read_lines", vec![stream.clone(), int_value(1)]).expect("lines"),
    );
    assert_eq!(lines.render(), "[second line]");
    let chunk = ok_payload(
        host("http::__aura_read_chunk", vec![stream.clone(), int_value(2)]).expect("a chunk"),
    );
    assert_eq!(chunk.render(), "[116, 104]");
    let rest = ok_payload(
        host("http::__aura_read_text", vec![stream.clone(), int_value(100)]).expect("the rest"),
    );
    assert_eq!(rest, Value::string("ird"));
    let end = ok_payload(
        host("http::__aura_read_chunk", vec![stream.clone(), int_value(2)]).expect("the end"),
    );
    assert_eq!(end.render(), "None");
    let end = ok_payload(host("http::__aura_read_line", vec![stream.clone()]).expect("the end"));
    assert_eq!(end.render(), "None");
    assert_eq!(
        host("http::__aura_close", vec![stream.clone()]).expect("a close"),
        Value::Unit
    );
    assert_eq!(
        err_variant(host("http::__aura_read_line", vec![stream]).expect("a closed read")),
        "Closed"
    );

    // A byte body wins over the text argument.
    let sent = tuple(ok_payload(
        host("http::__aura_send", send_args("POST", &format!("{base}/bytes"), "", b"raw", 3))
            .expect("a send"),
    ));
    let all = ok_payload(
        host("http::__aura_read_all", vec![sent[0].clone(), int_value(100)]).expect("a body"),
    );
    assert_eq!(all.render(), "[121, 101, 115, 124, 114, 97, 119, 10, 115, 101, 99, 111, 110, 100, 32, 108, 105, 110, 101, 10, 116, 104, 105, 114, 100]");

    let sent = tuple(ok_payload(
        host("http::__aura_send", send_args("GET", &format!("{base}/events"), "", b"", 3))
            .expect("a send"),
    ));
    let read_event = |stream: &Value| {
        tuple(ok_payload(
            host("http::__aura_read_event", vec![stream.clone()]).expect("an event"),
        ))
    };
    let first = read_event(&sent[0]);
    assert_eq!(first[0], Value::Bool(true));
    assert_eq!(first[1], Value::string("message"));
    assert_eq!(first[2], Value::string("one"));
    assert_eq!(first[4], int_value(-1));
    assert_eq!(read_event(&sent[0])[4], int_value(5));
    let end = read_event(&sent[0]);
    assert_eq!(end[0], Value::Bool(false));
    assert_eq!(end[4], int_value(-1));

    assert!(matches!(
        host("http::__aura_idle_connections", Vec::new()).expect("a count"),
        Value::Int(_)
    ));
    assert_eq!(pool_discard(port), 1);
    server.join().expect("the server");
}

#[test]
fn the_host_builtins_report_request_errors_as_io_errors_and_bad_arguments_as_traps() {
    let url = "http://127.0.0.1:1/";
    let mut negative = send_args("GET", url, "", b"", 4);
    negative[5] = Value::Duration(-1);
    assert_eq!(
        err_variant(host("http::__aura_send", negative).expect("a result")),
        "InvalidInput"
    );
    let mut negative_read = send_args("GET", url, "", b"", 8);
    negative_read[6] = Value::Duration(-1);
    assert_eq!(
        err_variant(host("http::__aura_send", negative_read).expect("a result")),
        "InvalidInput"
    );
    // A cleared flag ignores the duration beside it.
    let mut ignored = send_args("GET", "not-a-url", "", b"", 0);
    ignored[5] = Value::Duration(-1);
    assert_eq!(
        err_variant(host("http::__aura_send", ignored).expect("a result")),
        "InvalidInput"
    );
    assert_eq!(
        err_variant(host("http::__aura_send", send_args("GET", "not-a-url", "", b"", 3)).unwrap()),
        "InvalidInput"
    );

    let trap = |args: Vec<Value>| {
        host("http::__aura_send", args)
            .expect_err("a bad argument")
            .message
    };
    let mut bad_headers = send_args("GET", url, "", b"", 3);
    bad_headers[2] = int_value(1);
    assert!(trap(bad_headers).contains("expects headers as a `dict[str, str]`"));
    let mut bad_header_value = send_args("GET", url, "", b"", 3);
    bad_header_value[2] = Value::Map(MapValue {
        key_type: SharedType::named("str"),
        value_type: SharedType::named("int64"),
        entries: vec![(Value::string("a"), int_value(1))].into_iter().collect(),
    });
    assert!(trap(bad_header_value).contains("expects headers as a `dict[str, str]`"));
    for redirects in [-1, MAX_REDIRECT_LIMIT + 1] {
        let mut bad_redirects = send_args("GET", url, "", b"", 3);
        bad_redirects[7] = int_value(redirects);
        assert!(trap(bad_redirects).contains("expects a redirect limit from 0 to 100"));
    }
    let mut bad_timeout = send_args("GET", url, "", b"", 3);
    bad_timeout[5] = int_value(1);
    assert!(trap(bad_timeout).contains("expects a `Duration`"));
    let mut bad_flags = send_args("GET", url, "", b"", 3);
    bad_flags[8] = Value::string("x");
    assert!(trap(bad_flags).contains("expects an `int64`"));
    assert!(trap(Vec::new()).contains("expects 10 arguments"));

    for name in [
        "http::__aura_read_line",
        "http::__aura_read_event",
        "http::__aura_close",
    ] {
        let error = host(name, vec![int_value(1)]).expect_err("not a stream");
        assert!(error.message.contains("expects an `http.Stream`"), "{name}");
    }
    for name in [
        "http::__aura_read_chunk",
        "http::__aura_read_lines",
        "http::__aura_read_all",
        "http::__aura_read_text",
    ] {
        let error = host(name, vec![int_value(1), int_value(1)]).expect_err("not a stream");
        assert!(error.message.contains("expects an `http.Stream`"), "{name}");
    }
    assert!(http_host_function("http::__aura_missing").is_none());
}

#[test]
fn a_text_read_of_a_body_that_is_not_utf8_is_invalid_data() {
    let (base, port, server) = serve(1, |_| {
        let mut reply = b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\n".to_vec();
        reply.extend_from_slice(b"\xff\xfe");
        Reply::Send(reply)
    });
    let sent = tuple(ok_payload(
        host("http::__aura_send", send_args("GET", &base, "", b"", 3)).expect("a send"),
    ));
    assert_eq!(
        err_variant(
            host("http::__aura_read_text", vec![sent[0].clone(), int_value(10)]).expect("a read")
        ),
        "InvalidData"
    );
    pool_discard(port);
    server.join().expect("the server");
}

#[test]
fn a_stream_in_use_reads_as_closed_instead_of_blocking() {
    let (base, port, server) = serve(1, |_| Reply::Send(ok("x")));
    let stream = HttpResponseValue::new(get(&base).expect("a response"));
    let nested = stream.with_body(|_| {
        let inner = stream.with_body(|body| body.read_chunk(1, None));
        Ok(inner.expect_err("the body is out of its lock").to_string())
    });
    assert_eq!(nested.unwrap(), "resource is closed");
    // Closing during use leaves the body for its user.
    stream
        .with_body(|_| {
            stream.close();
            Ok(())
        })
        .unwrap();
    assert_eq!(
        stream.with_body(|body| body.read_chunk(8, None)).unwrap().unwrap(),
        b"x"
    );
    assert_eq!(format!("{stream:?}"), "<http-stream>");
    pool_discard(port);
    server.join().expect("the server");
}

#[test]
fn the_pool_can_be_cleared() {
    // Runs against the shared pool, so it only checks that clearing does
    // not disturb a connection that is in use.
    let (base, port, server) = serve(1, |_| Reply::Send(ok("x")));
    let mut response = get(&base).expect("a response");
    assert_eq!(
        host("http::__aura_close_idle", Vec::new()).expect("a clear"),
        Value::Unit
    );
    assert_eq!(text(&mut response), "x");
    pool_discard(port);
    server.join().expect("the server");
}
