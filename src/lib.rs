//! EconBoxServer: the simulation and rendering server behind EconBox.
//!
//! The split is the whole idea: **the server owns every number, the client
//! owns every pixel**. A client asks for a frame at a given size and gets back
//! a flat list of drawing commands — rectangles, lines, circles, text — that it
//! paints in order. It never runs the economy, never picks a colour, never
//! computes an axis.
//!
//! ```text
//!   EconBox client                     EconBoxServer
//!   -----------------                  ----------------------------------
//!   POST /api/frame  ---------------->  sim::World::step   (economy)
//!   {width, height, advance}            render::build      (layout, colour)
//!                    <----------------  {ops: [...]}
//!   paint ops on a canvas
//! ```
//!
//! Modules, bottom up:
//!
//! - [`rng`] deterministic random numbers, so a seed reproduces a run
//! - [`json`] a small JSON value, parser and serializer
//! - [`http`] a minimal HTTP/1.1 server on `std::net`
//! - [`sim`] the economy: agents, a market, a world that steps
//! - [`render`] world state to drawing commands
//! - [`api`] the endpoints tying the two together
//! - [`config`] defaults, environment and command line
//!
//! The crate has no dependencies. See `docs/ARCHITECTURE.md` for why, and for
//! the extension points if you want more.

pub mod api;
pub mod config;
pub mod http;
pub mod json;
pub mod render;
pub mod rng;
pub mod sim;
