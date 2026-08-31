# Architecture

## The one rule

**The server owns every number. The client owns every pixel.**

Everything else follows from that. Layout, axis ranges, bar heights, colours,
label text and label positions are all computed on the server and shipped as
absolute pixel coordinates. A client that can fill a rectangle, stroke a line,
fill a circle, stroke a path and draw a string is a complete EconBox client.

Why put the whole layout on the server:

- **Clients stay disposable.** A new platform costs five painting functions, not
  a port of the model.
- **Every client looks identical.** There is no second implementation of the
  layout to drift.
- **The interesting code stays in one language.** Change a colour, a panel or
  the economy in `src/`, and every client picks it up on its next frame.

The cost is bandwidth and latency: a frame is JSON, roughly 20-60 KB for a
1280x720 view, and every frame is a round trip. On a local socket that is
comfortably 60 fps. Over a wide-area link it is not, and you would want the
streaming or binary formats sketched under [Extension points](#extension-points).

## Request flow, end to end

A client posts to `/api/frame` with a viewport size and a number of ticks to
advance:

1. `http::server` accepts the connection and hands it to a worker thread.
2. `http::request` parses the request line, headers and body against fixed size
   limits.
3. `api::Api::handle` matches the method and path, and parses the JSON body.
4. The handler takes the world lock — a single `Mutex<World>`.
5. `sim::World::step` runs the requested number of ticks. This is the only code
   that mutates simulation state.
6. `render::build` reads the world and produces a `Scene`: a background colour
   and a flat `Vec<Op>` in pixel coordinates.
7. `Scene::to_json` serialises it; `http::response` writes it with a
   `Content-Length`, CORS headers and `Connection: keep-alive`.
8. The lock is released when the handler returns.

## Module map

Bottom up, each module depending only on the ones above it:

| Module        | Responsibility                                          | Knows about        |
| ------------- | ------------------------------------------------------- | ------------------ |
| `rng`         | Deterministic random numbers (SplitMix64)               | nothing            |
| `json`        | A JSON value, a serializer and a parser                 | nothing            |
| `http`        | HTTP/1.1 parsing, responses, thread pool, accept loop   | `json`             |
| `sim`         | Agents, market clearing, the world and its tick         | `rng`, `json`      |
| `render`      | World state to drawing commands                         | `sim`, `json`      |
| `api`         | Routing, request validation, one handler per endpoint   | all of the above   |
| `config`      | Defaults, environment, command line                     | nothing            |

`sim` does not know that HTTP exists. `render` does not know that HTTP exists.
That is what makes them easy to reuse from a test, a benchmark, or a different
transport.

### `sim`

- `sim::Params` — the runtime knobs, with a `sanitize` that clamps every field.
  Client input is never trusted into the model unclamped.
- `sim::Agent` — cash, goods, productivity, need, the order it last placed.
- `sim::market` — pure functions: `clear` matches aggregate demand against
  aggregate supply, `next_price` moves the price toward balance. No state, so
  they are directly testable.
- `sim::World` — the state and `step`, which is the only mutator. Also the
  history ring buffer and the aggregate statistics.

### `render`

- `render::Op` — the drawing command enum, and its JSON encoding.
- `render::Scene` — a frame: size, background, ops.
- `render::scene::build` — the layout. Every colour constant and every panel
  lives at the top of this file. **This is the file to edit to change how
  EconBox looks.**

## Concurrency

- A fixed pool of worker threads (`http::pool`), sized from `--workers`.
- The queue in front of the pool is bounded at four connections per worker, so a
  flood of connections becomes backpressure on `accept` rather than unbounded
  memory growth.
- A panicking handler is caught per job: the worker logs and keeps serving.
- All simulation state sits behind one `Mutex<World>`. Handlers hold it for the
  duration of their work, so requests are effectively serialised against each
  other.
- If a handler panics while holding the lock, the mutex is poisoned; `Api::world`
  recovers the guard instead of failing every later request. The world is
  structurally valid either way — a panic mid-tick can only leave it in a state
  that is odd, not one that is unsound.

One global lock is the right trade here because a tick is short and the whole
point is that all clients watch the *same* world. If you need many independent
worlds, see the sessions sketch below.

## Determinism

The same seed and the same sequence of API calls always produce exactly the same
world:

- The generator is seeded explicitly and stored inside the world. No system
  entropy, no clock, no hashing order.
- Nothing in `step` iterates over an unordered collection.
- `f64` arithmetic runs in a fixed order.

This is not incidental — it is what makes bugs reproducible and the tests in
`tests/economy.rs` meaningful. If you add state, seed it from `World::rng`.

Note that determinism is per *call sequence*, not per wall-clock time. Two
clients advancing the same server interleave their ticks; that is a property of
sharing one world, not a bug.

## Limits

Every one of these exists so that a client cannot make the server do unbounded
work. They live next to the code they protect.

| Limit                      | Value             | Where                     |
| -------------------------- | ----------------- | ------------------------- |
| Request line plus headers  | 16 KB             | `http::request`           |
| Request body               | 1 MB              | `http::request`           |
| Read and write timeout     | 30 s              | `http::server`            |
| Queued connections         | 4 per worker      | `http::pool`              |
| Ticks per request          | 10 000            | `api`                     |
| Agents                     | 1 to 20 000       | `sim::world`              |
| History kept               | 1024 ticks        | `sim::world`              |
| Frame size                 | 320x240 to 8192   | `render::scene`           |
| Scatter points per frame   | 1200              | `render::scene`           |
| Parameter ranges           | per field         | `sim::Params::sanitize`   |

Out-of-range frame sizes and parameters are **clamped, not rejected**, so a
client can pass its raw window size without validating it first. Malformed JSON
and unknown routes are rejected, because those are mistakes rather than
preferences.

## Deliberate omissions

Things this server does not do, and what to do instead:

- **TLS, authentication, rate limiting.** Bind to `127.0.0.1` (the default) or
  put a reverse proxy in front of it.
- **Multiple worlds or sessions.** One process, one world. See below.
- **Persistence.** The world lives in memory and dies with the process. The seed
  plus a tick count reproduces any run, which covers most of what saving would.
- **Chunked transfer encoding.** Refused with `501` rather than mis-parsed.
- **HTTP/2, pipelining, compression.** Keep-alive with `Content-Length` is
  enough for a frame loop on a local socket. `Content-Encoding: gzip` would be
  the first thing to add for a remote client — frames compress very well.
- **Dependencies.** HTTP, JSON and the RNG are hand-written. Each is small
  enough to read in one sitting, which is the point: this is a codebase meant to
  be modified. Adding a crate should be a decision, not a habit.

## Extension points

**Multiple sessions.** Replace `Api`'s `Mutex<World>` with a
`Mutex<HashMap<String, Mutex<World>>>`, take a session id from a path segment or
header, and add a create/destroy endpoint. Handlers already do nothing but "lock,
mutate, render", so they change one line each. Add an idle timeout so abandoned
sessions do not accumulate.

**Streaming instead of polling.** The current model is one request per frame.
For a smoother remote experience, add a Server-Sent Events endpoint that pushes a
frame per tick: keep the connection open, write `data: {...}\n\n` per frame. The
`http` module writes responses with an explicit `Content-Length` today, so this
means adding a streaming response variant.

**A smaller frame.** Wins in order of payoff: gzip the body; downsample chart
series to the plot width, since a 1024-point polyline in a 360 px panel is
mostly redundant (use min/max per pixel column, not a plain stride, or spikes
will alias away); send only the ops that changed since the client's last frame;
replace JSON with a compact binary encoding. The op set was designed so a binary encoding is
mechanical — every op is a tag plus a fixed set of numbers.

**New drawing ops.** Add a variant to `render::Op`, encode it in `Op::to_json`,
paint it in every client, and bump `render::PROTOCOL_VERSION`. See
[RENDERING.md](RENDERING.md#versioning).

**A different picture.** Everything visual is in `render::scene`: the palette
constants at the top, `grid` for the layout, and one function per panel. Adding a
panel is writing one function and one call.

**A richer economy.** `sim::World::step` is a numbered seven-step sequence.
Multiple goods means widening `price` and the order arrays; firms and households
means splitting `Agent` into two roles with different rules in step 2. The
invariants worth preserving are in [SIMULATION.md](SIMULATION.md#invariants).

**A different transport.** `sim` and `render` have no idea HTTP exists. A
WebSocket, a Unix socket or an FFI boundary can call `World::step` and
`render::build` directly.
