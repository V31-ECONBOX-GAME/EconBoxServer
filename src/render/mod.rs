//! Frame construction.
//!
//! The client never computes a layout, a scale or a colour: it receives a flat
//! list of drawing commands in pixel coordinates and paints them in order.
//! `docs/RENDERING.md` is the normative description of this format.

pub mod scene;

pub use scene::build;

use crate::json::Json;

/// Version of the frame format. Bumped whenever an op changes meaning, so a
/// client can refuse a server it does not understand.
pub const PROTOCOL_VERSION: u32 = 1;

/// Horizontal anchor of a [`Op::Text`] command.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Align {
    Left,
    Center,
    Right,
}

impl Align {
    fn as_str(self) -> &'static str {
        match self {
            Align::Left => "left",
            Align::Center => "center",
            Align::Right => "right",
        }
    }
}

/// One drawing command. Coordinates are pixels from the top-left corner of the
/// frame; colours are `#rrggbb` strings.
#[derive(Debug, Clone, PartialEq)]
pub enum Op {
    Rect {
        x: f64,
        y: f64,
        w: f64,
        h: f64,
        fill: String,
        radius: f64,
    },
    Line {
        x1: f64,
        y1: f64,
        x2: f64,
        y2: f64,
        stroke: String,
        width: f64,
    },
    Circle {
        x: f64,
        y: f64,
        r: f64,
        fill: String,
    },
    /// An open path. Points are flattened to `[x0, y0, x1, y1, ...]` on the
    /// wire because a frame can carry a thousand of them.
    Polyline {
        points: Vec<(f64, f64)>,
        stroke: String,
        width: f64,
    },
    Text {
        x: f64,
        y: f64,
        text: String,
        size: f64,
        fill: String,
        align: Align,
    },
}

impl Op {
    pub fn to_json(&self) -> Json {
        match self {
            Op::Rect {
                x,
                y,
                w,
                h,
                fill,
                radius,
            } => Json::obj([
                ("op", Json::str("rect")),
                ("x", Json::num2(*x)),
                ("y", Json::num2(*y)),
                ("w", Json::num2(*w)),
                ("h", Json::num2(*h)),
                ("fill", Json::str(fill.clone())),
                ("radius", Json::num2(*radius)),
            ]),
            Op::Line {
                x1,
                y1,
                x2,
                y2,
                stroke,
                width,
            } => Json::obj([
                ("op", Json::str("line")),
                ("x1", Json::num2(*x1)),
                ("y1", Json::num2(*y1)),
                ("x2", Json::num2(*x2)),
                ("y2", Json::num2(*y2)),
                ("stroke", Json::str(stroke.clone())),
                ("width", Json::num2(*width)),
            ]),
            Op::Circle { x, y, r, fill } => Json::obj([
                ("op", Json::str("circle")),
                ("x", Json::num2(*x)),
                ("y", Json::num2(*y)),
                ("r", Json::num2(*r)),
                ("fill", Json::str(fill.clone())),
            ]),
            Op::Polyline {
                points,
                stroke,
                width,
            } => Json::obj([
                ("op", Json::str("polyline")),
                (
                    "points",
                    Json::arr(
                        points
                            .iter()
                            .flat_map(|(x, y)| [Json::num2(*x), Json::num2(*y)]),
                    ),
                ),
                ("stroke", Json::str(stroke.clone())),
                ("width", Json::num2(*width)),
            ]),
            Op::Text {
                x,
                y,
                text,
                size,
                fill,
                align,
            } => Json::obj([
                ("op", Json::str("text")),
                ("x", Json::num2(*x)),
                ("y", Json::num2(*y)),
                ("text", Json::str(text.clone())),
                ("size", Json::num2(*size)),
                ("fill", Json::str(fill.clone())),
                ("align", Json::str(align.as_str())),
            ]),
        }
    }
}

/// A complete frame: everything needed to paint one image.
#[derive(Debug, Clone, PartialEq)]
pub struct Scene {
    pub tick: u64,
    pub width: f64,
    pub height: f64,
    pub background: String,
    pub ops: Vec<Op>,
}

impl Scene {
    pub fn to_json(&self) -> Json {
        Json::obj([
            ("protocol", Json::num(PROTOCOL_VERSION as f64)),
            ("tick", Json::num(self.tick as f64)),
            ("width", Json::num2(self.width)),
            ("height", Json::num2(self.height)),
            ("background", Json::str(self.background.clone())),
            ("ops", Json::arr(self.ops.iter().map(|op| op.to_json()))),
        ])
    }
}

/// An axis-aligned box used while laying the frame out. Never leaves the
/// server: only the resulting ops do.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

impl Rect {
    pub fn new(x: f64, y: f64, w: f64, h: f64) -> Rect {
        Rect {
            x,
            y,
            w: w.max(0.0),
            h: h.max(0.0),
        }
    }

    /// Shrink on every side by `d`.
    pub fn inset(&self, d: f64) -> Rect {
        Rect::new(self.x + d, self.y + d, self.w - 2.0 * d, self.h - 2.0 * d)
    }

    pub fn right(&self) -> f64 {
        self.x + self.w
    }

    pub fn bottom(&self) -> f64 {
        self.y + self.h
    }
}
