//! The `econbox-server` binary.
//!
//! Start-up is deliberately linear — read the configuration, build the world,
//! bind the socket, serve — so the whole program reads top to bottom. All of
//! the substance lives in the library next door.

use std::sync::Arc;

use econbox_server::api::Api;
use econbox_server::config::{Config, HELP, Outcome};
use econbox_server::http::Server;
use econbox_server::sim::{Params, World};

fn main() {
    if let Err(error) = run() {
        eprintln!("econbox-server: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let config = match Config::from_process()? {
        Outcome::Help => {
            println!("{HELP}");
            return Ok(());
        }
        Outcome::Version => {
            println!("{} {}", env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION"));
            return Ok(());
        }
        Outcome::Run(config) => *config,
    };

    let world = World::new(config.seed, config.agents, Params::default());
    let agents = world.agents.len();
    let api = Arc::new(Api::new(world));

    let server = Server::bind(&config.addr, config.workers)
        .map_err(|error| format!("cannot listen on {}: {error}", config.addr))?;
    let addr = server
        .local_addr()
        .map_err(|error| format!("cannot read the local address: {error}"))?;

    println!("{} {}", env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION"));
    println!("  client   http://{addr}/");
    println!("  api      http://{addr}/api/health");
    println!(
        "  world    {agents} agents, seed {}, {} worker threads",
        config.seed, config.workers
    );

    server
        .run(move |request| api.handle(request))
        .map_err(|error| format!("server stopped: {error}"))
}
