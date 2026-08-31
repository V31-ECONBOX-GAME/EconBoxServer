//! The HTTP API: routing plus one handler per endpoint.
//!
//! Everything the client can do to the world goes through this file, and every
//! handler is short on purpose — the interesting code is in `sim` and
//! `render`. `docs/API.md` documents the endpoints.

use std::sync::{Mutex, MutexGuard};
use std::time::Instant;

use crate::http::{Request, Response};
use crate::json::{self, Json};
use crate::render;
use crate::sim::World;
use crate::sim::world::{HISTORY_CAP, MAX_AGENTS};

/// The reference client, compiled into the binary so `cargo run` is enough to
/// see something. Serving it is a convenience, not part of the API.
const CLIENT_HTML: &str = include_str!("../web/index.html");

/// Largest number of ticks one request may run. Bounds how long a single call
/// can hold the world lock.
const MAX_TICKS_PER_CALL: u64 = 10_000;

/// Frame size used when the client does not ask for one.
const DEFAULT_WIDTH: f64 = 1280.0;
const DEFAULT_HEIGHT: f64 = 720.0;

pub struct Api {
    world: Mutex<World>,
    started: Instant,
}

impl Api {
    pub fn new(world: World) -> Api {
        Api {
            world: Mutex::new(world),
            started: Instant::now(),
        }
    }

    /// Route one request. Unknown paths give 404, known paths with the wrong
    /// method give 405, so a client can tell a typo from a mistake.
    pub fn handle(&self, request: &Request) -> Response {
        // CORS preflight: the browser sends this before a cross-origin POST.
        if request.method == "OPTIONS" {
            return Response::empty(204);
        }
        let method = request.method.as_str();
        match (method, request.path.as_str()) {
            ("GET", "/") => Response::html(CLIENT_HTML),
            ("GET", "/api/health") => self.health(),
            ("GET", "/api/state") => Response::json(200, &self.world().to_json()),
            ("GET", "/api/history") => self.history(request),
            ("GET", "/api/params") => Response::json(200, &self.world().params.to_json()),
            ("POST", "/api/params") => self.params(request),
            ("POST", "/api/step") => self.step(request),
            ("GET", "/api/frame") | ("POST", "/api/frame") => self.frame(request),
            ("POST", "/api/reset") => self.reset(request),
            (_, path) if is_known(path) => Response::error(405, "method not allowed"),
            _ => Response::error(404, "no such endpoint; see docs/API.md"),
        }
    }

    /// The world lock. A panic in one handler poisons the mutex; the world is
    /// still structurally valid, so recovering beats refusing every later
    /// request.
    fn world(&self) -> MutexGuard<'_, World> {
        self.world
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
    }

    fn health(&self) -> Response {
        let world = self.world();
        Response::json(
            200,
            &Json::obj([
                ("status", Json::str("ok")),
                ("name", Json::str(env!("CARGO_PKG_NAME"))),
                ("version", Json::str(env!("CARGO_PKG_VERSION"))),
                ("protocol", Json::num(render::PROTOCOL_VERSION as f64)),
                (
                    "uptime_ms",
                    Json::num(self.started.elapsed().as_millis() as f64),
                ),
                ("tick", Json::num(world.tick as f64)),
                ("agents", Json::num(world.agents.len() as f64)),
            ]),
        )
    }

    fn history(&self, request: &Request) -> Response {
        let limit = request
            .query_f64("limit")
            .map(|value| value.clamp(1.0, HISTORY_CAP as f64) as usize)
            .unwrap_or(256);
        let world = self.world();
        Response::json(
            200,
            &Json::obj([
                ("tick", Json::num(world.tick as f64)),
                ("samples", world.history_json(limit)),
            ]),
        )
    }

    fn params(&self, request: &Request) -> Response {
        let body = match body_json(request) {
            Ok(body) => body,
            Err(response) => return response,
        };
        let mut world = self.world();
        world.params.patch(&body);
        Response::json(200, &world.params.to_json())
    }

    fn step(&self, request: &Request) -> Response {
        let body = match body_json(request) {
            Ok(body) => body,
            Err(response) => return response,
        };
        let ticks = body
            .get("ticks")
            .as_f64()
            .unwrap_or(1.0)
            .clamp(0.0, MAX_TICKS_PER_CALL as f64) as u64;
        let mut world = self.world();
        world.step_many(ticks);
        let mut state = world.to_json();
        if let Json::Obj(fields) = &mut state {
            fields.push(("stepped".to_string(), Json::num(ticks as f64)));
        }
        Response::json(200, &state)
    }

    /// The endpoint EconBox calls every frame.
    ///
    /// `GET` is read-only, so it is safe to poll from a browser address bar or
    /// a cache. `POST` may also advance the simulation, which lets a client run
    /// the whole loop in one round trip.
    fn frame(&self, request: &Request) -> Response {
        let body = match body_json(request) {
            Ok(body) => body,
            Err(response) => return response,
        };
        let width = pick(request, &body, "width").unwrap_or(DEFAULT_WIDTH);
        let height = pick(request, &body, "height").unwrap_or(DEFAULT_HEIGHT);
        let advance = if request.method == "POST" {
            body.get("advance")
                .as_f64()
                .unwrap_or(0.0)
                .clamp(0.0, MAX_TICKS_PER_CALL as f64) as u64
        } else {
            0
        };

        let mut world = self.world();
        world.step_many(advance);
        let scene = render::build(&world, width, height);
        Response::json(200, &scene.to_json())
    }

    fn reset(&self, request: &Request) -> Response {
        let body = match body_json(request) {
            Ok(body) => body,
            Err(response) => return response,
        };
        let mut world = self.world();
        let seed = body.get("seed").as_u64().unwrap_or(world.seed);
        let agents = body
            .get("agents")
            .as_f64()
            .map(|value| value.clamp(1.0, MAX_AGENTS as f64) as usize)
            .unwrap_or(world.agents.len());
        if !body.get("params").is_null() {
            let mut params = world.params;
            params.patch(body.get("params"));
            world.params = params;
        }
        world.reset(seed, agents);
        Response::json(200, &world.to_json())
    }
}

fn is_known(path: &str) -> bool {
    matches!(
        path,
        "/" | "/api/health"
            | "/api/state"
            | "/api/history"
            | "/api/params"
            | "/api/step"
            | "/api/frame"
            | "/api/reset"
    )
}

/// Parse the request body as JSON. An empty body is `null`, so every handler
/// can treat "no body" as "no overrides".
fn body_json(request: &Request) -> Result<Json, Response> {
    if request.body.trim().is_empty() {
        return Ok(Json::Null);
    }
    json::parse(&request.body)
        .map_err(|error| Response::error(400, &format!("invalid JSON body: {error}")))
}

/// A number that may arrive either as a query parameter or as a body field.
fn pick(request: &Request, body: &Json, key: &str) -> Option<f64> {
    request.query_f64(key).or_else(|| body.get(key).as_f64())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::Params;

    fn api() -> Api {
        Api::new(World::new(1, 50, Params::default()))
    }

    fn request(method: &str, target: &str, body: &str) -> Request {
        let raw = format!(
            "{method} {target} HTTP/1.1\r\nContent-Length: {}\r\n\r\n{body}",
            body.len()
        );
        crate::http::Request::read(&mut std::io::Cursor::new(raw.into_bytes()))
            .expect("parses")
            .expect("a request")
    }

    fn json_body(response: &Response) -> Json {
        json::parse(std::str::from_utf8(&response.body).expect("utf-8")).expect("json")
    }

    #[test]
    fn health_reports_the_protocol() {
        let response = api().handle(&request("GET", "/api/health", ""));
        assert_eq!(response.status, 200);
        let body = json_body(&response);
        assert_eq!(body.get("status").as_str(), Some("ok"));
        assert_eq!(
            body.get("protocol").as_u64(),
            Some(render::PROTOCOL_VERSION as u64)
        );
    }

    #[test]
    fn stepping_advances_the_world() {
        let api = api();
        let response = api.handle(&request("POST", "/api/step", r#"{"ticks":12}"#));
        assert_eq!(json_body(&response).get("tick").as_u64(), Some(12));
    }

    #[test]
    fn get_frame_never_advances() {
        let api = api();
        api.handle(&request("POST", "/api/step", r#"{"ticks":3}"#));
        let response = api.handle(&request("GET", "/api/frame?width=800&height=600", ""));
        let body = json_body(&response);
        assert_eq!(body.get("tick").as_u64(), Some(3));
        assert_eq!(body.get("width").as_f64(), Some(800.0));
        assert!(!body.get("ops").is_null());
    }

    #[test]
    fn post_frame_can_advance() {
        let api = api();
        let response = api.handle(&request(
            "POST",
            "/api/frame",
            r#"{"width":640,"height":480,"advance":5}"#,
        ));
        assert_eq!(json_body(&response).get("tick").as_u64(), Some(5));
    }

    #[test]
    fn params_are_patched_and_clamped() {
        let api = api();
        let response = api.handle(&request("POST", "/api/params", r#"{"tax_rate":9.0}"#));
        let body = json_body(&response);
        assert_eq!(body.get("tax_rate").as_f64(), Some(0.5));
        // Absent fields keep their previous value.
        assert_eq!(body.get("price_flex").as_f64(), Some(0.08));
    }

    #[test]
    fn reset_rebuilds_the_world() {
        let api = api();
        api.handle(&request("POST", "/api/step", r#"{"ticks":9}"#));
        let response = api.handle(&request("POST", "/api/reset", r#"{"seed":5,"agents":7}"#));
        let body = json_body(&response);
        assert_eq!(body.get("tick").as_u64(), Some(0));
        assert_eq!(body.get("seed").as_u64(), Some(5));
        assert_eq!(body.get("agents").as_u64(), Some(7));
    }

    #[test]
    fn bad_json_is_rejected() {
        let response = api().handle(&request("POST", "/api/step", "{oops"));
        assert_eq!(response.status, 400);
    }

    #[test]
    fn unknown_and_mismatched_routes() {
        let api = api();
        assert_eq!(api.handle(&request("GET", "/nope", "")).status, 404);
        assert_eq!(api.handle(&request("GET", "/api/step", "")).status, 405);
        assert_eq!(
            api.handle(&request("OPTIONS", "/api/frame", "")).status,
            204
        );
    }
}
