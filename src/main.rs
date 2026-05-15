use std::collections::HashMap;
use std::env;
use std::fs;
use std::io::{self, BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

const DEFAULT_ADDR: &str = "127.0.0.1:8080";
const DEFAULT_LINKS_PATH: &str = "links.tsv";
const MAX_REQUEST_LINE: usize = 8192;
const MAX_HEADER_BYTES: usize = 16 * 1024;
const IO_TIMEOUT: Duration = Duration::from_secs(5);

type Links = Arc<HashMap<String, String>>;

fn main() -> io::Result<()> {
    let config = Config::from_args(env::args().skip(1))?;
    let links = Arc::new(load_links(&config.links_path)?);
    let listener = TcpListener::bind(&config.addr)?;

    eprintln!(
        "cendek listening on http://{} with {} links from {}",
        config.addr,
        links.len(),
        config.links_path
    );

    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                let links = Arc::clone(&links);
                thread::spawn(move || {
                    if let Err(err) = handle_connection(stream, links) {
                        eprintln!("connection error: {err}");
                    }
                });
            }
            Err(err) => eprintln!("accept error: {err}"),
        }
    }

    Ok(())
}

struct Config {
    addr: String,
    links_path: String,
}

impl Config {
    fn from_args<I>(args: I) -> io::Result<Self>
    where
        I: IntoIterator<Item = String>,
    {
        let mut addr = DEFAULT_ADDR.to_string();
        let mut links_path = DEFAULT_LINKS_PATH.to_string();
        let mut args = args.into_iter();

        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--addr" | "-a" => {
                    addr = next_value(&mut args, "--addr")?;
                }
                "--links" | "-l" => {
                    links_path = next_value(&mut args, "--links")?;
                }
                "--help" | "-h" => {
                    print_help();
                    std::process::exit(0);
                }
                _ => {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidInput,
                        format!("unknown argument: {arg}"),
                    ));
                }
            }
        }

        Ok(Self { addr, links_path })
    }
}

fn next_value<I>(args: &mut I, name: &str) -> io::Result<String>
where
    I: Iterator<Item = String>,
{
    args.next().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("missing value for {name}"),
        )
    })
}

fn print_help() {
    println!(
        "cendek\n\nUsage:\n  cendek [--addr 127.0.0.1:8080] [--links links.tsv]\n\nlinks.tsv format:\n  slug<TAB>target_url"
    );
}

fn load_links(path: &str) -> io::Result<HashMap<String, String>> {
    let contents = fs::read_to_string(path)?;
    let mut links = HashMap::new();

    for (line_no, raw_line) in contents.lines().enumerate() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        let Some((slug, target)) = line.split_once('\t') else {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("{}:{} must contain slug<TAB>target_url", path, line_no + 1),
            ));
        };

        let slug = slug.trim().trim_matches('/').to_ascii_lowercase();
        let target = target.trim().to_string();

        validate_slug(&slug).map_err(|msg| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("{}:{} invalid slug '{}': {msg}", path, line_no + 1, slug),
            )
        })?;

        if !is_supported_target(&target) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "{}:{} target must start with http://, https://, or mailto:",
                    path,
                    line_no + 1
                ),
            ));
        }

        if links.insert(slug.clone(), target).is_some() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("{}:{} duplicate slug '{}'", path, line_no + 1, slug),
            ));
        }
    }

    Ok(links)
}

fn validate_slug(slug: &str) -> Result<(), &'static str> {
    if slug.is_empty() {
        return Err("empty slug");
    }

    if slug.len() > 64 {
        return Err("slug is longer than 64 bytes");
    }

    if matches!(slug, "api" | "healthz" | "robots.txt") {
        return Err("reserved slug");
    }

    if !slug
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_'))
    {
        return Err("allowed characters are a-z, 0-9, dash, and underscore");
    }

    Ok(())
}

fn is_supported_target(target: &str) -> bool {
    if target.bytes().any(|b| b.is_ascii_control()) {
        return false;
    }

    target.starts_with("https://") || target.starts_with("http://") || target.starts_with("mailto:")
}

fn handle_connection(mut stream: TcpStream, links: Links) -> io::Result<()> {
    stream.set_read_timeout(Some(IO_TIMEOUT))?;
    stream.set_write_timeout(Some(IO_TIMEOUT))?;

    let peer = stream.peer_addr().ok();
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut request_line = String::new();
    reader.read_line(&mut request_line)?;

    if request_line.len() > MAX_REQUEST_LINE {
        return write_response(
            &mut stream,
            414,
            "URI Too Long",
            &plain_headers(),
            "uri too long\n",
        );
    }

    let Some(request) = parse_request_line(&request_line) else {
        return write_response(
            &mut stream,
            400,
            "Bad Request",
            &plain_headers(),
            "bad request\n",
        );
    };

    let headers = read_headers(&mut reader)?;

    if request.method != "GET" && request.method != "HEAD" {
        return write_response(
            &mut stream,
            405,
            "Method Not Allowed",
            &[
                ("content-type", "text/plain; charset=utf-8"),
                ("allow", "GET, HEAD"),
            ],
            "method not allowed\n",
        );
    }

    let body_allowed = request.method == "GET";
    let response = route(&request.path, &links, &headers);
    write_http(&mut stream, response, body_allowed)?;

    if let Some(peer) = peer {
        eprintln!("{} {} {}", peer, request.method, request.path);
    }

    Ok(())
}

struct Request<'a> {
    method: &'a str,
    path: String,
}

fn parse_request_line(line: &str) -> Option<Request<'_>> {
    let mut parts = line.split_whitespace();
    let method = parts.next()?;
    let raw_target = parts.next()?;
    let version = parts.next()?;

    if !version.starts_with("HTTP/") {
        return None;
    }

    let path = raw_target
        .split_once('?')
        .map_or(raw_target, |(path, _)| path)
        .to_string();

    Some(Request { method, path })
}

#[derive(Default)]
struct HeaderInfo {
    host: Option<String>,
    forwarded_proto: Option<String>,
}

fn read_headers(reader: &mut BufReader<TcpStream>) -> io::Result<HeaderInfo> {
    let mut line = String::new();
    let mut info = HeaderInfo::default();
    let mut total_bytes = 0;

    loop {
        line.clear();
        let bytes = reader.read_line(&mut line)?;
        total_bytes += bytes;

        if total_bytes > MAX_HEADER_BYTES {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "request headers are too large",
            ));
        }

        if bytes == 0 || line == "\r\n" || line == "\n" {
            return Ok(info);
        }

        if let Some((name, value)) = line.split_once(':') {
            let name = name.trim();
            let value = value.trim();

            if name.eq_ignore_ascii_case("host") {
                info.host = Some(value.to_string());
            } else if name.eq_ignore_ascii_case("x-forwarded-proto") {
                info.forwarded_proto = Some(value.to_string());
            }
        }
    }
}

enum Response {
    Redirect(String),
    Text {
        status: u16,
        reason: &'static str,
        body: String,
        content_type: &'static str,
    },
}

fn route(path: &str, links: &HashMap<String, String>, headers: &HeaderInfo) -> Response {
    match path {
        "/" => Response::Text {
            status: 200,
            reason: "OK",
            body: render_index(links),
            content_type: "text/html; charset=utf-8",
        },
        "/healthz" => Response::Text {
            status: 200,
            reason: "OK",
            body: "ok\n".to_string(),
            content_type: "text/plain; charset=utf-8",
        },
        "/robots.txt" => Response::Text {
            status: 200,
            reason: "OK",
            body: "User-agent: *\nAllow: /\n".to_string(),
            content_type: "text/plain; charset=utf-8",
        },
        "/api/links" => Response::Text {
            status: 200,
            reason: "OK",
            body: render_json(links, public_base_url(headers)),
            content_type: "application/json; charset=utf-8",
        },
        _ => {
            let slug = path.trim_matches('/').to_ascii_lowercase();
            match links.get(&slug) {
                Some(target) => Response::Redirect(target.clone()),
                None => Response::Text {
                    status: 404,
                    reason: "Not Found",
                    body: "short link not found\n".to_string(),
                    content_type: "text/plain; charset=utf-8",
                },
            }
        }
    }
}

fn write_http(stream: &mut TcpStream, response: Response, body_allowed: bool) -> io::Result<()> {
    match response {
        Response::Redirect(location) => {
            let headers = [
                ("location", location.as_str()),
                ("cache-control", "public, max-age=300"),
                ("content-length", "0"),
            ];
            write_head(stream, 302, "Found", &headers)
        }
        Response::Text {
            status,
            reason,
            body,
            content_type,
        } => {
            let content_length = body.len().to_string();
            let headers = [
                ("content-type", content_type),
                ("cache-control", "public, max-age=300"),
                ("content-length", content_length.as_str()),
            ];
            write_head(stream, status, reason, &headers)?;
            if body_allowed {
                stream.write_all(body.as_bytes())?;
            }
            stream.flush()
        }
    }
}

fn write_response(
    stream: &mut TcpStream,
    status: u16,
    reason: &'static str,
    headers: &[(&str, &str)],
    body: &str,
) -> io::Result<()> {
    let content_length = body.len().to_string();
    let mut merged = headers.to_vec();
    merged.push(("content-length", content_length.as_str()));
    write_head(stream, status, reason, &merged)?;
    stream.write_all(body.as_bytes())?;
    stream.flush()
}

fn write_head(
    stream: &mut TcpStream,
    status: u16,
    reason: &str,
    headers: &[(&str, &str)],
) -> io::Result<()> {
    write!(stream, "HTTP/1.1 {status} {reason}\r\n")?;
    for (name, value) in headers {
        write!(stream, "{name}: {value}\r\n")?;
    }
    write!(stream, "connection: close\r\n\r\n")
}

fn plain_headers() -> [(&'static str, &'static str); 1] {
    [("content-type", "text/plain; charset=utf-8")]
}

fn render_index(links: &HashMap<String, String>) -> String {
    let mut entries: Vec<_> = links.iter().collect();
    entries.sort_by(|a, b| a.0.cmp(b.0));

    let rows = entries
        .into_iter()
        .map(|(slug, target)| {
            format!(
                "<a href=\"/{slug}\"><strong>/{slug}</strong><span>{target}</span></a>",
                slug = escape_html(slug),
                target = escape_html(target)
            )
        })
        .collect::<String>();

    format!(
        "<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width,initial-scale=1\"><title>Short Links</title><style>body{{margin:0;background:#020617;color:#f8fafc;font-family:system-ui,-apple-system,BlinkMacSystemFont,'Segoe UI',sans-serif}}main{{width:min(680px,calc(100% - 32px));margin:0 auto;padding:48px 0}}h1{{font-size:42px;margin:0 0 20px}}.links{{display:grid;gap:8px}}a{{display:flex;justify-content:space-between;gap:16px;padding:12px 14px;border:1px solid rgba(148,163,184,.22);border-radius:8px;color:inherit;text-decoration:none;background:rgba(15,23,42,.78)}}a:hover{{border-color:#10b981}}span{{color:#94a3b8;overflow-wrap:anywhere;text-align:right}}</style></head><body><main><h1>Short Links</h1><section class=\"links\">{rows}</section></main></body></html>"
    )
}

fn public_base_url(headers: &HeaderInfo) -> String {
    let proto = headers
        .forwarded_proto
        .as_deref()
        .filter(|value| *value == "http" || *value == "https")
        .unwrap_or("http");
    let host = headers
        .host
        .as_deref()
        .filter(|value| is_safe_host(value))
        .unwrap_or("127.0.0.1");

    format!("{proto}://{host}")
}

fn is_safe_host(host: &str) -> bool {
    if host.is_empty() || host.len() > 253 {
        return false;
    }

    host.bytes()
        .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b':' | b'[' | b']'))
}

fn render_json(links: &HashMap<String, String>, base_url: String) -> String {
    let mut entries: Vec<_> = links.iter().collect();
    entries.sort_by(|a, b| a.0.cmp(b.0));

    let items = entries
        .into_iter()
        .map(|(slug, target)| {
            let short_url = format!("{}/{}", base_url.trim_end_matches('/'), slug);
            format!(
                "{{\"slug\":\"{}\",\"url\":\"{}\",\"target\":\"{}\"}}",
                escape_json(slug),
                escape_json(&short_url),
                escape_json(target)
            )
        })
        .collect::<Vec<_>>()
        .join(",");

    format!("[{items}]\n")
}

fn escape_html(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

fn escape_json(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());

    for ch in value.chars() {
        match ch {
            '"' => escaped.push_str("\\\""),
            '\\' => escaped.push_str("\\\\"),
            '\n' => escaped.push_str("\\n"),
            '\r' => escaped.push_str("\\r"),
            '\t' => escaped.push_str("\\t"),
            ch if ch.is_control() => {
                escaped.push_str(&format!("\\u{:04x}", ch as u32));
            }
            ch => escaped.push(ch),
        }
    }

    escaped
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_control_characters_in_targets() {
        assert!(is_supported_target("https://example.com/path"));
        assert!(is_supported_target("mailto:user@example.com"));
        assert!(!is_supported_target("https://example.com/\r\nx: y"));
        assert!(!is_supported_target("javascript:alert(1)"));
    }

    #[test]
    fn validates_hosts_used_for_public_api_urls() {
        assert!(is_safe_host("cendek.example.com"));
        assert!(is_safe_host("127.0.0.1:1234"));
        assert!(is_safe_host("[::1]:1234"));
        assert!(!is_safe_host(""));
        assert!(!is_safe_host("example.com\r\nx: y"));
        assert!(!is_safe_host("example.com/path"));
    }

    #[test]
    fn api_json_includes_short_url_and_target() {
        let links = HashMap::from([("app".to_string(), "https://app.example.com/".to_string())]);
        let headers = HeaderInfo {
            host: Some("cendek.example.com".to_string()),
            forwarded_proto: Some("https".to_string()),
        };

        let json = render_json(&links, public_base_url(&headers));

        assert!(json.contains("\"slug\":\"app\""));
        assert!(json.contains("\"url\":\"https://cendek.example.com/app\""));
        assert!(json.contains("\"target\":\"https://app.example.com/\""));
    }
}
