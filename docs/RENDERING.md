# The frame protocol

A frame is a complete picture with no state carried over from the previous one.
The client paints the ops in order — painter's algorithm, later ops on top — and
then throws them away.

Get one from `GET /api/frame?width=W&height=H`, or from `POST /api/frame` with
`{"width":W,"height":H,"advance":N}`.

## The frame object

```json
{
  "protocol": 1,
  "tick": 120,
  "width": 1280,
  "height": 720,
  "background": "#0d1017",
  "ops": [ ... ]
}
```

| Field        | Type   | Meaning                                                  |
| ------------ | ------ | -------------------------------------------------------- |
| `protocol`   | number | Frame format version. See [Versioning](#versioning).     |
| `tick`       | number | The world's tick when this frame was drawn               |
| `width`      | number | Frame width in pixels, after clamping                    |
| `height`     | number | Frame height in pixels, after clamping                   |
| `background` | string | Fill the whole frame with this before painting the ops   |
| `ops`        | array  | Drawing commands, in paint order                         |

`width` and `height` are the size the server actually used. Ask for `0x0` and
you get `320x240`; ask for `99999` and you get `8192`. Always lay out against
the returned values, never the requested ones.

## Coordinates and units

- The origin is the **top-left** corner. `x` grows right, `y` grows down.
- Units are **CSS pixels** — logical, not physical. On a high-DPI display,
  request the logical size and scale the canvas yourself; do not multiply the
  requested size by the device pixel ratio, or the text will come back tiny.
- Numbers are rounded to two decimals. Nothing meaningful is lost, and frames
  shrink noticeably.
- All coordinates are absolute. There are no transforms, no groups, no clipping
  regions and no coordinate stack to maintain.
- Ops may extend past the frame edges; clip to the frame.

## Colours

Always a `#rrggbb` string. No named colours, no alpha, no gradients. If your
renderer needs another form, convert it in your painting function.

## The ops

### `rect`

```json
{"op":"rect","x":12,"y":12,"w":376,"h":40,"fill":"#151922","radius":6}
```

A filled rectangle. `radius` is the corner radius in pixels; `0` means square
corners. A renderer without rounded-rectangle support may ignore `radius` — the
result is still legible.

### `line`

```json
{"op":"line","x1":22,"y1":100,"x2":380,"y2":100,"stroke":"#232936","width":1}
```

A single stroked segment. `width` is in pixels.

### `circle`

```json
{"op":"circle","x":640,"y":360,"r":2.5,"fill":"#e06c75"}
```

A filled circle centred on `(x, y)`.

### `polyline`

```json
{"op":"polyline","points":[10,50,11,48,12,53],"stroke":"#4cc38a","width":2}
```

An open stroked path. **`points` is flat**: `[x0, y0, x1, y1, ...]`, because a
frame can carry a thousand of them and nesting each pair doubles the bytes.
Never closed, never filled. Round joins look best but are not required.

### `text`

```json
{"op":"text","x":24,"y":37,"text":"EconBox","size":17,"fill":"#c9d1d9","align":"left"}
```

A single line of text.

- `y` is the **alphabetic baseline**, not the top of the glyphs.
- `align` is `"left"`, `"center"` or `"right"`, and anchors horizontally at `x`.
- `size` is the font size in pixels. **The font family is the client's choice.**
  The server assumes something monospaced when it budgets space; a proportional
  font still reads fine, it just uses the room differently.
- Text is never rotated and never wraps. Multi-line labels arrive as several
  ops.

## Writing a client

The whole contract, in one function per op:

```js
const painters = {
  rect(ctx, op) {
    ctx.fillStyle = op.fill;
    if (op.radius > 0 && ctx.roundRect) {
      ctx.beginPath();
      ctx.roundRect(op.x, op.y, op.w, op.h, op.radius);
      ctx.fill();
    } else {
      ctx.fillRect(op.x, op.y, op.w, op.h);
    }
  },
  line(ctx, op) {
    ctx.strokeStyle = op.stroke;
    ctx.lineWidth = op.width;
    ctx.beginPath();
    ctx.moveTo(op.x1, op.y1);
    ctx.lineTo(op.x2, op.y2);
    ctx.stroke();
  },
  circle(ctx, op) {
    ctx.fillStyle = op.fill;
    ctx.beginPath();
    ctx.arc(op.x, op.y, op.r, 0, Math.PI * 2);
    ctx.fill();
  },
  polyline(ctx, op) {
    ctx.strokeStyle = op.stroke;
    ctx.lineWidth = op.width;
    ctx.beginPath();
    for (let i = 0; i + 1 < op.points.length; i += 2) {
      if (i === 0) ctx.moveTo(op.points[i], op.points[i + 1]);
      else ctx.lineTo(op.points[i], op.points[i + 1]);
    }
    ctx.stroke();
  },
  text(ctx, op) {
    ctx.fillStyle = op.fill;
    ctx.textAlign = op.align;
    ctx.font = op.size + 'px ui-monospace, monospace';
    ctx.fillText(op.text, op.x, op.y);
  },
};

function paint(ctx, scene) {
  ctx.fillStyle = scene.background;
  ctx.fillRect(0, 0, scene.width, scene.height);
  for (const op of scene.ops) painters[op.op]?.(ctx, op);
}
```

[`web/index.html`](../web/index.html) is this plus a render loop and four
controls. It is the reference implementation; read it before writing your own.

### High-DPI displays

Ask for the **logical** size, then scale the drawing surface:

```js
const dpr = window.devicePixelRatio || 1;
canvas.width = scene.width * dpr;         // physical pixels
canvas.height = scene.height * dpr;
canvas.style.width = scene.width + 'px';  // logical pixels
canvas.style.height = scene.height + 'px';
ctx.setTransform(dpr, 0, 0, dpr, 0, 0);   // then paint in logical coordinates
```

### Frame loops

One request per frame, with one in flight at a time:

```js
let inFlight = false;
async function tick() {
  if (inFlight) return;            // never queue frames behind a slow one
  inFlight = true;
  try {
    const scene = await post('/api/frame', { width, height, advance: 1 });
    paint(ctx, scene);
  } finally {
    inFlight = false;
  }
}
setInterval(tick, 16);             // or requestAnimationFrame
```

Reuse the connection — the server speaks HTTP/1.1 keep-alive, and reconnecting
per frame costs more than the frame does. On localhost a full round trip,
including advancing the world and parsing the JSON, costs about 5 ms, so the
frame rate is bounded by your renderer rather than by the server.

`requestAnimationFrame` does not fire in a hidden tab, so a client driven by it
pauses when it is not visible. That is usually what you want; use `setInterval`
if the world should keep advancing in the background.

If your client wants its own HUD with its own fonts and numbers, call
`/api/state` alongside the frame rather than trying to parse text ops.

### Other renderers

The op set is deliberately the intersection of what every 2D renderer can do:
`fillRect`, `drawLine`, `fillCircle`, `drawPolyline`, `drawText`. SDL2, Skia,
Cairo, raylib, an SVG string builder and a terminal renderer are all a few dozen
lines. There is no compositing, no alpha, no blend mode and no transform to
emulate.

## What the server decides, and what you decide

| The server decides                       | The client decides                   |
| ---------------------------------------- | ------------------------------------ |
| Layout, panels, what is on screen        | Window size, and when to ask         |
| Axis ranges and tick labels              | Font family                          |
| Every colour, including the background   | Device pixel ratio handling          |
| Text content, size and alignment         | Whether to run, pause or single-step |
| How many agents are worth drawing        | Input, controls and shortcuts        |

If you find yourself computing a scale or picking a colour in a client, that
logic belongs in `src/render/scene.rs` instead — put it there and every client
gets it.

## Responsiveness

The server adapts the layout to the size you ask for:

- 720 px wide or more: four panels in a two by two grid.
- Narrower: a single column.
- Too short for four readable panels: fewer panels, in priority order — price
  first, then flows, then the wealth histogram, then the agent scatter. A short
  frame gets one real chart rather than four boxes too small to draw in.

Clients get all of that for free. Send the real viewport size on every frame and
lay out against the `width` and `height` that come back.

## Frame size and cost

A 1280x720 frame is typically 20-80 KB of JSON: about sixty ops for the panels
and labels, one circle per agent drawn, and one point per recorded tick in each
chart. The two things that move that number are how much history exists (up to
1024 points in a single polyline) and how many agents are on screen.

The scatter panel is capped at **1200 points per frame**. Above that the server
samples the population at a fixed stride and says so on the panel, so a 20 000
agent world costs the same to draw as a 1200 agent one.

## Versioning

`protocol` starts at `1` and increases when an op changes meaning or a field is
removed. Adding a new op or a new optional field does **not** bump it.

A client should therefore:

- check `protocol` once at startup and refuse a version it does not know;
- ignore ops whose `op` it does not recognise, rather than failing;
- ignore unknown fields on ops it does recognise.

Following those three rules means new panels and new statistics reach your
client with no work at all.
