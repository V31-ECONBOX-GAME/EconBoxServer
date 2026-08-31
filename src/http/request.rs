//! HTTP/1.1 request parsing.
//!
//! Only what the API needs: a request line, headers, an optional
//! `Content-Length` body. Chunked bodies are refused rather than mis-parsed.

use std::io::{BufRead, Read};

/// Largest request line plus headers accepted, in bytes.
pub const MAX_HEADER_BYTES: usize = 16 * 1024;
/// Largest body accepted, in bytes.
pub const MAX_BODY_BYTES: usize = 1024 * 1024;

#[derive(Debug)]
pub enum ReadError {
    /// The request could not be understood; answer 400.
    Malformed(&'static str),
    /// The request exceeded a limit; answer 413.
    TooLarge,
    /// Something the server does not implement, such as chunked encoding.
    Unsupported(&'static str),
    Io(std::io::Error),
}

impl ReadError {
    pub fn status(&self) -> u16 {
        match self {
            ReadError::Malformed(_) => 400,
            ReadError::TooLarge => 413,
            ReadError::Unsupported(_) => 501,
            ReadError::Io(_) => 400,
        }
    }

    pub fn message(&self) -> String {
        match self {
            ReadError::Malformed(m) | ReadError::Unsupported(m) => (*m).to_string(),
            ReadError::TooLarge => "request too large".to_string(),
            ReadError::Io(e) => format!("read failed: {e}"),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct Request {
    /// Upper-case method, e.g. `GET`.
    pub method: String,
    /// Percent-decoded path, without the query string.
    pub path: String,
    /// Percent-decoded query parameters, in the order they appeared.
    pub query: Vec<(String, String)>,
    /// Header names are lower-cased; values are trimmed.
    pub headers: Vec<(String, String)>,
    pub body: String,
    /// Whether the connection may be reused for another request.
    pub keep_alive: bool,
}

impl Request {
    /// Read one request. `Ok(None)` means the peer closed the connection
    /// cleanly, which is the normal end of a keep-alive session.
    pub fn read<R: BufRead>(reader: &mut R) -> Result<Option<Request>, ReadError> {
        let mut budget = MAX_HEADER_BYTES;

        let start = match read_line(reader, &mut budget)? {
            Some(line) => line,
            None => return Ok(None),
        };
        let mut parts = start.split_whitespace();
        let method = parts
            .next()
            .ok_or(ReadError::Malformed("empty request line"))?;
        let target = parts
            .next()
            .ok_or(ReadError::Malformed("missing request target"))?;
        let version = parts.next().unwrap_or("HTTP/1.0");

        let mut headers = Vec::new();
        loop {
            let line = read_line(reader, &mut budget)?
                .ok_or(ReadError::Malformed("headers ended early"))?;
            if line.is_empty() {
                break;
            }
            let (name, value) = line
                .split_once(':')
                .ok_or(ReadError::Malformed("malformed header"))?;
            headers.push((name.trim().to_ascii_lowercase(), value.trim().to_string()));
        }

        if find(&headers, "transfer-encoding").is_some() {
            return Err(ReadError::Unsupported("transfer-encoding is not supported"));
        }

        let length = match find(&headers, "content-length") {
            Some(value) => value
                .parse::<usize>()
                .map_err(|_| ReadError::Malformed("invalid content-length"))?,
            None => 0,
        };
        if length > MAX_BODY_BYTES {
            return Err(ReadError::TooLarge);
        }
        let mut raw = Vec::with_capacity(length.min(8 * 1024));
        if length > 0 {
            reader
                .by_ref()
                .take(length as u64)
                .read_to_end(&mut raw)
                .map_err(ReadError::Io)?;
            if raw.len() != length {
                return Err(ReadError::Malformed("body shorter than content-length"));
            }
        }

        let connection = find(&headers, "connection")
            .unwrap_or_default()
            .to_ascii_lowercase();
        let keep_alive = if version.ends_with("1.0") {
            connection.contains("keep-alive")
        } else {
            !connection.contains("close")
        };

        let (path, query) = split_target(target);
        Ok(Some(Request {
            method: method.to_ascii_uppercase(),
            path,
            query,
            headers,
            body: String::from_utf8_lossy(&raw).into_owned(),
            keep_alive,
        }))
    }

    pub fn header(&self, name: &str) -> Option<&str> {
        find(&self.headers, name)
    }

    /// First value of a query parameter.
    pub fn query(&self, key: &str) -> Option<&str> {
        self.query
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
    }

    /// Query parameter parsed as a number, ignoring values that do not parse.
    pub fn query_f64(&self, key: &str) -> Option<f64> {
        self.query(key)?
            .parse::<f64>()
            .ok()
            .filter(|v| v.is_finite())
    }
}

fn find<'a>(headers: &'a [(String, String)], name: &str) -> Option<&'a str> {
    headers
        .iter()
        .find(|(k, _)| k == name)
        .map(|(_, v)| v.as_str())
}

/// Read one CRLF- or LF-terminated line, charging it against `budget`.
fn read_line<R: BufRead>(reader: &mut R, budget: &mut usize) -> Result<Option<String>, ReadError> {
    let mut raw = Vec::new();
    let read = reader
        .by_ref()
        .take(*budget as u64 + 1)
        .read_until(b'\n', &mut raw)
        .map_err(ReadError::Io)?;
    if read == 0 {
        return Ok(None);
    }
    if read > *budget || !raw.ends_with(b"\n") {
        return Err(ReadError::TooLarge);
    }
    *budget -= read;
    while raw.last().is_some_and(|b| *b == b'\n' || *b == b'\r') {
        raw.pop();
    }
    Ok(Some(String::from_utf8_lossy(&raw).into_owned()))
}

fn split_target(target: &str) -> (String, Vec<(String, String)>) {
    let (path, rest) = match target.split_once('?') {
        Some((path, rest)) => (path, rest),
        None => (target, ""),
    };
    let query = rest
        .split('&')
        .filter(|pair| !pair.is_empty())
        .map(|pair| match pair.split_once('=') {
            Some((k, v)) => (percent_decode(k), percent_decode(v)),
            None => (percent_decode(pair), String::new()),
        })
        .collect();
    (percent_decode(path), query)
}

/// Decode `%XX` escapes and `+` as space. Invalid escapes are left as-is.
fn percent_decode(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            b'%' if i + 2 < bytes.len() => match u8::from_str_radix(&input[i + 1..i + 3], 16) {
                Ok(byte) => {
                    out.push(byte);
                    i += 3;
                }
                Err(_) => {
                    out.push(b'%');
                    i += 1;
                }
            },
            byte => {
                out.push(byte);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn parse(raw: &str) -> Result<Option<Request>, ReadError> {
        Request::read(&mut Cursor::new(raw.as_bytes().to_vec()))
    }

    #[test]
    fn parses_a_get() {
        let request = parse("GET /api/frame?width=800&height=600 HTTP/1.1\r\nHost: x\r\n\r\n")
            .unwrap()
            .unwrap();
        assert_eq!(request.method, "GET");
        assert_eq!(request.path, "/api/frame");
        assert_eq!(request.query("width"), Some("800"));
        assert_eq!(request.query_f64("height"), Some(600.0));
        assert_eq!(request.header("host"), Some("x"));
        assert!(request.keep_alive);
    }

    #[test]
    fn parses_a_post_body() {
        let request =
            parse("POST /api/step HTTP/1.1\r\nContent-Length: 13\r\n\r\n{\"ticks\": 4}\r\n")
                .unwrap()
                .unwrap();
        assert_eq!(request.body, "{\"ticks\": 4}\r");
    }

    #[test]
    fn closed_connection_reads_as_none() {
        assert!(parse("").unwrap().is_none());
    }

    #[test]
    fn honours_connection_close() {
        let request = parse("GET / HTTP/1.1\r\nConnection: close\r\n\r\n")
            .unwrap()
            .unwrap();
        assert!(!request.keep_alive);
        let legacy = parse("GET / HTTP/1.0\r\n\r\n").unwrap().unwrap();
        assert!(!legacy.keep_alive);
    }

    #[test]
    fn rejects_oversized_headers() {
        let raw = format!(
            "GET / HTTP/1.1\r\nX: {}\r\n\r\n",
            "a".repeat(MAX_HEADER_BYTES)
        );
        assert!(matches!(parse(&raw), Err(ReadError::TooLarge)));
    }

    #[test]
    fn rejects_chunked_bodies() {
        let raw = "POST / HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\n";
        assert!(matches!(parse(raw), Err(ReadError::Unsupported(_))));
    }

    #[test]
    fn decodes_percent_escapes() {
        let request = parse("GET /a%20b?q=x%2By+z HTTP/1.1\r\n\r\n")
            .unwrap()
            .unwrap();
        assert_eq!(request.path, "/a b");
        assert_eq!(request.query("q"), Some("x+y z"));
    }
}
