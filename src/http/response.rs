//! HTTP responses.
//!
//! Every response carries an explicit `Content-Length`, which is what lets the
//! server keep connections alive without chunked encoding.

use std::io::{self, Write};

use crate::json::Json;

#[derive(Debug, Clone)]
pub struct Response {
    pub status: u16,
    pub content_type: String,
    pub body: Vec<u8>,
    /// Extra headers, sent verbatim after the standard ones.
    pub headers: Vec<(String, String)>,
}

impl Response {
    pub fn new(status: u16, content_type: &str, body: Vec<u8>) -> Response {
        Response {
            status,
            content_type: content_type.to_string(),
            body,
            headers: Vec::new(),
        }
    }

    pub fn json(status: u16, value: &Json) -> Response {
        Response::new(
            status,
            "application/json; charset=utf-8",
            value.to_string().into_bytes(),
        )
    }

    pub fn text(status: u16, body: &str) -> Response {
        Response::new(
            status,
            "text/plain; charset=utf-8",
            body.as_bytes().to_vec(),
        )
    }

    pub fn html(body: &str) -> Response {
        Response::new(200, "text/html; charset=utf-8", body.as_bytes().to_vec())
    }

    pub fn empty(status: u16) -> Response {
        Response::new(status, "text/plain; charset=utf-8", Vec::new())
    }

    /// The single error shape the whole API uses.
    pub fn error(status: u16, message: &str) -> Response {
        Response::json(
            status,
            &Json::obj([
                ("error", Json::str(message)),
                ("status", Json::num(status as f64)),
            ]),
        )
    }

    pub fn with_header(mut self, name: &str, value: &str) -> Response {
        self.headers.push((name.to_string(), value.to_string()));
        self
    }

    /// Serialise the response onto the wire.
    pub fn write_to<W: Write>(&self, out: &mut W, keep_alive: bool) -> io::Result<()> {
        let mut head = String::with_capacity(256);
        head.push_str(&format!(
            "HTTP/1.1 {} {}\r\n",
            self.status,
            status_text(self.status)
        ));
        head.push_str(&format!("Content-Type: {}\r\n", self.content_type));
        head.push_str(&format!("Content-Length: {}\r\n", self.body.len()));
        head.push_str(if keep_alive {
            "Connection: keep-alive\r\n"
        } else {
            "Connection: close\r\n"
        });
        // The renderer is usually served from a different origin than the API.
        head.push_str("Access-Control-Allow-Origin: *\r\n");
        head.push_str("Access-Control-Allow-Methods: GET, POST, OPTIONS\r\n");
        head.push_str("Access-Control-Allow-Headers: content-type\r\n");
        head.push_str("Cache-Control: no-store\r\n");
        for (name, value) in &self.headers {
            head.push_str(&format!("{name}: {value}\r\n"));
        }
        head.push_str("\r\n");

        out.write_all(head.as_bytes())?;
        out.write_all(&self.body)?;
        out.flush()
    }
}

pub fn status_text(status: u16) -> &'static str {
    match status {
        200 => "OK",
        204 => "No Content",
        400 => "Bad Request",
        404 => "Not Found",
        405 => "Method Not Allowed",
        413 => "Payload Too Large",
        500 => "Internal Server Error",
        501 => "Not Implemented",
        503 => "Service Unavailable",
        _ => "Unknown",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn render(response: &Response, keep_alive: bool) -> String {
        let mut buffer = Vec::new();
        response.write_to(&mut buffer, keep_alive).unwrap();
        String::from_utf8(buffer).unwrap()
    }

    #[test]
    fn writes_a_json_response() {
        let response = Response::json(200, &Json::obj([("ok", Json::Bool(true))]));
        let raw = render(&response, true);
        assert!(raw.starts_with("HTTP/1.1 200 OK\r\n"));
        assert!(raw.contains("Content-Length: 11\r\n"));
        assert!(raw.contains("Connection: keep-alive\r\n"));
        assert!(raw.ends_with("{\"ok\":true}"));
    }

    #[test]
    fn errors_share_one_shape() {
        let raw = render(&Response::error(404, "no such route"), false);
        assert!(raw.contains("HTTP/1.1 404 Not Found"));
        assert!(raw.contains("{\"error\":\"no such route\",\"status\":404}"));
        assert!(raw.contains("Connection: close"));
    }
}
