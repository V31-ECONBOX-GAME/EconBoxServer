//! The accept loop.

use std::io::BufReader;
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::Arc;
use std::time::Duration;

use crate::http::pool::Pool;
use crate::http::request::{ReadError, Request};
use crate::http::response::Response;

/// How long a connection may stay silent before it is dropped. This also caps
/// how long an idle keep-alive connection holds a worker.
const READ_TIMEOUT: Duration = Duration::from_secs(30);
const WRITE_TIMEOUT: Duration = Duration::from_secs(30);

pub struct Server {
    listener: TcpListener,
    workers: usize,
}

impl Server {
    /// Bind without serving, so the caller can report the real address before
    /// the loop starts. Port 0 is allowed and picks a free port.
    pub fn bind(addr: &str, workers: usize) -> std::io::Result<Server> {
        Ok(Server {
            listener: TcpListener::bind(addr)?,
            workers: workers.max(1),
        })
    }

    pub fn local_addr(&self) -> std::io::Result<SocketAddr> {
        self.listener.local_addr()
    }

    /// Serve until the listener fails. `handler` is shared by every worker, so
    /// any state it captures must be behind a lock.
    pub fn run<H>(self, handler: H) -> std::io::Result<()>
    where
        H: Fn(&Request) -> Response + Send + Sync + 'static,
    {
        let handler = Arc::new(handler);
        let pool = Pool::new(self.workers);
        for incoming in self.listener.incoming() {
            match incoming {
                Ok(stream) => {
                    let handler = Arc::clone(&handler);
                    pool.execute(move || serve_connection(stream, handler.as_ref()));
                }
                // A failed accept is usually one bad client, not a dead server.
                Err(error) => eprintln!("econbox-server: accept failed: {error}"),
            }
        }
        Ok(())
    }
}

fn serve_connection<H>(stream: TcpStream, handler: &H)
where
    H: Fn(&Request) -> Response,
{
    // Frames are small and latency-sensitive; do not wait to coalesce them.
    let _ = stream.set_nodelay(true);
    let _ = stream.set_read_timeout(Some(READ_TIMEOUT));
    let _ = stream.set_write_timeout(Some(WRITE_TIMEOUT));

    let mut writer = match stream.try_clone() {
        Ok(writer) => writer,
        Err(_) => return,
    };
    let mut reader = BufReader::new(stream);

    loop {
        match Request::read(&mut reader) {
            // Clean close by the peer.
            Ok(None) => return,
            Ok(Some(request)) => {
                let keep_alive = request.keep_alive;
                let response = handler(&request);
                if response.write_to(&mut writer, keep_alive).is_err() || !keep_alive {
                    return;
                }
            }
            // An idle connection that timed out is not an error worth reporting.
            Err(ReadError::Io(ref error)) if timed_out(error) => return,
            Err(error) => {
                let _ =
                    Response::error(error.status(), &error.message()).write_to(&mut writer, false);
                return;
            }
        }
    }
}

fn timed_out(error: &std::io::Error) -> bool {
    matches!(
        error.kind(),
        std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};

    #[test]
    fn serves_requests_over_one_connection() {
        let server = Server::bind("127.0.0.1:0", 2).expect("binds");
        let addr = server.local_addr().expect("has an address");
        std::thread::spawn(move || {
            let _ = server.run(|request| Response::text(200, &format!("hi {}", request.path)));
        });

        let mut stream = TcpStream::connect(addr).expect("connects");
        // Two requests on one connection prove keep-alive framing is right.
        stream
            .write_all(b"GET /a HTTP/1.1\r\nHost: t\r\n\r\nGET /b HTTP/1.1\r\nHost: t\r\nConnection: close\r\n\r\n")
            .expect("writes");
        let mut raw = String::new();
        stream.read_to_string(&mut raw).expect("reads");
        assert!(raw.contains("hi /a"), "{raw}");
        assert!(raw.contains("hi /b"), "{raw}");
    }

    #[test]
    fn reports_malformed_requests() {
        let server = Server::bind("127.0.0.1:0", 1).expect("binds");
        let addr = server.local_addr().expect("has an address");
        std::thread::spawn(move || {
            let _ = server.run(|_| Response::empty(200));
        });

        let mut stream = TcpStream::connect(addr).expect("connects");
        stream
            .write_all(b"GET / HTTP/1.1\r\nbroken header\r\n\r\n")
            .expect("writes");
        let mut raw = String::new();
        stream.read_to_string(&mut raw).expect("reads");
        assert!(raw.starts_with("HTTP/1.1 400 Bad Request"), "{raw}");
    }
}
