use std::fs;
use std::io::{self, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use serde::Serialize;
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

use super::TeleportStatus;
use super::apply::{ApplyOptions, ApplyResponse, apply_bundle};
use super::bundle::{bundle_sha256, cache_dir};
use super::error::TeleportFailure;
use super::pack::{PackOptions, PackSessionSummary, pack_session};
use super::receive::ReceiveStagedSummary;
use crate::messages::service::QueryService;
use crate::types::SourceFilter;

const ENV_TELEPORT_BIND: &str = "MMR_TELEPORT_BIND";
const SHA256_HEADER: &str = "X-MMR-Bundle-Sha256";
const TOKEN_BYTES: usize = 32;

#[derive(Debug, Clone)]
pub struct ServeOptions {
    pub session_id: Option<String>,
    pub project: Option<String>,
    pub source_filter: Option<SourceFilter>,
    pub bind: Option<String>,
    pub timeout_secs: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct ServeStartupResponse {
    pub command: &'static str,
    pub status: TeleportStatus,
    pub transport: &'static str,
    pub listen_url: String,
    pub token: String,
    pub bundle_id: String,
    pub sha256: String,
    pub bytes: u64,
    pub expires_at: String,
    pub bind_addr: String,
    pub session: PackSessionSummary,
    pub dry_run: bool,
}

#[derive(Debug, Clone)]
pub struct HttpReceiveTarget {
    pub host: String,
    pub port: u16,
    pub token: String,
}

#[derive(Debug)]
pub enum ServeError {
    BeforeStartup(TeleportFailure),
    TimedOut,
}

pub fn is_http_locator(value: &str) -> bool {
    let trimmed = value.trim();
    trimmed.starts_with("mmtp://") || trimmed.starts_with("http://")
}

pub fn parse_http_locator(value: &str) -> Result<HttpReceiveTarget, TeleportFailure> {
    let trimmed = value.trim();
    let rest = trimmed
        .strip_prefix("mmtp://")
        .or_else(|| trimmed.strip_prefix("http://"))
        .ok_or_else(|| {
            TeleportFailure::usage(
                "teleport/receive",
                format!("unsupported HTTP locator {trimmed:?}; expected mmtp://host:port/token"),
            )
        })?;
    let slash = rest.rfind('/').ok_or_else(|| {
        TeleportFailure::usage(
            "teleport/receive",
            format!("HTTP locator must include /token path; got {trimmed:?}"),
        )
    })?;
    let token = rest[slash + 1..].trim();
    if token.is_empty() {
        return Err(TeleportFailure::usage(
            "teleport/receive",
            "HTTP locator token must not be empty",
        ));
    }
    let host_port = &rest[..slash];
    let (host, port_str) = host_port.rsplit_once(':').ok_or_else(|| {
        TeleportFailure::usage(
            "teleport/receive",
            format!("HTTP locator must include host:port; got {trimmed:?}"),
        )
    })?;
    if host.is_empty() {
        return Err(TeleportFailure::usage(
            "teleport/receive",
            "HTTP locator host must not be empty",
        ));
    }
    let port = port_str.parse::<u16>().map_err(|_| {
        TeleportFailure::usage(
            "teleport/receive",
            format!("invalid HTTP locator port {port_str:?}"),
        )
    })?;
    Ok(HttpReceiveTarget {
        host: host.to_string(),
        port,
        token: token.to_string(),
    })
}

pub fn generate_token() -> Result<String, TeleportFailure> {
    let mut bytes = [0u8; TOKEN_BYTES];
    read_urandom(&mut bytes)?;
    Ok(bytes.iter().map(|byte| format!("{byte:02x}")).collect())
}

pub fn serve_session(service: &QueryService, options: ServeOptions) -> Result<(), ServeError> {
    let pack = pack_session(
        service,
        PackOptions {
            session_id: options.session_id,
            project: options.project,
            source_filter: options.source_filter,
            output_path: None,
            fidelity: super::manifest::TeleportFidelity::Native,
            dry_run: false,
        },
    )
    .map_err(|failure| ServeError::BeforeStartup(map_pack_failure_for_serve(failure)))?;

    let bundle_path = Path::new(pack.bundle_path.as_ref().ok_or_else(|| {
        ServeError::BeforeStartup(TeleportFailure::runtime(
            "share/session",
            "pack did not produce a bundle path",
        ))
    })?);
    let sha256 = pack.sha256.clone().ok_or_else(|| {
        ServeError::BeforeStartup(TeleportFailure::runtime(
            "share/session",
            "pack did not produce bundle sha256",
        ))
    })?;
    let bytes = pack.bytes.ok_or_else(|| {
        ServeError::BeforeStartup(TeleportFailure::runtime(
            "share/session",
            "pack did not produce bundle bytes",
        ))
    })?;
    let bundle_body = fs::read(bundle_path).map_err(|error| {
        ServeError::BeforeStartup(TeleportFailure::runtime(
            "share/session",
            format!("read packed bundle {}: {error}", bundle_path.display()),
        ))
    })?;

    let token = generate_token().map_err(ServeError::BeforeStartup)?;
    let bind_spec =
        resolve_bind_spec(options.bind.as_deref()).map_err(ServeError::BeforeStartup)?;
    let listener = bind_listener(&bind_spec).map_err(ServeError::BeforeStartup)?;
    let bound_addr = listener.local_addr().map_err(|error| {
        ServeError::BeforeStartup(TeleportFailure::runtime(
            "share/session",
            format!("read bound address: {error}"),
        ))
    })?;
    let advertised_host = advertised_host(&bind_spec, bound_addr);
    let listen_url = format!("mmtp://{advertised_host}:{}/{}", bound_addr.port(), token);
    let expires_at = (OffsetDateTime::now_utc()
        + time::Duration::seconds(options.timeout_secs as i64))
    .format(&Rfc3339)
    .map_err(|error| {
        ServeError::BeforeStartup(TeleportFailure::runtime(
            "share/session",
            format!("format expires_at: {error}"),
        ))
    })?;

    let startup = ServeStartupResponse {
        command: "share/session",
        status: TeleportStatus::Ok,
        transport: "http",
        listen_url,
        token: token.clone(),
        bundle_id: pack.bundle_id.clone(),
        sha256: sha256.clone(),
        bytes,
        expires_at,
        bind_addr: bound_addr.to_string(),
        session: pack.session.clone(),
        dry_run: false,
    };
    let startup_json = serde_json::to_string(&startup).map_err(|error| {
        ServeError::BeforeStartup(TeleportFailure::runtime(
            "share/session",
            format!("serialize startup JSON: {error}"),
        ))
    })?;
    let mut stdout = io::stdout();
    stdout
        .write_all(startup_json.as_bytes())
        .and_then(|_| stdout.write_all(b"\n"))
        .and_then(|_| stdout.flush())
        .map_err(|error| {
            ServeError::BeforeStartup(TeleportFailure::runtime(
                "share/session",
                format!("write startup JSON: {error}"),
            ))
        })?;

    eprintln!("share: native transfer may contain secrets/private paths");

    listener.set_nonblocking(true).map_err(|error| {
        ServeError::BeforeStartup(TeleportFailure::runtime(
            "share/session",
            format!("set listener nonblocking: {error}"),
        ))
    })?;

    let deadline = Instant::now() + Duration::from_secs(options.timeout_secs);
    let mut consumed = false;
    while Instant::now() < deadline {
        match listener.accept() {
            Ok((mut stream, _)) => {
                // Accepted sockets inherit the listener's non-blocking mode on macOS/BSD.
                // Large bundle bodies need blocking writes or write_all fails with EAGAIN.
                stream.set_nonblocking(false).map_err(|error| {
                    ServeError::BeforeStartup(TeleportFailure::runtime(
                        "share/session",
                        format!("set accepted stream blocking: {error}"),
                    ))
                })?;
                if handle_serve_connection(
                    &mut stream,
                    &token,
                    &sha256,
                    &bundle_body,
                    &mut consumed,
                )
                .map_err(ServeError::BeforeStartup)?
                {
                    return Ok(());
                }
            }
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(10));
            }
            Err(error) => {
                return Err(ServeError::BeforeStartup(TeleportFailure::runtime(
                    "share/session",
                    format!("accept connection: {error}"),
                )));
            }
        }
    }

    if consumed {
        Ok(())
    } else {
        Err(ServeError::TimedOut)
    }
}

pub fn fetch_and_cache_http_bundle(
    target: &HttpReceiveTarget,
    dry_run: bool,
    command: &'static str,
) -> Result<Vec<ReceiveStagedSummary>, TeleportFailure> {
    let (body, header_sha256) = fetch_bundle_http(target, command)?;
    if dry_run {
        return Ok(vec![ReceiveStagedSummary {
            bundle_id: String::new(),
            inbox_path: String::new(),
            bundle_path: String::new(),
            sha256: header_sha256,
        }]);
    }

    let download_dir = cache_dir("http-downloads")
        .map_err(|error| TeleportFailure::runtime(command, error.to_string()))?;
    fs::create_dir_all(&download_dir).map_err(|error| {
        TeleportFailure::runtime(
            command,
            format!(
                "create HTTP download cache {}: {error}",
                download_dir.display()
            ),
        )
    })?;
    let partial_path = download_dir.join(format!("{}.partial", target.token));
    fs::write(&partial_path, &body).map_err(|error| {
        TeleportFailure::runtime(
            command,
            format!(
                "write downloaded bundle {}: {error}",
                partial_path.display()
            ),
        )
    })?;
    let actual_sha256 = bundle_sha256(&partial_path)
        .map_err(|error| TeleportFailure::runtime(command, error.to_string()))?;
    if actual_sha256 != header_sha256 {
        let _ = fs::remove_file(&partial_path);
        return Err(TeleportFailure::runtime(
            command,
            format!(
                "bundle hash mismatch: header expected {}, computed {}",
                header_sha256, actual_sha256
            ),
        )
        .with_error_kind("bundle_hash_mismatch"));
    }

    let bundle = super::bundle::load_bundle(&partial_path).map_err(|error| {
        TeleportFailure::runtime(command, error.to_string()).with_error_kind("bundle_corrupt")
    })?;
    let bundle_id = bundle.manifest.bundle_id.clone();
    let final_dir = cache_dir(&bundle_id)
        .map_err(|error| TeleportFailure::runtime(command, error.to_string()))?;
    fs::create_dir_all(&final_dir).map_err(|error| {
        TeleportFailure::runtime(
            command,
            format!("create bundle cache {}: {error}", final_dir.display()),
        )
    })?;
    let bundle_path = final_dir.join(super::file::BUNDLE_FILENAME);
    fs::rename(&partial_path, &bundle_path).map_err(|error| {
        TeleportFailure::runtime(
            command,
            format!(
                "move downloaded bundle to {}: {error}",
                bundle_path.display()
            ),
        )
    })?;

    Ok(vec![ReceiveStagedSummary {
        bundle_id,
        inbox_path: final_dir.display().to_string(),
        bundle_path: bundle_path.display().to_string(),
        sha256: actual_sha256,
    }])
}

pub fn receive_http_bundle(
    target: &HttpReceiveTarget,
    dry_run: bool,
    project: Option<String>,
    force: bool,
) -> Result<(Vec<ReceiveStagedSummary>, Option<ApplyResponse>), TeleportFailure> {
    let staged = fetch_and_cache_http_bundle(target, dry_run, "teleport/receive")?;
    if dry_run {
        return Ok((staged, None));
    }

    let cached = staged.first().ok_or_else(|| {
        TeleportFailure::runtime("teleport/receive", "HTTP receive produced no staged bundle")
    })?;
    let bundle_path = PathBuf::from(&cached.bundle_path);
    let apply = apply_bundle(ApplyOptions {
        bundle_path,
        project,
        dry_run: false,
        force,
        skip_store_import: true,
    })
    .map_err(map_apply_failure)?;
    Ok((staged, Some(apply)))
}

fn fetch_bundle_http(
    target: &HttpReceiveTarget,
    command: &'static str,
) -> Result<(Vec<u8>, String), TeleportFailure> {
    let addr = format!("{}:{}", target.host, target.port);
    let mut stream = TcpStream::connect(&addr).map_err(|error| {
        TeleportFailure::runtime(command, format!("connect to {addr}: {error}"))
            .with_error_kind("http_connect_failed")
    })?;
    let request = format!(
        "GET /{} HTTP/1.1\r\nHost: {}\r\nConnection: close\r\n\r\n",
        target.token, addr
    );
    stream
        .write_all(request.as_bytes())
        .and_then(|_| stream.flush())
        .map_err(|error| {
            TeleportFailure::runtime(command, format!("write HTTP request to {addr}: {error}"))
                .with_error_kind("http_transfer")
        })?;

    let mut raw = Vec::new();
    stream.read_to_end(&mut raw).map_err(|error| {
        TeleportFailure::runtime(command, format!("read HTTP response from {addr}: {error}"))
            .with_error_kind("http_transfer")
    })?;

    parse_http_bundle_response(&raw, command)
}

fn parse_http_bundle_response(
    raw: &[u8],
    command: &'static str,
) -> Result<(Vec<u8>, String), TeleportFailure> {
    if raw.is_empty() {
        return Err(TeleportFailure::runtime(command, "HTTP response was empty")
            .with_error_kind("http_transfer"));
    }
    let (header_block, header_end) = split_http_header(raw).ok_or_else(|| {
        TeleportFailure::runtime(command, "HTTP response missing header terminator")
            .with_error_kind("http_transfer")
    })?;
    let header_text = std::str::from_utf8(header_block).map_err(|error| {
        TeleportFailure::runtime(
            command,
            format!("HTTP response headers are not valid UTF-8: {error}"),
        )
        .with_error_kind("http_transfer")
    })?;
    let mut status_code = None;
    let mut header_sha256 = None;
    for line in header_text.lines() {
        if let Some(rest) = line.strip_prefix("HTTP/") {
            status_code = rest
                .split_whitespace()
                .nth(1)
                .and_then(|code| code.parse().ok());
            continue;
        }
        if let Some((name, value)) = line.split_once(':')
            && name.eq_ignore_ascii_case(SHA256_HEADER)
        {
            header_sha256 = Some(value.trim().to_string());
        }
    }
    let status_code = status_code.ok_or_else(|| {
        TeleportFailure::runtime(command, "HTTP response missing status line")
            .with_error_kind("http_transfer")
    })?;
    let body = raw[header_end..].to_vec();
    match status_code {
        200 => {
            let header_sha256 = header_sha256.ok_or_else(|| {
                TeleportFailure::runtime(
                    command,
                    format!("HTTP response missing {SHA256_HEADER} header"),
                )
                .with_error_kind("http_transfer")
            })?;
            Ok((body, header_sha256))
        }
        403 => Err(TeleportFailure::runtime(
            command,
            "HTTP bundle download rejected: invalid token",
        )
        .with_error_kind("http_invalid_token")),
        410 => Err(TeleportFailure::runtime(
            command,
            "HTTP bundle download rejected: bundle already consumed or expired",
        )
        .with_error_kind("http_bundle_consumed")),
        other => Err(TeleportFailure::runtime(
            command,
            format!("HTTP bundle download failed with status {other}"),
        )
        .with_error_kind("http_transfer")),
    }
}

fn split_http_header(raw: &[u8]) -> Option<(&[u8], usize)> {
    raw.windows(4)
        .position(|window| window == b"\r\n\r\n")
        .map(|index| (&raw[..index], index + 4))
}

fn handle_serve_connection(
    stream: &mut TcpStream,
    expected_token: &str,
    sha256: &str,
    bundle_body: &[u8],
    consumed: &mut bool,
) -> Result<bool, TeleportFailure> {
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .map_err(|error| {
            TeleportFailure::runtime("share/session", format!("set read timeout: {error}"))
        })?;
    let mut buffer = [0u8; 4096];
    let read = stream.read(&mut buffer).map_err(|error| {
        TeleportFailure::runtime("share/session", format!("read HTTP request: {error}"))
    })?;
    if read == 0 {
        return Ok(false);
    }
    let request = std::str::from_utf8(&buffer[..read]).map_err(|error| {
        TeleportFailure::runtime(
            "share/session",
            format!("HTTP request is not valid UTF-8: {error}"),
        )
    })?;
    let request_line = request.lines().next().unwrap_or_default();
    let token = parse_request_token(request_line).unwrap_or_default();
    if *consumed {
        write_http_response(stream, 410, "text/plain", b"bundle already consumed")?;
        return Ok(false);
    }
    if !constant_time_eq(token.as_bytes(), expected_token.as_bytes()) {
        write_http_response(stream, 403, "text/plain", b"invalid token")?;
        return Ok(false);
    }
    let headers = format!(
        "Content-Type: application/octet-stream\r\n{SHA256_HEADER}: {sha256}\r\nConnection: close"
    );
    write_http_response_with_headers(stream, 200, &headers, bundle_body)?;
    *consumed = true;
    Ok(true)
}

fn parse_request_token(request_line: &str) -> Option<String> {
    let mut parts = request_line.split_whitespace();
    let method = parts.next()?;
    if method != "GET" {
        return None;
    }
    let path = parts.next()?;
    let token = path.trim_start_matches('/');
    if token.is_empty() {
        None
    } else {
        Some(token.to_string())
    }
}

fn write_http_response(
    stream: &mut TcpStream,
    status: u16,
    content_type: &str,
    body: &[u8],
) -> Result<(), TeleportFailure> {
    let headers = format!("Content-Type: {content_type}\r\nConnection: close");
    write_http_response_with_headers(stream, status, &headers, body)
}

fn write_http_response_with_headers(
    stream: &mut TcpStream,
    status: u16,
    extra_headers: &str,
    body: &[u8],
) -> Result<(), TeleportFailure> {
    let status_text = match status {
        200 => "OK",
        403 => "Forbidden",
        410 => "Gone",
        _ => "Error",
    };
    let response = format!(
        "HTTP/1.1 {status} {status_text}\r\n{extra_headers}\r\nContent-Length: {}\r\n\r\n",
        body.len()
    );
    stream
        .write_all(response.as_bytes())
        .and_then(|_| stream.write_all(body))
        .and_then(|_| stream.flush())
        .map_err(|error| {
            TeleportFailure::runtime("share/session", format!("write HTTP response: {error}"))
        })
}

fn resolve_bind_spec(explicit: Option<&str>) -> Result<String, TeleportFailure> {
    if let Some(value) = explicit {
        return normalize_bind_spec(value.trim());
    }
    if let Ok(value) = std::env::var(ENV_TELEPORT_BIND) {
        let trimmed = value.trim();
        if !trimmed.is_empty() {
            return normalize_bind_spec(trimmed);
        }
    }
    if let Some(ip) = tailscale_ipv4() {
        return Ok(format!("{ip}:0"));
    }
    Ok("127.0.0.1:0".to_string())
}

fn normalize_bind_spec(value: &str) -> Result<String, TeleportFailure> {
    if value.contains(':') {
        Ok(value.to_string())
    } else {
        Ok(format!("{value}:0"))
    }
}

fn tailscale_ipv4() -> Option<String> {
    let output = Command::new("tailscale")
        .arg("ip")
        .arg("-4")
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let ip = String::from_utf8_lossy(&output.stdout)
        .lines()
        .next()?
        .trim()
        .to_string();
    if ip.is_empty() { None } else { Some(ip) }
}

fn bind_listener(bind_spec: &str) -> Result<TcpListener, TeleportFailure> {
    let addr: SocketAddr = bind_spec.parse().map_err(|_| {
        TeleportFailure::usage(
            "share/session",
            format!("invalid bind address {bind_spec:?}; expected host:port"),
        )
    })?;
    TcpListener::bind(addr).map_err(|error| {
        TeleportFailure::runtime("share/session", format!("bind {bind_spec}: {error}"))
    })
}

fn advertised_host(bind_spec: &str, bound_addr: SocketAddr) -> String {
    if let Ok(parsed) = bind_spec.parse::<SocketAddr>()
        && !parsed.ip().is_unspecified()
    {
        return parsed.ip().to_string();
    }
    match bound_addr.ip() {
        std::net::IpAddr::V4(ip) if ip.is_loopback() => "127.0.0.1".to_string(),
        other => other.to_string(),
    }
}

fn read_urandom(out: &mut [u8]) -> Result<(), TeleportFailure> {
    let mut file = fs::File::open("/dev/urandom").map_err(|error| {
        TeleportFailure::runtime("share/session", format!("open /dev/urandom: {error}"))
    })?;
    file.read_exact(out).map_err(|error| {
        TeleportFailure::runtime("share/session", format!("read /dev/urandom: {error}"))
    })
}

fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
    if left.len() != right.len() {
        return false;
    }
    let mut diff = 0u8;
    for (left_byte, right_byte) in left.iter().zip(right.iter()) {
        diff |= left_byte ^ right_byte;
    }
    diff == 0
}

fn map_pack_failure_for_serve(failure: TeleportFailure) -> TeleportFailure {
    let mut mapped = TeleportFailure::runtime("share/session", failure.message);
    mapped.exit_code = failure.exit_code;
    mapped.error_kind = failure.error_kind;
    mapped
}

fn map_apply_failure(failure: TeleportFailure) -> TeleportFailure {
    let mut mapped = TeleportFailure::runtime("teleport/receive", failure.message);
    mapped.exit_code = failure.exit_code;
    mapped.error_kind = failure.error_kind;
    mapped
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generate_token_is_64_hex_chars_and_differs_across_calls() {
        let first = generate_token().expect("first token");
        let second = generate_token().expect("second token");
        assert_eq!(first.len(), TOKEN_BYTES * 2);
        assert!(first.chars().all(|ch| ch.is_ascii_hexdigit()));
        assert_ne!(first, second);
    }

    #[test]
    fn parse_http_locator_accepts_mmtp_and_http_schemes() {
        let mmtp = parse_http_locator("mmtp://127.0.0.1:8765/abc123").expect("mmtp");
        assert_eq!(mmtp.host, "127.0.0.1");
        assert_eq!(mmtp.port, 8765);
        assert_eq!(mmtp.token, "abc123");

        let http = parse_http_locator("http://100.64.0.2:9000/deadbeef").expect("http");
        assert_eq!(http.host, "100.64.0.2");
        assert_eq!(http.port, 9000);
        assert_eq!(http.token, "deadbeef");
    }

    #[test]
    fn constant_time_eq_matches_only_identical_values() {
        assert!(constant_time_eq(b"abc", b"abc"));
        assert!(!constant_time_eq(b"abc", b"abd"));
        assert!(!constant_time_eq(b"abc", b"ab"));
    }

    #[test]
    fn serve_writes_large_bundle_over_nonblocking_listener() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        listener
            .set_nonblocking(true)
            .expect("set listener nonblocking");
        let addr = listener.local_addr().expect("local addr");
        let token = "abc123";
        let sha256 = "sha256:deadbeef";
        let body = vec![b'x'; 512 * 1024];

        let client = std::thread::spawn(move || {
            let mut stream = TcpStream::connect(addr).expect("connect");
            let request = format!("GET /{token} HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n");
            stream.write_all(request.as_bytes()).expect("write request");
            stream.flush().expect("flush request");
            let mut response = Vec::new();
            stream
                .read_to_end(&mut response)
                .expect("read large response");
            response
        });

        let (mut stream, _) = loop {
            match listener.accept() {
                Ok(pair) => break pair,
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(10));
                }
                Err(error) => panic!("accept: {error}"),
            }
        };
        stream.set_nonblocking(false).expect("set stream blocking");
        let mut consumed = false;
        handle_serve_connection(&mut stream, token, sha256, &body, &mut consumed)
            .expect("serve large bundle");
        assert!(consumed);
        drop(stream);

        let response = client.join().expect("client thread");
        let (header_block, header_end) = split_http_header(&response).expect("response headers");
        let header_text = std::str::from_utf8(header_block).expect("header utf8");
        assert!(header_text.contains("HTTP/1.1 200"));
        assert_eq!(&response[header_end..], body.as_slice());
    }
}
