//! A small HTTP/1.1 server built on `std::net`.
//!
//! It implements the parts EconBox needs and nothing else: request line,
//! headers, `Content-Length` bodies, keep-alive, CORS. There is no TLS, no
//! routing framework and no async runtime. Put a reverse proxy in front of it
//! for anything public — see `docs/ARCHITECTURE.md`.

pub mod pool;
pub mod request;
pub mod response;
pub mod server;

pub use request::{ReadError, Request};
pub use response::Response;
pub use server::Server;
