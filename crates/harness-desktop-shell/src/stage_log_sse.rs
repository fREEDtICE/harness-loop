use std::{
    ffi::OsStr,
    fs::{self, File},
    io::{self, BufRead, BufReader, Read, Seek, SeekFrom, Write},
    net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream},
    path::{Component, Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    thread::{self, JoinHandle},
    time::Duration,
};

use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64_STANDARD};
use loopsmith_core::paths::normalize_path;
use serde::Serialize;

const SSE_ENDPOINT: &str = "/stage-log";
const POLL_INTERVAL: Duration = Duration::from_millis(100);
const MAX_ACTIVE_CONNECTIONS: usize = 16;

/// Loopback-only SSE source for live stage stdout logs.
///
/// The server emits a `snapshot` event with the current file contents first and
/// then `append` events as new bytes land on disk.
#[derive(Debug)]
pub struct StageLogSseServer {
    inner: Arc<StageLogSseServerInner>,
    accept_thread: Mutex<Option<JoinHandle<()>>>,
}

#[derive(Debug)]
struct StageLogSseServerInner {
    base_url: String,
    listener_addr: SocketAddr,
    shutdown: AtomicBool,
    active_connections: AtomicUsize,
}

#[derive(Debug)]
enum StreamEvent {
    Snapshot { bytes: Vec<u8> },
    Append { byte_offset: u64, bytes: Vec<u8> },
}

#[derive(Debug)]
struct StageLogCursor {
    path: PathBuf,
    byte_offset: u64,
    needs_snapshot: bool,
}

#[derive(Debug)]
struct HttpError {
    status: &'static str,
    message: String,
}

#[derive(Debug, Serialize)]
struct StageLogChunk {
    byte_offset: u64,
    content_b64: String,
}

impl StageLogSseServer {
    /// Starts the local stage-log SSE server on an ephemeral loopback port.
    pub fn bind_loopback() -> io::Result<Self> {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))?;
        listener.set_nonblocking(true)?;
        let listener_addr = listener.local_addr()?;
        let inner = Arc::new(StageLogSseServerInner {
            base_url: format!("http://{listener_addr}"),
            listener_addr,
            shutdown: AtomicBool::new(false),
            active_connections: AtomicUsize::new(0),
        });
        let accept_inner = Arc::clone(&inner);
        let accept_thread = thread::spawn(move || accept_loop(listener, accept_inner));

        Ok(Self {
            inner,
            accept_thread: Mutex::new(Some(accept_thread)),
        })
    }

    /// Resolves a full SSE URL for a live stage stdout log path.
    pub fn stream_url(&self, path: impl AsRef<Path>) -> io::Result<String> {
        let path = validate_stage_stdout_log_path(path.as_ref())?;
        let encoded_path = hex_encode(path.to_string_lossy().as_bytes());
        Ok(format!(
            "{}{SSE_ENDPOINT}?path={encoded_path}",
            self.inner.base_url
        ))
    }

    /// Returns the current number of active streaming connections.
    ///
    /// This is primarily intended for deterministic lifecycle tests.
    pub fn active_connections(&self) -> usize {
        self.inner.active_connections.load(Ordering::SeqCst)
    }
}

impl Drop for StageLogSseServer {
    fn drop(&mut self) {
        self.inner.shutdown.store(true, Ordering::SeqCst);
        let _ = TcpStream::connect(self.inner.listener_addr);
        if let Ok(mut guard) = self.accept_thread.lock() {
            if let Some(handle) = guard.take() {
                let _ = handle.join();
            }
        }
    }
}

impl StageLogCursor {
    fn new(path: PathBuf) -> Self {
        Self {
            path,
            byte_offset: 0,
            needs_snapshot: false,
        }
    }

    fn initial_snapshot(&mut self) -> io::Result<Vec<u8>> {
        let bytes = read_full_file(&self.path)?;
        self.byte_offset = bytes.len() as u64;
        Ok(bytes)
    }

    fn poll(&mut self) -> io::Result<Option<StreamEvent>> {
        match File::open(&self.path) {
            Ok(mut file) => {
                let file_len = file.metadata()?.len();
                if self.needs_snapshot || file_len < self.byte_offset {
                    let bytes = read_open_file(&mut file)?;
                    self.byte_offset = bytes.len() as u64;
                    self.needs_snapshot = false;
                    return Ok(Some(StreamEvent::Snapshot { bytes }));
                }

                if file_len == self.byte_offset {
                    return Ok(None);
                }

                let byte_offset = self.byte_offset;
                file.seek(SeekFrom::Start(byte_offset))?;
                let mut bytes = Vec::with_capacity((file_len - byte_offset) as usize);
                file.read_to_end(&mut bytes)?;

                // If the file shrank while this poll was in flight, the bytes we
                // just read may no longer line up with the prefix already sent to
                // the client. Resend a full snapshot from the current path.
                if file.metadata()?.len() < file_len {
                    let bytes = read_full_file(&self.path)?;
                    self.byte_offset = bytes.len() as u64;
                    self.needs_snapshot = false;
                    return Ok(Some(StreamEvent::Snapshot { bytes }));
                }

                self.byte_offset = byte_offset + bytes.len() as u64;
                Ok(Some(StreamEvent::Append { byte_offset, bytes }))
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                self.byte_offset = 0;
                self.needs_snapshot = true;
                Ok(None)
            }
            Err(error) => Err(error),
        }
    }
}

fn accept_loop(listener: TcpListener, inner: Arc<StageLogSseServerInner>) {
    while !inner.shutdown.load(Ordering::SeqCst) {
        match listener.accept() {
            Ok((stream, _addr)) => {
                let connection_inner = Arc::clone(&inner);
                thread::spawn(move || {
                    if let Err(error) = handle_connection(stream, connection_inner) {
                        eprintln!("warn: stage log SSE connection failed: {error}");
                    }
                });
            }
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                thread::sleep(POLL_INTERVAL);
            }
            Err(error) => {
                if inner.shutdown.load(Ordering::SeqCst) {
                    break;
                }
                eprintln!("warn: stage log SSE accept failed: {error}");
                thread::sleep(POLL_INTERVAL);
            }
        }
    }
}

fn handle_connection(
    mut stream: TcpStream,
    inner: Arc<StageLogSseServerInner>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    stream.set_nodelay(true)?;
    let response = match handle_connection_inner(&mut stream, &inner) {
        Ok(()) => return Ok(()),
        Err(error) => error,
    };
    if response.status == "499 Client Closed Request" {
        return Ok(());
    }
    write_error_response(&mut stream, &response)?;
    Ok(())
}

fn handle_connection_inner(
    stream: &mut TcpStream,
    inner: &Arc<StageLogSseServerInner>,
) -> Result<(), HttpError> {
    let request_target = read_request_target(stream).map_err(http_internal_error)?;
    let path = parse_requested_path(&request_target)?;
    let _connection_guard =
        ActiveConnectionGuard::try_new(&inner.active_connections, MAX_ACTIVE_CONNECTIONS)
            .ok_or_else(|| HttpError {
                status: "503 Service Unavailable",
                message: format!(
                    "too many active stage log streams; limit is {MAX_ACTIVE_CONNECTIONS}"
                ),
            })?;

    write_sse_headers(stream).map_err(http_internal_error)?;

    let mut cursor = StageLogCursor::new(path);
    let snapshot = cursor.initial_snapshot().map_err(http_internal_error)?;
    write_chunk_event(stream, "snapshot", 0, &snapshot).map_err(http_internal_error)?;

    loop {
        if inner.shutdown.load(Ordering::SeqCst) {
            return Ok(());
        }

        match cursor.poll().map_err(http_internal_error)? {
            Some(StreamEvent::Snapshot { bytes }) => {
                write_chunk_event(stream, "snapshot", 0, &bytes).map_err(http_internal_error)?;
            }
            Some(StreamEvent::Append { byte_offset, bytes }) => {
                write_chunk_event(stream, "append", byte_offset, &bytes)
                    .map_err(http_internal_error)?;
            }
            None => {
                write_keepalive(stream).map_err(http_internal_error)?;
            }
        }

        thread::sleep(POLL_INTERVAL);
    }
}

fn read_request_target(stream: &TcpStream) -> io::Result<String> {
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut request_line = String::new();
    let read = reader.read_line(&mut request_line)?;
    if read == 0 {
        return Err(io::Error::new(
            io::ErrorKind::UnexpectedEof,
            "missing HTTP request line",
        ));
    }

    let mut request_parts = request_line.split_whitespace();
    let method = request_parts.next().unwrap_or_default();
    if method != "GET" {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("unsupported HTTP method: {method}"),
        ));
    }

    let target = request_parts
        .next()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "missing request target"))?
        .to_string();

    let mut header_line = String::new();
    loop {
        header_line.clear();
        let read = reader.read_line(&mut header_line)?;
        if read == 0 || header_line == "\r\n" {
            break;
        }
    }

    Ok(target)
}

fn parse_requested_path(request_target: &str) -> Result<PathBuf, HttpError> {
    let (request_path, query) = request_target.split_once('?').ok_or_else(|| HttpError {
        status: "400 Bad Request",
        message: "missing path query".to_string(),
    })?;

    if request_path != SSE_ENDPOINT {
        return Err(HttpError {
            status: "404 Not Found",
            message: format!("unknown SSE endpoint: {request_path}"),
        });
    }

    let encoded_path = parse_query_value(query, "path").ok_or_else(|| HttpError {
        status: "400 Bad Request",
        message: "missing stage log path".to_string(),
    })?;
    let decoded_path = decode_hex(encoded_path).map_err(|message| HttpError {
        status: "400 Bad Request",
        message,
    })?;
    let decoded_path = String::from_utf8(decoded_path).map_err(|error| HttpError {
        status: "400 Bad Request",
        message: format!("invalid UTF-8 stage log path: {error}"),
    })?;

    validate_stage_stdout_log_path(Path::new(&decoded_path)).map_err(|error| HttpError {
        status: "400 Bad Request",
        message: error.to_string(),
    })
}

fn parse_query_value<'a>(query: &'a str, key: &str) -> Option<&'a str> {
    query.split('&').find_map(|entry| {
        let (entry_key, entry_value) = entry.split_once('=')?;
        (entry_key == key).then_some(entry_value)
    })
}

/// Normalizes and validates a stage stdout log path used by the desktop UI.
///
/// Valid stage logs must be absolute `*-stdout.log` files under a `logs`
/// directory inside a `.loopsmith-runs` artifact root.
pub fn validate_stage_stdout_log_path(path: &Path) -> io::Result<PathBuf> {
    let path = normalize_path(path.to_path_buf());
    if !path.is_absolute() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("stage log path must be absolute: {}", path.display()),
        ));
    }

    let file_name = path
        .file_name()
        .and_then(OsStr::to_str)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "missing stage log name"))?;
    if !file_name.ends_with("-stdout.log") {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("path is not a stage stdout log: {}", path.display()),
        ));
    }

    if path.parent().and_then(Path::file_name) != Some(OsStr::new("logs")) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!(
                "stage stdout log must live under a logs directory: {}",
                path.display()
            ),
        ));
    }

    let has_runs_component = path.components().any(|component| {
        matches!(
            component,
            Component::Normal(part) if part == OsStr::new(".loopsmith-runs")
        )
    });
    if !has_runs_component {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!(
                "stage stdout log must stay within a .loopsmith-runs artifact root: {}",
                path.display()
            ),
        ));
    }

    Ok(path)
}

/// Validates an arbitrary run artifact path inside the `.loopsmith-runs` tree.
///
/// Unlike [`validate_stage_stdout_log_path`], this does not enforce a
/// `-stdout.log` suffix or a `logs` parent directory, so it can be used
/// for `request.md`, JSON artifacts, and similar files.
pub fn validate_run_artifact_path(path: &Path) -> io::Result<PathBuf> {
    let path = normalize_path(path.to_path_buf());
    if !path.is_absolute() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("artifact path must be absolute: {}", path.display()),
        ));
    }

    let has_runs_component = path.components().any(|component| {
        matches!(
            component,
            Component::Normal(part) if part == OsStr::new(".loopsmith-runs")
        )
    });
    if !has_runs_component {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!(
                "artifact path must stay within a .loopsmith-runs artifact root: {}",
                path.display()
            ),
        ));
    }

    Ok(path)
}

fn read_full_file(path: &Path) -> io::Result<Vec<u8>> {
    match fs::read(path) {
        Ok(bytes) => Ok(bytes),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(error) => Err(error),
    }
}

fn read_open_file(file: &mut File) -> io::Result<Vec<u8>> {
    file.seek(SeekFrom::Start(0))?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)?;
    Ok(bytes)
}

fn write_sse_headers(stream: &mut TcpStream) -> io::Result<()> {
    stream.write_all(
        b"HTTP/1.1 200 OK\r\n\
Content-Type: text/event-stream\r\n\
Cache-Control: no-cache\r\n\
Connection: keep-alive\r\n\
Access-Control-Allow-Origin: *\r\n\
\r\n",
    )?;
    stream.flush()
}

fn write_error_response(stream: &mut TcpStream, error: &HttpError) -> io::Result<()> {
    let body = error.message.as_bytes();
    write!(
        stream,
        "HTTP/1.1 {}\r\n\
Content-Type: text/plain; charset=utf-8\r\n\
Content-Length: {}\r\n\
Connection: close\r\n\
Access-Control-Allow-Origin: *\r\n\
\r\n{}",
        error.status,
        body.len(),
        error.message
    )?;
    stream.flush()
}

fn write_chunk_event(
    stream: &mut TcpStream,
    event_name: &str,
    byte_offset: u64,
    bytes: &[u8],
) -> io::Result<()> {
    let payload = StageLogChunk {
        byte_offset,
        content_b64: BASE64_STANDARD.encode(bytes),
    };
    let payload = serde_json::to_string(&payload).expect("stage log chunk JSON");
    write!(stream, "event: {event_name}\r\ndata: {payload}\r\n\r\n")?;
    stream.flush()
}

fn write_keepalive(stream: &mut TcpStream) -> io::Result<()> {
    stream.write_all(b": keepalive\r\n\r\n")?;
    stream.flush()
}

fn hex_encode(bytes: &[u8]) -> String {
    let mut encoded = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        use std::fmt::Write as _;
        let _ = write!(&mut encoded, "{byte:02x}");
    }
    encoded
}

fn decode_hex(input: &str) -> Result<Vec<u8>, String> {
    if input.len() % 2 != 0 {
        return Err("stage log path hex must have even length".to_string());
    }

    let mut decoded = Vec::with_capacity(input.len() / 2);
    let bytes = input.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        let value = decode_hex_pair(bytes[index], bytes[index + 1]).ok_or_else(|| {
            format!(
                "invalid hex byte in stage log path at characters {}-{}",
                index,
                index + 1
            )
        })?;
        decoded.push(value);
        index += 2;
    }
    Ok(decoded)
}

fn decode_hex_pair(high: u8, low: u8) -> Option<u8> {
    Some((decode_hex_nibble(high)? << 4) | decode_hex_nibble(low)?)
}

fn decode_hex_nibble(value: u8) -> Option<u8> {
    match value {
        b'0'..=b'9' => Some(value - b'0'),
        b'a'..=b'f' => Some(value - b'a' + 10),
        b'A'..=b'F' => Some(value - b'A' + 10),
        _ => None,
    }
}

fn http_internal_error(error: io::Error) -> HttpError {
    if matches!(
        error.kind(),
        io::ErrorKind::BrokenPipe
            | io::ErrorKind::ConnectionAborted
            | io::ErrorKind::ConnectionReset
            | io::ErrorKind::UnexpectedEof
    ) {
        HttpError {
            status: "499 Client Closed Request",
            message: error.to_string(),
        }
    } else if error.kind() == io::ErrorKind::InvalidInput {
        HttpError {
            status: "400 Bad Request",
            message: error.to_string(),
        }
    } else {
        HttpError {
            status: "500 Internal Server Error",
            message: error.to_string(),
        }
    }
}

struct ActiveConnectionGuard<'a> {
    counter: &'a AtomicUsize,
}

impl<'a> ActiveConnectionGuard<'a> {
    fn try_new(counter: &'a AtomicUsize, max_connections: usize) -> Option<Self> {
        let mut current = counter.load(Ordering::SeqCst);
        loop {
            if current >= max_connections {
                return None;
            }
            match counter.compare_exchange(current, current + 1, Ordering::SeqCst, Ordering::SeqCst)
            {
                Ok(_) => return Some(Self { counter }),
                Err(observed) => current = observed,
            }
        }
    }
}

impl Drop for ActiveConnectionGuard<'_> {
    fn drop(&mut self) {
        self.counter.fetch_sub(1, Ordering::SeqCst);
    }
}

#[cfg(test)]
mod tests {
    use std::{
        io::{self, BufRead, BufReader, Write},
        net::TcpStream,
        path::Path,
        time::{Duration, Instant},
    };

    use base64::Engine as _;
    use serde::Deserialize;
    use tempfile::tempdir;

    use super::{
        BASE64_STANDARD, MAX_ACTIVE_CONNECTIONS, POLL_INTERVAL, StageLogSseServer,
        validate_stage_stdout_log_path,
    };

    #[derive(Debug, Deserialize)]
    struct StageLogChunk {
        byte_offset: u64,
        content_b64: String,
    }

    #[derive(Debug)]
    struct ReceivedEvent {
        name: String,
        chunk: StageLogChunk,
    }

    #[test]
    fn validate_stage_stdout_log_path_rejects_non_artifact_paths() {
        let temp = tempdir().expect("tempdir");
        let invalid_path = temp.path().join("notes.txt");
        std::fs::write(&invalid_path, b"secret").expect("write invalid log file");

        let error =
            validate_stage_stdout_log_path(&invalid_path).expect_err("reject invalid log path");
        assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
    }

    #[test]
    fn streams_snapshot_then_incremental_appends() {
        let temp = tempdir().expect("tempdir");
        let logs_dir = temp
            .path()
            .join(".loopsmith-runs")
            .join("run-1")
            .join("worker")
            .join("logs");
        std::fs::create_dir_all(&logs_dir).expect("logs dir");
        let stdout_log = logs_dir.join("plan-01-stdout.log");
        std::fs::write(&stdout_log, b"alpha\n").expect("write initial log");

        let server = StageLogSseServer::bind_loopback().expect("bind stage log SSE server");
        let mut connection = connect(&server.stream_url(&stdout_log).expect("stream url"));

        let snapshot = read_next_event(&mut connection);
        assert_eq!(snapshot.name, "snapshot");
        assert_eq!(snapshot.chunk.byte_offset, 0);
        assert_eq!(decode_chunk(&snapshot.chunk), b"alpha\n");

        append_to_file(&stdout_log, b"beta\n");
        let append = read_next_event(&mut connection);
        assert_eq!(append.name, "append");
        assert_eq!(append.chunk.byte_offset, 6);
        assert_eq!(decode_chunk(&append.chunk), b"beta\n");

        append_to_file(&stdout_log, b"gamma\n");
        let append = read_next_event(&mut connection);
        assert_eq!(append.name, "append");
        assert_eq!(append.chunk.byte_offset, 11);
        assert_eq!(decode_chunk(&append.chunk), b"gamma\n");
    }

    #[test]
    fn streams_empty_snapshot_for_missing_file_then_replays_created_file() {
        let temp = tempdir().expect("tempdir");
        let logs_dir = temp
            .path()
            .join(".loopsmith-runs")
            .join("run-1")
            .join("worker")
            .join("logs");
        std::fs::create_dir_all(&logs_dir).expect("logs dir");
        let stdout_log = logs_dir.join("repair-01-stdout.log");

        let server = StageLogSseServer::bind_loopback().expect("bind stage log SSE server");
        let mut connection = connect(&server.stream_url(&stdout_log).expect("stream url"));

        let snapshot = read_next_event(&mut connection);
        assert_eq!(snapshot.name, "snapshot");
        assert_eq!(snapshot.chunk.byte_offset, 0);
        assert_eq!(decode_chunk(&snapshot.chunk), b"");

        std::fs::write(&stdout_log, b"late\n").expect("write created log");
        let snapshot = read_next_event(&mut connection);
        assert_eq!(snapshot.name, "snapshot");
        assert_eq!(snapshot.chunk.byte_offset, 0);
        assert_eq!(decode_chunk(&snapshot.chunk), b"late\n");
    }

    #[test]
    fn resnapshots_after_log_truncation() {
        let temp = tempdir().expect("tempdir");
        let logs_dir = temp
            .path()
            .join(".loopsmith-runs")
            .join("run-1")
            .join("worker")
            .join("logs");
        std::fs::create_dir_all(&logs_dir).expect("logs dir");
        let stdout_log = logs_dir.join("build-01-stdout.log");
        std::fs::write(&stdout_log, b"alpha\nbeta\n").expect("write initial log");

        let server = StageLogSseServer::bind_loopback().expect("bind stage log SSE server");
        let mut connection = connect(&server.stream_url(&stdout_log).expect("stream url"));

        let snapshot = read_next_event(&mut connection);
        assert_eq!(snapshot.name, "snapshot");
        assert_eq!(decode_chunk(&snapshot.chunk), b"alpha\nbeta\n");

        std::fs::write(&stdout_log, b"reset\n").expect("truncate and rewrite log");
        let snapshot = read_next_event(&mut connection);
        assert_eq!(snapshot.name, "snapshot");
        assert_eq!(snapshot.chunk.byte_offset, 0);
        assert_eq!(decode_chunk(&snapshot.chunk), b"reset\n");
    }

    #[test]
    fn closes_disconnected_stream_before_next_subscription_reads_new_path() {
        let temp = tempdir().expect("tempdir");
        let logs_dir = temp
            .path()
            .join(".loopsmith-runs")
            .join("run-1")
            .join("worker")
            .join("logs");
        std::fs::create_dir_all(&logs_dir).expect("logs dir");
        let first_log = logs_dir.join("build-01-stdout.log");
        let second_log = logs_dir.join("evaluate-01-stdout.log");
        std::fs::write(&first_log, b"first\n").expect("write first log");
        std::fs::write(&second_log, b"second\n").expect("write second log");

        let server = StageLogSseServer::bind_loopback().expect("bind stage log SSE server");

        let mut first_connection = connect(&server.stream_url(&first_log).expect("first url"));
        let snapshot = read_next_event(&mut first_connection);
        assert_eq!(snapshot.name, "snapshot");
        assert_eq!(decode_chunk(&snapshot.chunk), b"first\n");
        drop(first_connection);

        wait_for(
            Duration::from_secs(2),
            || server.active_connections() == 0,
            "first SSE connection to close",
        );

        let mut second_connection = connect(&server.stream_url(&second_log).expect("second url"));
        let snapshot = read_next_event(&mut second_connection);
        assert_eq!(snapshot.name, "snapshot");
        assert_eq!(decode_chunk(&snapshot.chunk), b"second\n");

        append_to_file(&first_log, b"stale\n");
        append_to_file(&second_log, b"fresh\n");
        let append = read_next_event(&mut second_connection);
        assert_eq!(append.name, "append");
        assert_eq!(append.chunk.byte_offset, 7);
        assert_eq!(decode_chunk(&append.chunk), b"fresh\n");
    }

    #[test]
    fn rejects_connections_beyond_active_limit() {
        let temp = tempdir().expect("tempdir");
        let logs_dir = temp
            .path()
            .join(".loopsmith-runs")
            .join("run-1")
            .join("worker")
            .join("logs");
        std::fs::create_dir_all(&logs_dir).expect("logs dir");
        let stdout_log = logs_dir.join("plan-01-stdout.log");
        std::fs::write(&stdout_log, b"alpha\n").expect("write initial log");

        let server = StageLogSseServer::bind_loopback().expect("bind stage log SSE server");
        let url = server.stream_url(&stdout_log).expect("stream url");

        let mut connections = Vec::new();
        for _ in 0..MAX_ACTIVE_CONNECTIONS {
            let mut connection = connect(&url);
            let snapshot = read_next_event(&mut connection);
            assert_eq!(snapshot.name, "snapshot");
            connections.push(connection);
        }

        wait_for(
            Duration::from_secs(2),
            || server.active_connections() == MAX_ACTIVE_CONNECTIONS,
            "all SSE connections to register",
        );

        let rejected = connect_response(&url);
        assert_eq!(rejected.status_line, "HTTP/1.1 503 Service Unavailable");

        drop(connections);
    }

    fn append_to_file(path: &Path, bytes: &[u8]) {
        let mut file = std::fs::OpenOptions::new()
            .append(true)
            .create(true)
            .open(path)
            .expect("open file for append");
        file.write_all(bytes).expect("append file bytes");
        file.flush().expect("flush appended bytes");
    }

    struct HttpConnection {
        status_line: String,
        reader: BufReader<TcpStream>,
    }

    fn connect(url: &str) -> BufReader<TcpStream> {
        let connection = connect_response(url);
        assert_eq!(connection.status_line, "HTTP/1.1 200 OK");
        connection.reader
    }

    fn connect_response(url: &str) -> HttpConnection {
        let (addr, request_target) = parse_url(url);
        let mut stream = TcpStream::connect(addr).expect("connect SSE stream");
        stream
            .set_read_timeout(Some(Duration::from_secs(3)))
            .expect("set read timeout");
        write!(
            stream,
            "GET {request_target} HTTP/1.1\r\nHost: {addr}\r\nAccept: text/event-stream\r\nConnection: keep-alive\r\n\r\n"
        )
        .expect("write HTTP request");
        stream.flush().expect("flush HTTP request");
        let mut reader = BufReader::new(stream);
        let mut status_line = String::new();
        reader
            .read_line(&mut status_line)
            .expect("read status line");
        let mut header_line = String::new();
        loop {
            header_line.clear();
            reader
                .read_line(&mut header_line)
                .expect("read header line");
            if header_line == "\r\n" {
                break;
            }
        }
        HttpConnection {
            status_line: status_line.trim_end().to_string(),
            reader,
        }
    }

    fn parse_url(url: &str) -> (&str, &str) {
        let rest = url
            .strip_prefix("http://")
            .expect("stage log SSE URL must be HTTP");
        let slash_index = rest.find('/').expect("stage log SSE URL path");
        (&rest[..slash_index], &rest[slash_index..])
    }

    fn read_next_event(reader: &mut BufReader<TcpStream>) -> ReceivedEvent {
        loop {
            let mut event_name = None;
            let mut event_data = None;
            let mut line = String::new();
            loop {
                line.clear();
                reader.read_line(&mut line).expect("read SSE line");
                if line == "\r\n" {
                    break;
                }
                if let Some(value) = line.strip_prefix("event: ") {
                    event_name = Some(value.trim_end().to_string());
                } else if let Some(value) = line.strip_prefix("data: ") {
                    event_data = Some(value.trim_end().to_string());
                }
            }

            if let (Some(name), Some(data)) = (event_name, event_data) {
                let chunk = serde_json::from_str(&data).expect("deserialize SSE chunk");
                return ReceivedEvent { name, chunk };
            }
        }
    }

    fn decode_chunk(chunk: &StageLogChunk) -> Vec<u8> {
        BASE64_STANDARD
            .decode(&chunk.content_b64)
            .expect("decode base64 chunk")
    }

    fn wait_for(timeout: Duration, mut condition: impl FnMut() -> bool, context: &str) {
        let deadline = Instant::now() + timeout;
        while Instant::now() <= deadline {
            if condition() {
                return;
            }
            std::thread::sleep(POLL_INTERVAL);
        }
        panic!("timed out waiting for {context}");
    }
}
