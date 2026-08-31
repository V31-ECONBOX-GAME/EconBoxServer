//! Server configuration: built-in defaults, then environment, then flags.

use std::thread;

pub const DEFAULT_ADDR: &str = "127.0.0.1:8080";
pub const DEFAULT_AGENTS: usize = 240;
/// A fixed default seed keeps the out-of-the-box run reproducible; pass
/// `--seed` for a different economy.
pub const DEFAULT_SEED: u64 = 42;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    pub addr: String,
    pub agents: usize,
    pub seed: u64,
    pub workers: usize,
}

/// What the command line asked the process to do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    Run(Box<Config>),
    Help,
    Version,
}

pub const HELP: &str = "\
econbox-server - the EconBox simulation and rendering server

USAGE:
    econbox-server [OPTIONS]

OPTIONS:
    --addr <HOST:PORT>   Address to listen on        [default: 127.0.0.1:8080]
    --agents <N>         Number of agents            [default: 240]
    --seed <N>           Simulation seed             [default: 42]
    --workers <N>        Request worker threads      [default: 2x CPU cores]
    -h, --help           Print this help
    -V, --version        Print the version

ENVIRONMENT:
    ECONBOX_ADDR, ECONBOX_AGENTS, ECONBOX_SEED, ECONBOX_WORKERS
    Flags take precedence over the environment.

Open http://<addr>/ for the reference client, or point your own EconBox
client at the JSON API documented in docs/API.md.";

impl Default for Config {
    fn default() -> Config {
        Config {
            addr: DEFAULT_ADDR.to_string(),
            agents: DEFAULT_AGENTS,
            seed: DEFAULT_SEED,
            workers: default_workers(),
        }
    }
}

impl Config {
    /// Read the real process environment and command line.
    pub fn from_process() -> Result<Outcome, String> {
        Config::parse(std::env::args().skip(1), |key| std::env::var(key).ok())
    }

    /// Pure version of [`Config::from_process`], so the precedence rules can be
    /// tested without touching global state.
    pub fn parse<I, F>(args: I, env: F) -> Result<Outcome, String>
    where
        I: IntoIterator<Item = String>,
        F: Fn(&str) -> Option<String>,
    {
        let mut config = Config::default();

        if let Some(value) = env("ECONBOX_ADDR") {
            config.addr = value;
        }
        if let Some(value) = env("ECONBOX_AGENTS") {
            config.agents = number("ECONBOX_AGENTS", &value)?;
        }
        if let Some(value) = env("ECONBOX_SEED") {
            config.seed = number("ECONBOX_SEED", &value)?;
        }
        if let Some(value) = env("ECONBOX_WORKERS") {
            config.workers = number("ECONBOX_WORKERS", &value)?;
        }

        let mut args = args.into_iter().peekable();
        while let Some(arg) = args.next() {
            // Accept both `--flag value` and `--flag=value`.
            let (flag, inline) = match arg.split_once('=') {
                Some((flag, value)) => (flag.to_string(), Some(value.to_string())),
                None => (arg, None),
            };
            let mut value = || -> Result<String, String> {
                inline
                    .clone()
                    .or_else(|| args.next())
                    .ok_or_else(|| format!("{flag} needs a value"))
            };
            match flag.as_str() {
                "-h" | "--help" => return Ok(Outcome::Help),
                "-V" | "--version" => return Ok(Outcome::Version),
                "--addr" => config.addr = value()?,
                "--agents" => config.agents = number("--agents", &value()?)?,
                "--seed" => config.seed = number("--seed", &value()?)?,
                "--workers" => config.workers = number("--workers", &value()?)?,
                other => return Err(format!("unknown option: {other}")),
            }
        }

        config.workers = config.workers.clamp(1, 512);
        Ok(Outcome::Run(Box::new(config)))
    }
}

fn number<T: std::str::FromStr>(name: &str, value: &str) -> Result<T, String> {
    value
        .trim()
        .parse::<T>()
        .map_err(|_| format!("invalid value for {name}: {value}"))
}

/// Two workers per core: request handling is dominated by the simulation lock,
/// so a little oversubscription keeps cores busy without thrashing.
fn default_workers() -> usize {
    thread::available_parallelism()
        .map(|n| n.get() * 2)
        .unwrap_or(8)
        .clamp(4, 64)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &[&str], env: &[(&str, &str)]) -> Result<Outcome, String> {
        let env: Vec<(String, String)> = env
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        Config::parse(args.iter().map(|a| a.to_string()), move |key| {
            env.iter().find(|(k, _)| k == key).map(|(_, v)| v.clone())
        })
    }

    fn config(outcome: Outcome) -> Config {
        match outcome {
            Outcome::Run(config) => *config,
            other => panic!("expected a run, got {other:?}"),
        }
    }

    #[test]
    fn defaults_apply() {
        let config = config(parse(&[], &[]).unwrap());
        assert_eq!(config.addr, DEFAULT_ADDR);
        assert_eq!(config.agents, DEFAULT_AGENTS);
        assert_eq!(config.seed, DEFAULT_SEED);
    }

    #[test]
    fn flags_beat_the_environment() {
        let outcome = parse(
            &["--agents", "10", "--seed=7"],
            &[("ECONBOX_AGENTS", "999"), ("ECONBOX_ADDR", "0.0.0.0:9000")],
        )
        .unwrap();
        let config = config(outcome);
        assert_eq!(config.agents, 10);
        assert_eq!(config.seed, 7);
        assert_eq!(config.addr, "0.0.0.0:9000");
    }

    #[test]
    fn help_and_version_short_circuit() {
        assert_eq!(parse(&["--help"], &[]).unwrap(), Outcome::Help);
        assert_eq!(parse(&["-V", "--nonsense"], &[]).unwrap(), Outcome::Version);
    }

    #[test]
    fn bad_input_is_reported() {
        assert!(parse(&["--agents", "many"], &[]).is_err());
        assert!(parse(&["--agents"], &[]).is_err());
        assert!(parse(&["--nope"], &[]).is_err());
        assert!(parse(&[], &[("ECONBOX_SEED", "x")]).is_err());
    }
}
