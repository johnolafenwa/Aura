//! The HTTP/1.1 client under the `http` module.
//!
//! A request returns as soon as the response head has arrived. The body
//! stays on the connection and is read in pieces: bytes, lines, or
//! server-sent events. The client follows redirects, decodes gzip and
//! deflate bodies as they arrive, and keeps finished connections in a
//! process-wide pool so the next request to the same host skips the TCP
//! and TLS handshakes.
//!
//! `std/http.au` holds the typed API. It keeps an [`HttpResponseValue`] as
//! the private `http.Stream` field of each `http.Response`, so dropping a
//! response closes its connection.

use super::*;
use flate2::write::{MultiGzDecoder, ZlibDecoder};

/// The most bytes a response head may take.
const MAX_HEAD_BYTES: usize = 256 * 1024;
/// The bytes asked of the connection in one read.
const READ_BLOCK: usize = 16 * 1024;
/// The longest chunk-size line or trailer section accepted.
const MAX_CHUNK_LINE_BYTES: usize = 64 * 1024;
/// The longest line `read_line` returns.
const MAX_LINE_BYTES: usize = 64 * 1024 * 1024;
/// The most lines one `read_lines` call returns.
const MAX_LINES_PER_CALL: i64 = 1_048_576;
/// The most bytes one `read_chunk` call returns, and the largest limit a
/// whole-body read accepts.
pub const MAX_HTTP_BODY_READ_BYTES: usize = 256 * 1024 * 1024;
/// Idle connections kept for one host and port.
const MAX_IDLE_PER_HOST: usize = 8;
/// Idle connections kept in all.
const MAX_IDLE_TOTAL: usize = 64;
/// How long an idle connection is kept.
const IDLE_TIMEOUT: StdDuration = StdDuration::from_secs(90);
/// The most body bytes read and thrown away to reuse the connection of a
/// redirect response.
const MAX_REDIRECT_DRAIN_BYTES: usize = 256 * 1024;
/// The most redirects a request may ask to follow.
const MAX_REDIRECT_LIMIT: i64 = 100;

/// What identifies a server a connection can be reused for.
#[derive(Clone, Debug, PartialEq, Eq)]
struct PoolKey {
    tls: bool,
    host: String,
    port: u16,
    ca_pem_path: String,
}

enum Transport {
    Plain(StdTcpStream),
    /// Empty only after a failed step, which loses the stream.
    #[cfg(unix)]
    Tls(Option<Box<TlsClientStream>>),
}

impl Transport {
    fn read_some(
        &mut self,
        max_bytes: usize,
        deadline: Option<Instant>,
        cancellation: Option<&CancellationContext>,
    ) -> io::Result<Option<Vec<u8>>> {
        match self {
            Transport::Plain(stream) => {
                read_some_with_deadline(stream, max_bytes, deadline, cancellation)
            }
            #[cfg(unix)]
            Transport::Tls(slot) => {
                let stream = slot.take().ok_or_else(closed_resource_error)?;
                let (stream, bytes) =
                    tls_client_read_some_owned(*stream, max_bytes, deadline, cancellation)?;
                *slot = Some(Box::new(stream));
                Ok(bytes)
            }
        }
    }

    fn write_all(
        &mut self,
        bytes: &[u8],
        deadline: Option<Instant>,
        cancellation: Option<&CancellationContext>,
    ) -> io::Result<()> {
        match self {
            Transport::Plain(stream) => {
                write_all_with_deadline(stream, bytes, deadline, cancellation)
            }
            #[cfg(unix)]
            Transport::Tls(slot) => {
                let stream = slot.take().ok_or_else(closed_resource_error)?;
                let stream = write_all_tls_client_owned(*stream, bytes, deadline, cancellation)?;
                *slot = Some(Box::new(stream));
                Ok(())
            }
        }
    }

    /// Whether an idle connection has anything to read: the server closed
    /// it or sent bytes no request asked for. Either way it cannot carry
    /// another request.
    fn is_stale(&self) -> bool {
        #[cfg(unix)]
        {
            let fd = match self {
                Transport::Plain(stream) => stream.as_raw_fd(),
                Transport::Tls(Some(stream)) => stream.sock.as_raw_fd(),
                Transport::Tls(None) => return true,
            };
            let mut poll = libc::pollfd {
                fd,
                events: libc::POLLIN,
                revents: 0,
            };
            // SAFETY: `poll` points at one valid `pollfd`, and a zero
            // timeout returns at once.
            let ready = unsafe { libc::poll(&mut poll, 1, 0) };
            ready != 0
        }
        #[cfg(not(unix))]
        {
            false
        }
    }
}

struct Connection {
    transport: Transport,
    key: PoolKey,
    /// Bytes read from the transport; `buffer[start..]` is not yet used.
    buffer: Vec<u8>,
    start: usize,
    /// Whether the connection came from the pool.
    reused: bool,
}

impl Connection {
    fn unread(&self) -> &[u8] {
        &self.buffer[self.start..]
    }

    fn consume(&mut self, count: usize) {
        self.start += count;
        if self.start == self.buffer.len() {
            self.buffer.clear();
            self.start = 0;
        }
    }

    /// Reads more bytes. `Ok(false)` is the end of the stream.
    fn fill(
        &mut self,
        deadline: Option<Instant>,
        cancellation: Option<&CancellationContext>,
    ) -> io::Result<bool> {
        if self.start > 0 && self.start >= self.buffer.len() / 2 {
            self.buffer.drain(..self.start);
            self.start = 0;
        }
        match self
            .transport
            .read_some(READ_BLOCK, deadline, cancellation)?
        {
            Some(bytes) => {
                self.buffer.extend_from_slice(&bytes);
                Ok(true)
            }
            None => Ok(false),
        }
    }
}

struct IdleConnection {
    connection: Connection,
    since: Instant,
}

static POOL: Mutex<Vec<IdleConnection>> = Mutex::new(Vec::new());

/// The newest live idle connection to `key`, if the pool has one.
fn pool_take(key: &PoolKey) -> Option<Connection> {
    let mut dropped = Vec::new();
    let found = {
        let mut pool = lock_mutex(&POOL);
        let mut found = None;
        let mut index = pool.len();
        while index > 0 {
            index -= 1;
            if pool[index].since.elapsed() > IDLE_TIMEOUT {
                dropped.push(pool.remove(index));
            } else if found.is_none() && pool[index].connection.key == *key {
                let idle = pool.remove(index);
                if idle.connection.transport.is_stale() {
                    dropped.push(idle);
                } else {
                    found = Some(idle.connection);
                }
            }
        }
        found
    };
    // Sockets close after the lock is released.
    drop(dropped);
    found.map(|mut connection| {
        connection.reused = true;
        connection
    })
}

fn pool_put(connection: Connection) {
    if !connection.unread().is_empty() {
        return;
    }
    let mut dropped = Vec::new();
    {
        let mut pool = lock_mutex(&POOL);
        let same_host = pool
            .iter()
            .filter(|idle| idle.connection.key == connection.key)
            .count();
        if same_host >= MAX_IDLE_PER_HOST {
            let oldest = pool
                .iter()
                .position(|idle| idle.connection.key == connection.key)
                .expect("a host at its idle limit has an idle connection");
            dropped.push(pool.remove(oldest));
        }
        if pool.len() >= MAX_IDLE_TOTAL {
            dropped.push(pool.remove(0));
        }
        pool.push(IdleConnection {
            connection,
            since: Instant::now(),
        });
    }
    drop(dropped);
}

/// The idle connections the pool holds.
pub fn http_pool_idle_connections() -> usize {
    lock_mutex(&POOL).len()
}

/// Closes every idle connection.
pub fn http_pool_clear() {
    let dropped = std::mem::take(&mut *lock_mutex(&POOL));
    drop(dropped);
}

fn connect(
    key: &PoolKey,
    deadline: Option<Instant>,
    cancellation: Option<&CancellationContext>,
) -> io::Result<Connection> {
    let address = if key.host.contains(':') {
        format!("[{}]:{}", key.host, key.port)
    } else {
        format!("{}:{}", key.host, key.port)
    };
    let transport = if key.tls {
        #[cfg(unix)]
        {
            let stream = TlsStreamValue::connect_before(
                &address,
                &key.host,
                Some(key.ca_pem_path.as_str()),
                deadline,
                cancellation,
            )?;
            let taken = lock_mutex(&stream.inner.stream).take();
            let Some(TlsStreamKind::Client(raw)) = taken else {
                return Err(closed_resource_error());
            };
            Transport::Tls(Some(Box::new(raw)))
        }
        #[cfg(not(unix))]
        {
            return Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "HTTPS requests are supported on Aura's Unix preview platforms",
            ));
        }
    } else {
        let stream = TcpStreamValue::connect_before(&address, deadline, cancellation)?;
        let taken = lock_mutex(&stream.inner.stream).take();
        let Some(raw) = taken else {
            return Err(closed_resource_error());
        };
        Transport::Plain(raw)
    };
    Ok(Connection {
        transport,
        key: key.clone(),
        buffer: Vec::new(),
        start: 0,
        reused: false,
    })
}

/// How a request is carried out.
#[derive(Clone, Debug)]
pub struct HttpRequestOptions {
    /// The limit from the start of the request to the arrival of the
    /// response head, redirects included.
    pub timeout: Option<StdDuration>,
    /// The limit of each read of the body.
    pub read_timeout: Option<StdDuration>,
    /// Redirects followed before the redirect response itself is returned
    /// as an error. Zero returns every redirect response to the caller.
    pub max_redirects: usize,
    /// Ask for gzip and deflate and decode them.
    pub decompress: bool,
    /// Keep finished connections for later requests and use kept ones.
    pub keep_alive: bool,
    /// A PEM file of the certificate authorities to trust instead of the
    /// built-in roots. Empty for the built-in roots.
    pub ca_pem_path: String,
}

impl Default for HttpRequestOptions {
    fn default() -> Self {
        Self {
            timeout: None,
            read_timeout: None,
            max_redirects: 10,
            decompress: true,
            keep_alive: true,
            ca_pem_path: String::new(),
        }
    }
}

enum Decoder {
    Gzip(MultiGzDecoder<Vec<u8>>),
    Deflate(ZlibDecoder<Vec<u8>>),
}

impl Decoder {
    fn output(&mut self) -> &mut Vec<u8> {
        match self {
            Decoder::Gzip(decoder) => decoder.get_mut(),
            Decoder::Deflate(decoder) => decoder.get_mut(),
        }
    }

    fn feed(&mut self, bytes: &[u8], decoded: &mut Vec<u8>) -> io::Result<()> {
        let result = match self {
            Decoder::Gzip(decoder) => decoder.write_all(bytes),
            Decoder::Deflate(decoder) => decoder.write_all(bytes),
        };
        decoded.append(self.output());
        result.map_err(invalid_compressed_body)
    }

    fn finish(&mut self, decoded: &mut Vec<u8>) -> io::Result<()> {
        let result = match self {
            Decoder::Gzip(decoder) => decoder.try_finish(),
            Decoder::Deflate(decoder) => decoder.try_finish(),
        };
        decoded.append(self.output());
        result.map_err(invalid_compressed_body)
    }
}

fn invalid_compressed_body(error: io::Error) -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        format!("invalid compressed HTTP response body: {error}"),
    )
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Framing {
    /// This many body bytes remain.
    Length(u64),
    /// A chunk-size line comes next.
    ChunkSize,
    /// This many bytes of the current chunk remain.
    ChunkData(u64),
    /// The CRLF after a chunk's data comes next.
    ChunkEnd,
    /// The trailer section comes next.
    Trailers,
    /// The body runs until the server closes the connection.
    UntilClose,
    /// The whole body has been read.
    Done,
}

fn truncated_body() -> io::Error {
    io::Error::new(
        io::ErrorKind::UnexpectedEof,
        "connection closed before the HTTP response body was complete",
    )
}

fn invalid_chunk(message: &str) -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        format!("invalid chunked HTTP response body: {message}"),
    )
}

/// One server-sent event.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct HttpServerEvent {
    /// The `event` field, or `message` when the event has none.
    pub event: String,
    /// The `data` lines joined with newlines.
    pub data: String,
    /// The last `id` field the stream has sent.
    pub id: String,
    /// The `retry` field in milliseconds, or -1 when the event has none.
    pub retry_ms: i64,
}

/// A response whose body is still on its connection.
pub struct HttpBody {
    status: i32,
    reason: String,
    headers: Vec<(String, String)>,
    url: String,
    connection: Option<Connection>,
    framing: Framing,
    decoder: Option<Decoder>,
    decoder_fed: bool,
    /// Decoded body bytes; `decoded[start..]` has not been handed out.
    decoded: Vec<u8>,
    start: usize,
    /// How far `read_line` has searched `decoded` for a line feed.
    scanned: usize,
    reusable: bool,
    read_timeout: Option<StdDuration>,
    closed: bool,
    last_event_id: String,
    at_stream_start: bool,
}

impl HttpBody {
    pub fn status(&self) -> i32 {
        self.status
    }

    pub fn reason(&self) -> &str {
        &self.reason
    }

    /// The response headers in the order sent, names in lower case.
    pub fn headers(&self) -> &[(String, String)] {
        &self.headers
    }

    /// The URL the response came from, after redirects.
    pub fn url(&self) -> &str {
        &self.url
    }

    fn available(&self) -> usize {
        self.decoded.len() - self.start
    }

    fn consume(&mut self, count: usize) {
        self.start += count;
        self.scanned = self.scanned.max(self.start);
        if self.start == self.decoded.len() {
            self.decoded.clear();
            self.start = 0;
            self.scanned = 0;
        } else if self.start >= 64 * 1024 && self.start >= self.decoded.len() / 2 {
            self.decoded.drain(..self.start);
            self.scanned -= self.start;
            self.start = 0;
        }
    }

    /// Ends the body. A connection whose response was read to its end goes
    /// back to the pool.
    fn finish(&mut self, complete: bool) -> io::Result<()> {
        self.framing = Framing::Done;
        let connection = self.connection.take();
        if let Some(connection) = connection {
            if complete && self.reusable {
                pool_put(connection);
            }
        }
        if complete && self.decoder_fed {
            if let Some(mut decoder) = self.decoder.take() {
                decoder.finish(&mut self.decoded)?;
            }
        }
        self.decoder = None;
        Ok(())
    }

    fn fail<T>(&mut self, error: io::Error) -> io::Result<T> {
        self.framing = Framing::Done;
        self.connection = None;
        self.decoder = None;
        Err(error)
    }

    fn feed(&mut self, count: usize) -> io::Result<()> {
        let connection = self
            .connection
            .as_mut()
            .expect("a body with bytes to feed has a connection");
        let bytes = &connection.buffer[connection.start..connection.start + count];
        let result = match &mut self.decoder {
            Some(decoder) => {
                self.decoder_fed = true;
                decoder.feed(bytes, &mut self.decoded)
            }
            None => {
                self.decoded.extend_from_slice(bytes);
                Ok(())
            }
        };
        connection.consume(count);
        result
    }

    /// Reads more of the connection when nothing is waiting in its buffer.
    /// `Ok(false)` is the end of the stream.
    fn need_bytes(
        &mut self,
        deadline: Option<Instant>,
        cancellation: Option<&CancellationContext>,
    ) -> io::Result<bool> {
        let Some(connection) = self.connection.as_mut() else {
            return Err(closed_resource_error());
        };
        connection.fill(deadline, cancellation)
    }

    /// Moves the next piece of the body into `decoded`. `Ok(false)` is the
    /// end of the body. A step may add no bytes.
    fn pull(
        &mut self,
        deadline: Option<Instant>,
        cancellation: Option<&CancellationContext>,
    ) -> io::Result<bool> {
        match self.pull_step(deadline, cancellation) {
            Ok(more) => Ok(more),
            Err(error) => self.fail(error),
        }
    }

    fn pull_step(
        &mut self,
        deadline: Option<Instant>,
        cancellation: Option<&CancellationContext>,
    ) -> io::Result<bool> {
        loop {
            let unread = self
                .connection
                .as_ref()
                .map_or(0, |connection| connection.unread().len());
            match self.framing {
                Framing::Done => return Ok(false),
                Framing::Length(0) => {
                    self.finish(true)?;
                    return Ok(false);
                }
                Framing::Length(remaining) | Framing::ChunkData(remaining) => {
                    if unread == 0 {
                        if !self.need_bytes(deadline, cancellation)? {
                            return Err(truncated_body());
                        }
                        continue;
                    }
                    let count = usize::try_from(remaining).map_or(unread, |r| r.min(unread));
                    self.feed(count)?;
                    let left = remaining - count as u64;
                    self.framing = match self.framing {
                        Framing::Length(_) => Framing::Length(left),
                        _ if left == 0 => Framing::ChunkEnd,
                        _ => Framing::ChunkData(left),
                    };
                    if self.framing == Framing::Length(0) {
                        self.finish(true)?;
                    }
                    return Ok(true);
                }
                Framing::UntilClose => {
                    if unread == 0 {
                        let more = match self.need_bytes(deadline, cancellation) {
                            Ok(more) => more,
                            // A TLS server that closes without a close
                            // notice still ended the body.
                            Err(error) if error.kind() == io::ErrorKind::UnexpectedEof => false,
                            Err(error) => return Err(error),
                        };
                        if !more {
                            self.finish(true)?;
                            return Ok(false);
                        }
                        continue;
                    }
                    self.feed(unread)?;
                    return Ok(true);
                }
                Framing::ChunkSize => {
                    let Some(line) = self.chunk_line(deadline, cancellation)? else {
                        continue;
                    };
                    let digits = line.split(';').next().unwrap_or("").trim();
                    let size = u64::from_str_radix(digits, 16)
                        .ok()
                        .filter(|_| {
                            !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_hexdigit())
                        })
                        .ok_or_else(|| invalid_chunk("a chunk size is not a hex number"))?;
                    self.framing = if size == 0 {
                        Framing::Trailers
                    } else {
                        Framing::ChunkData(size)
                    };
                }
                Framing::ChunkEnd => {
                    if unread < 2 {
                        if !self.need_bytes(deadline, cancellation)? {
                            return Err(truncated_body());
                        }
                        continue;
                    }
                    let connection = self.connection.as_mut().expect("unread bytes");
                    if &connection.unread()[..2] != b"\r\n" {
                        return Err(invalid_chunk("a chunk does not end with CRLF"));
                    }
                    connection.consume(2);
                    self.framing = Framing::ChunkSize;
                }
                Framing::Trailers => {
                    let Some(line) = self.chunk_line(deadline, cancellation)? else {
                        continue;
                    };
                    if line.is_empty() {
                        self.finish(true)?;
                        return Ok(false);
                    }
                }
            }
        }
    }

    /// The next CRLF-terminated line of chunk framing, or `None` after
    /// reading more bytes to find its end.
    fn chunk_line(
        &mut self,
        deadline: Option<Instant>,
        cancellation: Option<&CancellationContext>,
    ) -> io::Result<Option<String>> {
        let connection = self
            .connection
            .as_mut()
            .ok_or_else(closed_resource_error)?;
        let unread = connection.unread();
        let Some(end) = unread.windows(2).position(|pair| pair == b"\r\n") else {
            if unread.len() > MAX_CHUNK_LINE_BYTES {
                return Err(invalid_chunk("a framing line is too long"));
            }
            if !connection.fill(deadline, cancellation)? {
                return Err(truncated_body());
            }
            return Ok(None);
        };
        let line = std::str::from_utf8(&unread[..end])
            .map_err(|_| invalid_chunk("a framing line is not text"))?
            .to_string();
        connection.consume(end + 2);
        Ok(Some(line))
    }

    fn read_deadline(&self) -> io::Result<Option<Instant>> {
        deadline_from_timeout(self.read_timeout)
    }

    fn check_open(&self) -> io::Result<()> {
        if self.closed {
            return Err(closed_resource_error());
        }
        Ok(())
    }

    /// The next bytes of the body, at most `max_bytes` of them, or `None`
    /// at its end. Returns what has arrived without waiting to fill the
    /// request.
    pub fn read_chunk(
        &mut self,
        max_bytes: i64,
        cancellation: Option<&CancellationContext>,
    ) -> io::Result<Option<Vec<u8>>> {
        self.check_open()?;
        let max_bytes = usize::try_from(max_bytes)
            .ok()
            .filter(|max| (1..=MAX_HTTP_BODY_READ_BYTES).contains(max))
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!(
                        "read_chunk(...) requires a byte count from 1 to {MAX_HTTP_BODY_READ_BYTES}"
                    ),
                )
            })?;
        let deadline = self.read_deadline()?;
        while self.available() == 0 {
            if !self.pull(deadline, cancellation)? && self.available() == 0 {
                return Ok(None);
            }
        }
        let count = self.available().min(max_bytes);
        let bytes = self.decoded[self.start..self.start + count].to_vec();
        self.consume(count);
        Ok(Some(bytes))
    }

    /// The bytes of the next line without its line ending, or `None` at
    /// the end of the body. A final line without a line feed is a line.
    fn read_line_bytes(
        &mut self,
        deadline: Option<Instant>,
        cancellation: Option<&CancellationContext>,
        wait: bool,
    ) -> io::Result<Option<Vec<u8>>> {
        loop {
            let from = self.scanned.max(self.start);
            if let Some(offset) = self.decoded[from..].iter().position(|byte| *byte == b'\n') {
                let end = from + offset;
                let mut line = self.decoded[self.start..end].to_vec();
                if line.last() == Some(&b'\r') {
                    line.pop();
                }
                self.consume(end + 1 - self.start);
                return Ok(Some(line));
            }
            self.scanned = self.decoded.len();
            if self.available() > MAX_LINE_BYTES {
                return self.fail(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("a line of the HTTP response body is longer than {MAX_LINE_BYTES} bytes"),
                ));
            }
            if !wait {
                return Ok(None);
            }
            if !self.pull(deadline, cancellation)? {
                // The end of the body may have flushed the decoder.
                if self.decoded[self.scanned..].contains(&b'\n') {
                    continue;
                }
                if self.available() == 0 {
                    return Ok(None);
                }
                let line = self.decoded[self.start..].to_vec();
                let count = self.available();
                self.consume(count);
                return Ok(Some(line));
            }
        }
    }

    fn line_text(line: Vec<u8>) -> io::Result<String> {
        String::from_utf8(line).map_err(|_| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "a line of the HTTP response body is not valid UTF-8",
            )
        })
    }

    /// The next line of the body without its line ending, or `None` at
    /// the end.
    pub fn read_line(
        &mut self,
        cancellation: Option<&CancellationContext>,
    ) -> io::Result<Option<String>> {
        self.check_open()?;
        let deadline = self.read_deadline()?;
        self.read_line_bytes(deadline, cancellation, true)?
            .map(Self::line_text)
            .transpose()
    }

    /// The next lines of the body, at most `max_lines` of them. Waits for
    /// the first line, then returns it with the complete lines that have
    /// already arrived. An empty list is the end of the body.
    pub fn read_lines(
        &mut self,
        max_lines: i64,
        cancellation: Option<&CancellationContext>,
    ) -> io::Result<Vec<String>> {
        self.check_open()?;
        if !(1..=MAX_LINES_PER_CALL).contains(&max_lines) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("read_lines(...) requires a line count from 1 to {MAX_LINES_PER_CALL}"),
            ));
        }
        let deadline = self.read_deadline()?;
        let mut lines = Vec::new();
        while (lines.len() as i64) < max_lines {
            let wait = lines.is_empty();
            let Some(line) = self.read_line_bytes(deadline, cancellation, wait)? else {
                break;
            };
            lines.push(Self::line_text(line)?);
        }
        Ok(lines)
    }

    /// The next server-sent event, or `None` at the end of the body. An
    /// event the stream ends in the middle of is dropped, as the event
    /// stream format requires.
    pub fn read_event(
        &mut self,
        cancellation: Option<&CancellationContext>,
    ) -> io::Result<Option<HttpServerEvent>> {
        self.check_open()?;
        let deadline = self.read_deadline()?;
        let mut event = String::new();
        let mut data = String::new();
        let mut has_data = false;
        let mut retry_ms = -1i64;
        loop {
            let Some(line) = self.read_line_bytes(deadline, cancellation, true)? else {
                return Ok(None);
            };
            let mut line = Self::line_text(line)?;
            if std::mem::take(&mut self.at_stream_start) && line.starts_with('\u{feff}') {
                line.drain(..'\u{feff}'.len_utf8());
            }
            if line.is_empty() {
                if !has_data {
                    event.clear();
                    retry_ms = -1;
                    continue;
                }
                // The format ends every data line with a line feed and
                // drops the last one.
                data.pop();
                return Ok(Some(HttpServerEvent {
                    event: if event.is_empty() {
                        "message".to_string()
                    } else {
                        event
                    },
                    data,
                    id: self.last_event_id.clone(),
                    retry_ms,
                }));
            }
            if line.starts_with(':') {
                continue;
            }
            let (field, value) = match line.split_once(':') {
                Some((field, value)) => (field, value.strip_prefix(' ').unwrap_or(value)),
                None => (line.as_str(), ""),
            };
            match field {
                "event" => event = value.to_string(),
                "data" => {
                    has_data = true;
                    data.push_str(value);
                    data.push('\n');
                }
                "id" if !value.contains('\0') => self.last_event_id = value.to_string(),
                "retry" if !value.is_empty() && value.bytes().all(|b| b.is_ascii_digit()) => {
                    retry_ms = value.parse().unwrap_or(i64::MAX);
                }
                _ => {}
            }
        }
    }

    /// The rest of the body. Fails with `InvalidData` when it is larger
    /// than `max_bytes`.
    pub fn read_all(
        &mut self,
        max_bytes: i64,
        cancellation: Option<&CancellationContext>,
    ) -> io::Result<Vec<u8>> {
        self.check_open()?;
        let limit = usize::try_from(max_bytes)
            .ok()
            .filter(|max| *max <= MAX_HTTP_BODY_READ_BYTES)
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!(
                        "a whole-body read requires a byte limit from 0 to {MAX_HTTP_BODY_READ_BYTES}"
                    ),
                )
            })?;
        let deadline = self.read_deadline()?;
        if let Framing::Length(remaining) = self.framing {
            // A declared length over the limit fails before any of the
            // body is read. Only an undecoded body has a known length.
            let declared = (self.available() as u64).saturating_add(remaining);
            if self.decoder.is_none() && declared > limit as u64 {
                return self.fail(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("the HTTP response body is larger than {limit} bytes"),
                ));
            }
        }
        loop {
            if self.available() > limit {
                return self.fail(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("the HTTP response body is larger than {limit} bytes"),
                ));
            }
            if !self.pull(deadline, cancellation)? {
                if self.available() > limit {
                    continue;
                }
                break;
            }
        }
        let bytes = if self.start == 0 {
            std::mem::take(&mut self.decoded)
        } else {
            self.decoded[self.start..].to_vec()
        };
        let count = self.available();
        if count > 0 {
            self.consume(count);
        }
        self.scanned = 0;
        Ok(bytes)
    }

    /// Releases the connection. A body that was not read to its end closes
    /// its connection, since the unread bytes are still on it.
    pub fn close(&mut self) {
        self.closed = true;
        self.framing = Framing::Done;
        self.connection = None;
        self.decoder = None;
        self.decoded = Vec::new();
        self.start = 0;
        self.scanned = 0;
    }

    /// Reads and drops the rest of a small body so its connection can be
    /// reused.
    fn drain_for_reuse(
        &mut self,
        deadline: Option<Instant>,
        cancellation: Option<&CancellationContext>,
    ) {
        let mut dropped = 0usize;
        loop {
            dropped += self.available();
            let count = self.available();
            if count > 0 {
                self.consume(count);
            }
            if dropped > MAX_REDIRECT_DRAIN_BYTES {
                break;
            }
            match self.pull(deadline, cancellation) {
                Ok(true) => {}
                Ok(false) | Err(_) => break,
            }
        }
        self.close();
    }
}

struct ResponseHead {
    length: usize,
    status: i32,
    reason: String,
    headers: Vec<(String, String)>,
    /// HTTP/1.1 or later, where a connection stays open unless told not to.
    persistent_by_default: bool,
}

fn parse_head(buffer: &[u8]) -> io::Result<Option<ResponseHead>> {
    let mut raw_headers = [httparse::EMPTY_HEADER; MAX_HTTP_HEADERS];
    let mut response = HttpParseResponse::new(&mut raw_headers);
    let status = response.parse(buffer).map_err(|error| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("invalid HTTP response: {error}"),
        )
    })?;
    let HttpParseStatus::Complete(length) = status else {
        return Ok(None);
    };
    let status = i32::from(response.code.ok_or(io::Error::new(
        io::ErrorKind::InvalidData,
        "HTTP response missing status code",
    ))?);
    let reason = response
        .reason
        .filter(|reason| !reason.is_empty())
        .unwrap_or(http_reason_phrase(status))
        .to_string();
    let headers = parse_http_headers(response.headers)?
        .into_iter()
        .map(|(name, value)| (name.to_ascii_lowercase(), value))
        .collect();
    Ok(Some(ResponseHead {
        length,
        status,
        reason,
        headers,
        persistent_by_default: response.version == Some(1),
    }))
}

fn header_value<'a>(headers: &'a [(String, String)], name: &str) -> Option<&'a str> {
    headers
        .iter()
        .find(|(header, _)| header.eq_ignore_ascii_case(name))
        .map(|(_, value)| value.as_str())
}

fn header_has_token(headers: &[(String, String)], name: &str, token: &str) -> bool {
    headers
        .iter()
        .filter(|(header, _)| header.eq_ignore_ascii_case(name))
        .flat_map(|(_, value)| value.split(','))
        .any(|value| value.trim().eq_ignore_ascii_case(token))
}

fn is_idempotent(method: &str) -> bool {
    matches!(
        method,
        "GET" | "HEAD" | "PUT" | "DELETE" | "OPTIONS" | "TRACE"
    )
}

fn validate_method(method: &str) -> io::Result<()> {
    let valid = !method.is_empty()
        && method
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&byte));
    if valid {
        Ok(())
    } else {
        Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("invalid HTTP method `{method}`"),
        ))
    }
}

fn parse_request_url(text: &str) -> io::Result<(Url, PoolKey)> {
    let url = Url::parse(text).map_err(|error| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("invalid URL `{text}`: {error}"),
        )
    })?;
    if !matches!(url.scheme(), "http" | "https") {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("Aura HTTP requests require `http://` or `https://` URLs, found `{url}`"),
        ));
    }
    let host = match url.host() {
        Some(url::Host::Ipv6(host)) => host.to_string(),
        Some(url::Host::Ipv4(host)) => host.to_string(),
        Some(url::Host::Domain(host)) => host.to_ascii_lowercase(),
        None => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("invalid URL `{url}`: missing host"),
            ))
        }
    };
    let key = PoolKey {
        tls: url.scheme() == "https",
        host,
        port: url.port_or_known_default().unwrap_or(80),
        ca_pem_path: String::new(),
    };
    Ok((url, key))
}

/// Sends `request` on `connection` and reads the response head. The flag
/// in an error says whether the whole request had been written.
fn exchange(
    connection: &mut Connection,
    request: &[u8],
    deadline: Option<Instant>,
    cancellation: Option<&CancellationContext>,
) -> std::result::Result<ResponseHead, (io::Error, bool)> {
    connection
        .transport
        .write_all(request, deadline, cancellation)
        .map_err(|error| (error, false))?;
    loop {
        let head = loop {
            let parsed = run_http_step_before(
                || parse_head(connection.unread()),
                deadline,
                cancellation,
            )
            .map_err(|error| (error, true))?;
            if let Some(head) = parsed {
                break head;
            }
            if connection.unread().len() > MAX_HEAD_BYTES {
                return Err((
                    io::Error::new(
                        io::ErrorKind::InvalidData,
                        format!("the HTTP response head is larger than {MAX_HEAD_BYTES} bytes"),
                    ),
                    true,
                ));
            }
            let more = connection
                .fill(deadline, cancellation)
                .map_err(|error| (error, true))?;
            if !more {
                return Err((
                    io::Error::new(
                        io::ErrorKind::UnexpectedEof,
                        "stream closed before a complete HTTP response was received",
                    ),
                    true,
                ));
            }
        };
        connection.consume(head.length);
        match head.status {
            101 => {
                return Err((
                    io::Error::new(
                        io::ErrorKind::InvalidData,
                        "the server switched protocols on an HTTP request",
                    ),
                    true,
                ))
            }
            // An interim response; the real one follows.
            100..=199 => continue,
            _ => return Ok(head),
        }
    }
}

fn is_redirect(status: i32) -> bool {
    matches!(status, 301 | 302 | 303 | 307 | 308)
}

/// Starts reading the body that follows `head` on `connection`.
fn begin_body(
    connection: Connection,
    head: ResponseHead,
    method: &str,
    url: &str,
    decompress: bool,
    asked_to_close: bool,
    read_timeout: Option<StdDuration>,
) -> io::Result<HttpBody> {
    let no_body = method == "HEAD" || matches!(head.status, 204 | 304);
    let framing = if no_body {
        Framing::Length(0)
    } else {
        match parse_http_body_framing(&head.headers, HttpBodyFraming::UntilClose)? {
            HttpBodyFraming::ContentLength(length) => Framing::Length(length as u64),
            HttpBodyFraming::Chunked => Framing::ChunkSize,
            HttpBodyFraming::UntilClose => Framing::UntilClose,
        }
    };
    let server_closes = header_has_token(&head.headers, "connection", "close")
        || (!head.persistent_by_default
            && !header_has_token(&head.headers, "connection", "keep-alive"));
    let reusable = !asked_to_close && !server_closes && framing != Framing::UntilClose;
    let mut headers = head.headers;
    let decoder = if decompress && !no_body {
        match header_value(&headers, "content-encoding")
            .map(|value| value.trim().to_ascii_lowercase())
            .as_deref()
        {
            Some("gzip" | "x-gzip") => Some(Decoder::Gzip(MultiGzDecoder::new(Vec::new()))),
            Some("deflate") => Some(Decoder::Deflate(ZlibDecoder::new(Vec::new()))),
            _ => None,
        }
    } else {
        None
    };
    if decoder.is_some() {
        // The headers describe the bytes on the wire, not the decoded
        // bytes the caller reads.
        headers.retain(|(name, _)| name != "content-encoding" && name != "content-length");
    }
    let mut response = HttpBody {
        status: head.status,
        reason: head.reason,
        headers,
        url: url.to_string(),
        connection: Some(connection),
        framing,
        decoder,
        decoder_fed: false,
        decoded: Vec::new(),
        start: 0,
        scanned: 0,
        reusable,
        read_timeout,
        closed: false,
        last_event_id: String::new(),
        at_stream_start: true,
    };
    if response.framing == Framing::Length(0) {
        response.finish(true)?;
    }
    Ok(response)
}

/// Sends a request and returns once the response head has arrived.
pub fn http_send(
    method: &str,
    url: &str,
    headers: Vec<(String, String)>,
    body: &[u8],
    options: &HttpRequestOptions,
    cancellation: Option<&CancellationContext>,
) -> io::Result<HttpBody> {
    let deadline = deadline_from_timeout(options.timeout)?;
    validate_method(method)?;
    validate_http_headers(&headers)?;
    let mut method = method.to_string();
    let mut headers = headers;
    let mut body = body;
    let (mut url, mut key) = parse_request_url(url)?;
    let mut redirects = 0usize;
    loop {
        key.ca_pem_path.clone_from(&options.ca_pem_path);
        let mut request_headers = headers.clone();
        let has = |name: &str| header_value(&headers, name).is_some();
        let asked_for_compression = options.decompress && !has("accept-encoding");
        if asked_for_compression {
            request_headers.push(("Accept-Encoding".to_string(), "gzip, deflate".to_string()));
        }
        if !has("user-agent") {
            request_headers.push((
                "User-Agent".to_string(),
                concat!("aura/", env!("CARGO_PKG_VERSION")).to_string(),
            ));
        }
        if !has("accept") {
            request_headers.push(("Accept".to_string(), "*/*".to_string()));
        }
        if !has("connection") && options.keep_alive {
            request_headers.push(("Connection".to_string(), "keep-alive".to_string()));
        }
        let asked_to_close = !options.keep_alive || header_has_token(&headers, "connection", "close");
        let request = build_http_request_bytes(&method, &url, body, request_headers)?;

        let pooled = if options.keep_alive {
            pool_take(&key)
        } else {
            None
        };
        let mut connection = match pooled {
            Some(connection) => connection,
            None => connect(&key, deadline, cancellation)?,
        };
        let head = match exchange(&mut connection, &request, deadline, cancellation) {
            Ok(head) => head,
            Err((error, wrote_request)) => {
                // A kept connection the server has since closed fails
                // before any of the response arrives. Send the request
                // again on a new connection when that cannot repeat an
                // effect on the server.
                let nothing_arrived = connection.buffer.is_empty();
                let timed_out_or_cancelled = matches!(
                    error.kind(),
                    io::ErrorKind::TimedOut | io::ErrorKind::Interrupted
                );
                let retry = connection.reused
                    && nothing_arrived
                    && !timed_out_or_cancelled
                    && (!wrote_request || is_idempotent(&method));
                if !retry {
                    return Err(error);
                }
                connection = connect(&key, deadline, cancellation)?;
                exchange(&mut connection, &request, deadline, cancellation)
                    .map_err(|(error, _)| error)?
            }
        };

        let mut response = begin_body(
            connection,
            head,
            &method,
            url.as_str(),
            asked_for_compression,
            asked_to_close,
            options.read_timeout,
        )?;

        let location = if is_redirect(response.status) && options.max_redirects > 0 {
            header_value(&response.headers, "location").map(str::to_string)
        } else {
            None
        };
        let Some(location) = location else {
            return Ok(response);
        };
        if redirects >= options.max_redirects {
            return Err(io::Error::other(format!(
                "stopped after {} redirects",
                options.max_redirects
            )));
        }
        redirects += 1;
        let status = response.status;
        response.drain_for_reuse(deadline, cancellation);
        let next = url.join(&location).map_err(|error| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("invalid redirect location `{location}`: {error}"),
            )
        })?;
        let (next, next_key) = parse_request_url(next.as_str()).map_err(|_| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("redirect to an unsupported URL `{next}`"),
            )
        })?;
        let same_origin =
            next_key.tls == key.tls && next_key.host == key.host && next_key.port == key.port;
        if !same_origin {
            // Credentials for one server do not follow a redirect to
            // another.
            headers.retain(|(name, _)| {
                !["authorization", "cookie", "proxy-authorization", "host"]
                    .iter()
                    .any(|secret| name.eq_ignore_ascii_case(secret))
            });
        }
        let becomes_get = status == 303 && method != "HEAD"
            || (matches!(status, 301 | 302) && method == "POST");
        if becomes_get {
            method = "GET".to_string();
            body = &[];
            headers.retain(|(name, _)| {
                !["content-type", "content-length", "content-encoding", "transfer-encoding"]
                    .iter()
                    .any(|content| name.eq_ignore_ascii_case(content))
            });
        }
        url = next;
        key = next_key;
    }
}

/// The body of an `http.Response`: the value of the private `http.Stream`
/// type. Copies share one body.
#[derive(Clone)]
pub struct HttpResponseValue(Arc<Mutex<Option<Box<HttpBody>>>>);

impl PartialEq for HttpResponseValue {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

impl std::fmt::Debug for HttpResponseValue {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("<http-stream>")
    }
}

impl HttpResponseValue {
    pub fn new(body: HttpBody) -> Self {
        Self(Arc::new(Mutex::new(Some(Box::new(body)))))
    }

    /// Runs `operation` on the body. The body leaves its lock while the
    /// operation waits on the network, so a second use at the same time
    /// finds it closed instead of blocking a worker thread.
    pub fn with_body<T>(
        &self,
        operation: impl FnOnce(&mut HttpBody) -> io::Result<T>,
    ) -> io::Result<T> {
        let taken = lock_mutex(&self.0).take();
        let Some(mut body) = taken else {
            return Err(closed_resource_error());
        };
        let result = operation(&mut body);
        *lock_mutex(&self.0) = Some(body);
        result
    }

    pub fn close(&self) {
        let taken = lock_mutex(&self.0).take();
        if let Some(mut body) = taken {
            body.close();
            *lock_mutex(&self.0) = Some(body);
        }
    }
}

/// A whole response read into memory, for tests that drive Aura's HTTP
/// server with one request per connection.
#[cfg(any(test, feature = "test-support"))]
#[derive(Clone, Debug, PartialEq)]
pub struct HttpSnapshot {
    status: i32,
    reason: String,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

#[cfg(any(test, feature = "test-support"))]
impl HttpSnapshot {
    pub fn request_text(
        method: &str,
        url: &str,
        body: &str,
        headers: Vec<(String, String)>,
        timeout: Option<StdDuration>,
        cancellation: Option<&CancellationContext>,
    ) -> io::Result<Self> {
        Self::request_bytes(method, url, body.as_bytes(), headers, timeout, cancellation)
    }

    pub fn request_text_with_ca(
        method: &str,
        url: &str,
        body: &str,
        headers: Vec<(String, String)>,
        timeout: Option<StdDuration>,
        cancellation: Option<&CancellationContext>,
        ca_pem_path: &str,
    ) -> io::Result<Self> {
        Self::request(
            method,
            url,
            body.as_bytes(),
            headers,
            timeout,
            cancellation,
            ca_pem_path,
        )
    }

    pub fn request_bytes(
        method: &str,
        url: &str,
        body: &[u8],
        headers: Vec<(String, String)>,
        timeout: Option<StdDuration>,
        cancellation: Option<&CancellationContext>,
    ) -> io::Result<Self> {
        Self::request(method, url, body, headers, timeout, cancellation, "")
    }

    fn request(
        method: &str,
        url: &str,
        body: &[u8],
        headers: Vec<(String, String)>,
        timeout: Option<StdDuration>,
        cancellation: Option<&CancellationContext>,
        ca_pem_path: &str,
    ) -> io::Result<Self> {
        let options = HttpRequestOptions {
            timeout,
            read_timeout: timeout,
            max_redirects: 0,
            decompress: false,
            keep_alive: false,
            ca_pem_path: ca_pem_path.to_string(),
        };
        let mut response = http_send(method, url, headers, body, &options, cancellation)?;
        let body = response.read_all(MAX_HTTP_MESSAGE_BYTES as i64, cancellation)?;
        Ok(Self {
            status: response.status,
            reason: response.reason.clone(),
            headers: response.headers.clone(),
            body,
        })
    }

    /// Reads the whole response a peer writes to `stream`, at most `limit`
    /// body bytes, with the reader every request uses.
    #[cfg(test)]
    pub(super) fn read_from(
        stream: &StdTcpStream,
        deadline: Option<Instant>,
        cancellation: Option<&CancellationContext>,
        limit: usize,
    ) -> io::Result<Self> {
        let mut connection = Connection {
            transport: Transport::Plain(stream.try_clone()?),
            key: PoolKey {
                tls: false,
                host: String::new(),
                port: 0,
                ca_pem_path: String::new(),
            },
            buffer: Vec::new(),
            start: 0,
            reused: false,
        };
        let head = exchange(&mut connection, b"", deadline, cancellation)
            .map_err(|(error, _)| error)?;
        let timeout = deadline.map(|deadline| deadline.saturating_duration_since(Instant::now()));
        let mut response = begin_body(connection, head, "GET", "", false, true, timeout)?;
        let body = response.read_all(limit as i64, cancellation)?;
        Ok(Self {
            status: response.status,
            reason: response.reason.clone(),
            headers: response.headers.clone(),
            body,
        })
    }

    pub fn status(&self) -> i32 {
        self.status
    }

    pub fn reason(&self) -> String {
        self.reason.clone()
    }

    pub fn headers(&self) -> Vec<(String, String)> {
        self.headers.clone()
    }

    pub fn text(&self) -> io::Result<String> {
        io_decode_utf8(&self.body)
    }

    pub fn bytes(&self) -> Vec<u8> {
        self.body.clone()
    }
}

fn http_argument_error(name: &str, message: &str) -> Diagnostic {
    Diagnostic::coded("AU4001", format!("`{}` {message}", name.replace("::", ".")))
}

fn stream_arg<'a>(args: &'a [Value], index: usize, name: &str) -> Result<&'a HttpResponseValue> {
    match &args[index] {
        Value::HttpResponse(stream) => Ok(stream),
        _ => Err(http_argument_error(name, "expects an `http.Stream`")),
    }
}

fn int_arg(args: &[Value], index: usize, name: &str) -> Result<i64> {
    match &args[index] {
        Value::Int(value) => value.as_i128().and_then(|value| i64::try_from(value).ok()),
        _ => None,
    }
    .ok_or_else(|| http_argument_error(name, "expects an `int64`"))
}

/// A timeout argument: `None` when its flag is clear. A negative timeout
/// is an `InvalidInput` error of the request, as it is for sockets.
fn timeout_arg(
    args: &[Value],
    index: usize,
    present: bool,
    name: &str,
) -> Result<io::Result<Option<StdDuration>>> {
    let Value::Duration(nanos) = &args[index] else {
        return Err(http_argument_error(name, "expects a `Duration`"));
    };
    if !present {
        return Ok(Ok(None));
    }
    Ok(match u64::try_from(*nanos) {
        Ok(nanos) => Ok(Some(StdDuration::from_nanos(nanos))),
        Err(_) => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "timeout must not be negative",
        )),
    })
}

fn str_list_value(items: impl IntoIterator<Item = String>) -> Value {
    Value::Vec(VecValue {
        element_type: SharedType::named("str"),
        elements: items.into_iter().map(Value::string).collect(),
        str_view: Default::default(),
    })
}

fn bytes_list_value(bytes: Vec<u8>) -> Value {
    Value::Vec(VecValue {
        element_type: SharedType::named("uint8"),
        elements: ListElements::from_uint8(bytes),
        str_view: Default::default(),
    })
}

fn io_result(result: io::Result<Value>) -> Value {
    match result {
        Ok(value) => result_ok(value),
        Err(error) => result_err(io_error(error)),
    }
}

fn int_value(value: i64) -> Value {
    Value::Int(IntegerValue::from_i64(value))
}

/// `http::__aura_send`.
pub fn host_http_send(args: Vec<Value>, _program_args: Option<&[String]>) -> Result<Value> {
    let name = "http::__aura_send";
    host_expect_arity(name, &args, 10)?;
    let method = host_string_ref_arg(&args, 0, name)?;
    let url = host_string_ref_arg(&args, 1, name)?;
    let Value::Map(map) = &args[2] else {
        return Err(http_argument_error(name, "expects headers as a `dict[str, str]`"));
    };
    let mut headers = Vec::with_capacity(map.entries.len());
    for (key, value) in map.entries.iter() {
        let (Value::String(key), Value::String(value)) = (key, value) else {
            return Err(http_argument_error(name, "expects headers as a `dict[str, str]`"));
        };
        headers.push((key.to_string(), value.to_string()));
    }
    let text = host_string_ref_arg(&args, 3, name)?;
    let bytes = host_builtins::host_bytes_arg(&args, 4, name)?;
    let body: &[u8] = if bytes.is_empty() {
        text.as_bytes()
    } else {
        &bytes
    };
    let max_redirects = int_arg(&args, 7, name)?;
    if !(0..=MAX_REDIRECT_LIMIT).contains(&max_redirects) {
        return Err(http_argument_error(
            name,
            &format!("expects a redirect limit from 0 to {MAX_REDIRECT_LIMIT}"),
        ));
    }
    let flags = int_arg(&args, 8, name)?;
    let timeout = timeout_arg(&args, 5, flags & 4 != 0, name)?;
    let read_timeout = timeout_arg(&args, 6, flags & 8 != 0, name)?;
    let ca_pem_path = host_string_ref_arg(&args, 9, name)?.to_string();
    let cancellation = current_lightweight_task_cancellation();
    let response = timeout.and_then(|timeout| {
        let options = HttpRequestOptions {
            timeout,
            read_timeout: read_timeout?,
            max_redirects: max_redirects as usize,
            decompress: flags & 1 != 0,
            keep_alive: flags & 2 != 0,
            ca_pem_path,
        };
        http_send(method, url, headers, body, &options, cancellation.as_ref())
    });
    Ok(io_result(response.map(|body| {
        let status = int_value(i64::from(body.status()));
        let reason = Value::string(body.reason());
        let fields = str_list_value(
            body.headers()
                .iter()
                .flat_map(|(name, value)| [name.clone(), value.clone()]),
        );
        let url = Value::string(body.url());
        let list_of_str = Type::Named("list".to_string(), vec![Type::named("str")]);
        Value::Tuple(TupleValue {
            element_types: vec![
                Type::named("http.Stream"),
                Type::named("int64"),
                Type::named("str"),
                list_of_str,
                Type::named("str"),
            ],
            elements: vec![
                Value::HttpResponse(HttpResponseValue::new(body)),
                status,
                reason,
                fields,
                url,
            ],
        })
    })))
}

/// `http::__aura_read_chunk`.
pub fn host_http_read_chunk(args: Vec<Value>, _program_args: Option<&[String]>) -> Result<Value> {
    let name = "http::__aura_read_chunk";
    host_expect_arity(name, &args, 2)?;
    let max_bytes = int_arg(&args, 1, name)?;
    let cancellation = current_lightweight_task_cancellation();
    let chunk = stream_arg(&args, 0, name)?
        .with_body(|body| body.read_chunk(max_bytes, cancellation.as_ref()));
    Ok(io_result(chunk.map(|chunk| {
        optional_bytes_value(chunk.map(bytes_list_value))
    })))
}

/// `http::__aura_read_line`.
pub fn host_http_read_line(args: Vec<Value>, _program_args: Option<&[String]>) -> Result<Value> {
    let name = "http::__aura_read_line";
    host_expect_arity(name, &args, 1)?;
    let cancellation = current_lightweight_task_cancellation();
    let line =
        stream_arg(&args, 0, name)?.with_body(|body| body.read_line(cancellation.as_ref()));
    Ok(io_result(line.map(optional_str_value)))
}

/// `http::__aura_read_lines`.
pub fn host_http_read_lines(args: Vec<Value>, _program_args: Option<&[String]>) -> Result<Value> {
    let name = "http::__aura_read_lines";
    host_expect_arity(name, &args, 2)?;
    let max_lines = int_arg(&args, 1, name)?;
    let cancellation = current_lightweight_task_cancellation();
    let lines = stream_arg(&args, 0, name)?
        .with_body(|body| body.read_lines(max_lines, cancellation.as_ref()));
    Ok(io_result(lines.map(str_list_value)))
}

/// `http::__aura_read_event`: `(found, event, data, id, retry_ms)`.
pub fn host_http_read_event(args: Vec<Value>, _program_args: Option<&[String]>) -> Result<Value> {
    let name = "http::__aura_read_event";
    host_expect_arity(name, &args, 1)?;
    let cancellation = current_lightweight_task_cancellation();
    let event =
        stream_arg(&args, 0, name)?.with_body(|body| body.read_event(cancellation.as_ref()));
    Ok(io_result(event.map(|event| {
        let found = event.is_some();
        let event = event.unwrap_or_default();
        Value::Tuple(TupleValue {
            element_types: vec![
                Type::named("bool"),
                Type::named("str"),
                Type::named("str"),
                Type::named("str"),
                Type::named("int64"),
            ],
            elements: vec![
                Value::Bool(found),
                Value::string(event.event),
                Value::string(event.data),
                Value::string(event.id),
                int_value(if found { event.retry_ms } else { -1 }),
            ],
        })
    })))
}

/// `http::__aura_read_all`.
pub fn host_http_read_all(args: Vec<Value>, _program_args: Option<&[String]>) -> Result<Value> {
    let name = "http::__aura_read_all";
    host_expect_arity(name, &args, 2)?;
    let max_bytes = int_arg(&args, 1, name)?;
    let cancellation = current_lightweight_task_cancellation();
    let bytes = stream_arg(&args, 0, name)?
        .with_body(|body| body.read_all(max_bytes, cancellation.as_ref()));
    Ok(io_result(bytes.map(bytes_list_value)))
}

/// `http::__aura_read_text`.
pub fn host_http_read_text(args: Vec<Value>, _program_args: Option<&[String]>) -> Result<Value> {
    let name = "http::__aura_read_text";
    host_expect_arity(name, &args, 2)?;
    let max_bytes = int_arg(&args, 1, name)?;
    let cancellation = current_lightweight_task_cancellation();
    let text = stream_arg(&args, 0, name)?.with_body(|body| {
        let bytes = body.read_all(max_bytes, cancellation.as_ref())?;
        String::from_utf8(bytes).map_err(|_| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "the HTTP response body is not valid UTF-8",
            )
        })
    });
    Ok(io_result(text.map(Value::string)))
}

/// `http::__aura_close`.
pub fn host_http_close(args: Vec<Value>, _program_args: Option<&[String]>) -> Result<Value> {
    let name = "http::__aura_close";
    host_expect_arity(name, &args, 1)?;
    stream_arg(&args, 0, name)?.close();
    Ok(Value::Unit)
}

/// `http::__aura_idle_connections`.
pub fn host_http_idle_connections(
    args: Vec<Value>,
    _program_args: Option<&[String]>,
) -> Result<Value> {
    host_expect_arity("http::__aura_idle_connections", &args, 0)?;
    Ok(int_value(http_pool_idle_connections() as i64))
}

/// `http::__aura_close_idle`.
pub fn host_http_close_idle(args: Vec<Value>, _program_args: Option<&[String]>) -> Result<Value> {
    host_expect_arity("http::__aura_close_idle", &args, 0)?;
    http_pool_clear();
    Ok(Value::Unit)
}

/// The interpreter's entry for an `http` host builtin.
pub fn http_host_function(name: &str) -> Option<host_builtins::HostBuiltinFn> {
    Some(match name {
        "http::__aura_send" => host_http_send,
        "http::__aura_read_chunk" => host_http_read_chunk,
        "http::__aura_read_line" => host_http_read_line,
        "http::__aura_read_lines" => host_http_read_lines,
        "http::__aura_read_event" => host_http_read_event,
        "http::__aura_read_all" => host_http_read_all,
        "http::__aura_read_text" => host_http_read_text,
        "http::__aura_close" => host_http_close,
        "http::__aura_idle_connections" => host_http_idle_connections,
        "http::__aura_close_idle" => host_http_close_idle,
        _ => return None,
    })
}

#[cfg(test)]
#[path = "http_client_tests.rs"]
mod tests;
