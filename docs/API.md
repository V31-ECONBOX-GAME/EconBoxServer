# HTTP API

Base URL: `http://<addr>` — `http://127.0.0.1:8080` by default.

- Request bodies are JSON. An empty body means "no overrides", so
  `POST /api/step` with no body advances one tick.
- Response bodies are JSON, except `GET /` which serves the reference client.
- `Access-Control-Allow-Origin: *` is sent on every response, and `OPTIONS`
  preflights are answered with `204`, so a browser client may live on another
  origin.
- Every response carries `Cache-Control: no-store`. Frames are never reusable.
- Connections are keep-alive by default (HTTP/1.1 with `Content-Length`). A
  frame loop should reuse one connection.

All examples below are real responses from a default server.

## Errors

Every failure has the same shape:

```json
{ "error": "invalid JSON body: expected '\"' at byte 1", "status": 400 }
```

| Status | When                                                              |
| ------ | ----------------------------------------------------------------- |
| `400`  | Malformed request or unparseable JSON body                        |
| `404`  | Unknown path                                                      |
| `405`  | Known path, wrong method                                          |
| `413`  | Headers over 16 KB or body over 1 MB                              |
| `501`  | Something unimplemented, such as `Transfer-Encoding: chunked`     |

Out-of-range *values* are not errors. Frame sizes and simulation parameters are
clamped into their valid ranges, so a client can pass its raw window size or a
slider value without validating first.

---

## `GET /api/health`

Liveness, versions, and enough state to confirm you are talking to the world you
think you are.

```console
$ curl -s localhost:8080/api/health
{"status":"ok","name":"econbox-server","version":"0.1.0","protocol":1,
 "uptime_ms":17,"tick":0,"agents":240}
```

`protocol` is the frame format version — see
[RENDERING.md](RENDERING.md#versioning). A client should check it once at
startup.

---

## `GET /api/state`

The whole world except the per-agent detail.

```console
$ curl -s localhost:8080/api/state
{"tick":120,"price":0.9,"seed":42,"agents":240,
 "params":{"price_flex":0.08,"tax_rate":0,"stock_target":4,
           "spend_fraction":0.35,"shock":0.15},
 "totals":{"cash":24205.3,"goods":1110.72,"gini":0.1625},
 "last":{"tick":120,"price":0.86,"demand":37.57,"supply":7.99,"volume":7.99,
         "stock":1110.72,"cash":24205.3,"gini":0.1629,"unmet":0}}
```

| Field    | Meaning                                                            |
| -------- | ------------------------------------------------------------------ |
| `tick`   | Ticks elapsed since the world was built                            |
| `price`  | The price the *next* tick will trade at                            |
| `seed`   | The seed this world was built from                                 |
| `params` | Current parameters, as returned by `/api/params`                   |
| `totals` | Aggregate cash, aggregate goods, and the Gini coefficient of wealth |
| `last`   | The most recent recorded tick, or `null` before the first step      |

A `last` sample is described in
[SIMULATION.md](SIMULATION.md#what-gets-recorded). Note that `last.price` is the
price that tick *traded* at, while the top-level `price` is the one the next
tick will use.

---

## `GET /api/history`

The recorded tail, oldest first. Use it to plot with your own charting code
instead of the server's panels.

| Query   | Default | Range      |
| ------- | ------- | ---------- |
| `limit` | `256`   | `1..=1024` |

```console
$ curl -s 'localhost:8080/api/history?limit=2'
{"tick":120,"samples":[
  {"tick":119,"price":0.91,"demand":6.03,"supply":38.76,"volume":6.03,
   "stock":1106.37,"cash":24205.3,"gini":0.1625,"unmet":0},
  {"tick":120,"price":0.86,"demand":37.57,"supply":7.99,"volume":7.99,
   "stock":1110.72,"cash":24205.3,"gini":0.1629,"unmet":0}]}
```

The server keeps the last 1024 ticks. Older ones are dropped, not archived.

---

## `GET /api/frame` and `POST /api/frame`

The endpoint EconBox calls every frame. Returns a complete picture: a size, a
background colour, and a list of drawing commands in pixel coordinates. The
format is specified in **[RENDERING.md](RENDERING.md)**.

| Field     | Where                | Default | Notes                              |
| --------- | -------------------- | ------- | ---------------------------------- |
| `width`   | query or body        | `1280`  | Clamped to `320..=8192`            |
| `height`  | query or body        | `720`   | Clamped to `240..=8192`            |
| `advance` | body, **`POST` only**| `0`     | Ticks to run first, max `10000`    |

`GET` never changes the world, so it is safe to poll, cache or open in a browser.
`POST` may advance it, which lets a client run its whole loop in one round trip:

```console
$ curl -s -X POST localhost:8080/api/frame -d '{"width":400,"height":300,"advance":1}'
{"protocol":1,"tick":120,"width":400,"height":300,"background":"#0d1017",
 "ops":[{"op":"rect","x":12,"y":12,"w":376,"h":40,"fill":"#151922","radius":6},
        {"op":"text","x":24,"y":37,"text":"EconBox","size":17,
         "fill":"#c9d1d9","align":"left"},
        ...]}
```

A 1280x720 frame is typically 20-80 KB of JSON: about sixty ops for the panels
and labels, one circle per agent drawn (capped at 1200), and one point per
recorded tick in each chart.

If a query parameter and a body field give the same key, the query parameter
wins.

---

## `POST /api/step`

Advance the world without rendering. For headless clients, batch runs and tests.

| Field   | Default | Range        |
| ------- | ------- | ------------ |
| `ticks` | `1`     | `0..=10000`  |

```console
$ curl -s -X POST localhost:8080/api/step -d '{"ticks":1}'
{"tick":121,"price":0.84,"seed":42,"agents":240, ... ,"stepped":1}
```

The response is the same object as `/api/state`, plus `stepped` — the number of
ticks that actually ran after clamping.

To run more than 10 000 ticks, call it repeatedly. The cap exists because one
request holds the world lock for its whole duration.

---

## `GET /api/params` and `POST /api/params`

Read or patch the simulation parameters. `POST` applies only the fields present,
clamps every one of them, and returns the full set.

```console
$ curl -s -X POST localhost:8080/api/params -d '{"tax_rate":0.01}'
{"price_flex":0.08,"tax_rate":0.01,"stock_target":4,
 "spend_fraction":0.35,"shock":0.15}
```

| Field            | Default | Range     | Effect                                        |
| ---------------- | ------- | --------- | --------------------------------------------- |
| `price_flex`     | `0.08`  | `0..=1`   | How fast the price reacts to excess demand    |
| `tax_rate`       | `0`     | `0..=0.5` | Share of each agent's cash taxed **per tick** and paid back as an equal dividend |
| `stock_target`   | `4`     | `0..=50`  | Desired inventory, in ticks of consumption    |
| `spend_fraction` | `0.35`  | `0..=1`   | Most of its cash an agent commits per tick    |
| `shock`          | `0.15`  | `0..=2`   | Size of the productivity shocks               |

`tax_rate` is charged every tick, so it is a strong lever: at `0.01` the spread
in wealth halves roughly every 70 ticks. Values are documented in detail in
[SIMULATION.md](SIMULATION.md#parameters).

Parameters survive a reset unless the reset supplies new ones.

---

## `POST /api/reset`

Throw the world away and build a fresh one. Both fields are optional; each
defaults to what the current world uses, so `{}` restarts the same run.

| Field    | Default        | Range        |
| -------- | -------------- | ------------ |
| `seed`   | current seed   | any `u64`    |
| `agents` | current count  | `1..=20000`  |
| `params` | current params | a `params` object, patched as above |

```console
$ curl -s -X POST localhost:8080/api/reset -d '{"seed":7,"agents":100}'
{"tick":0,"price":1,"seed":7,"agents":100,
 "params":{"price_flex":0.08,"tax_rate":0,"stock_target":4,
           "spend_fraction":0.35,"shock":0.15},
 "totals":{"cash":9642.69,"goods":411.92,"gini":0.1543},"last":null}
```

The history is cleared, the tick returns to zero, and the price returns to 1.

---

## `GET /`

The reference client, compiled into the binary from `web/index.html`. It is a
convenience for `cargo run`, not part of the API — a real EconBox client talks
only to `/api/*`.

---

## A worked session

```bash
S=http://127.0.0.1:8080

curl -s $S/api/health                                        # is it there?
curl -s -X POST $S/api/reset  -d '{"seed":7,"agents":500}'   # a known world
curl -s -X POST $S/api/step   -d '{"ticks":500}'             # warm it up
curl -s -X POST $S/api/params -d '{"tax_rate":0.01}'         # tax the rich
curl -s -X POST $S/api/step   -d '{"ticks":500}'             # let it settle
curl -s $S/api/state | grep -o '"gini":[0-9.]*'              # did it work?
curl -s -X POST $S/api/frame  -d '{"width":1280,"height":720}' > frame.json
```

Because the world is deterministic, that exact sequence gives the exact same
numbers on any machine.
