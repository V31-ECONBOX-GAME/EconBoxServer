//! Turning a world into a frame.
//!
//! Every number a client would otherwise have to work out — axis ranges, bar
//! heights, colours, label positions — is decided here. Changing how EconBox
//! looks means editing this file and nothing on the client.

use crate::render::{Align, Op, Rect, Scene};
use crate::sim::World;

/// Palette. One place to restyle the whole product.
const BG: &str = "#0d1017";
const PANEL: &str = "#151922";
const GRID: &str = "#232936";
const TEXT: &str = "#c9d1d9";
const MUTED: &str = "#6b7683";
const PRICE: &str = "#4cc38a";
const DEMAND: &str = "#e06c75";
const SUPPLY: &str = "#61afef";
const WEALTH: &str = "#e5c07b";

/// Frame sizes the server accepts. Requests outside the range are clamped
/// rather than rejected, so a client can pass its raw window size.
pub const MIN_WIDTH: f64 = 320.0;
pub const MIN_HEIGHT: f64 = 240.0;
pub const MAX_SIDE: f64 = 8192.0;

/// Upper bound on scatter points in one frame. A 20 000 agent world would
/// otherwise produce a frame no browser can paint at 60 fps, so the population
/// is sampled at a fixed stride instead.
const MAX_POINTS: usize = 1200;

/// Number of buckets in the wealth histogram.
const BUCKETS: usize = 24;

/// How many recent ticks the demand/supply panel shows.
const FLOW_WINDOW: usize = 200;

/// Shortest panel that still holds a readable chart. Below this the layout
/// drops panels rather than squashing every one of them into a title bar.
const MIN_PANEL: f64 = 96.0;

/// Build the frame for `world` at the requested pixel size.
pub fn build(world: &World, width: f64, height: f64) -> Scene {
    let width = clamp_side(width, MIN_WIDTH);
    let height = clamp_side(height, MIN_HEIGHT);
    let pad = 12.0;
    let canvas = Rect::new(0.0, 0.0, width, height).inset(pad);
    let mut ops = Vec::new();

    let header = Rect::new(canvas.x, canvas.y, canvas.w, 40.0);
    header_bar(&mut ops, world, header);

    let footer_height = 16.0;
    let body = Rect::new(
        canvas.x,
        header.bottom() + pad,
        canvas.w,
        canvas.h - header.h - footer_height - 2.0 * pad,
    );
    // Panels in priority order: a frame too small for all four keeps the ones
    // that matter most.
    let panels: [fn(&mut Vec<Op>, &World, Rect); 4] =
        [price_panel, flow_panel, wealth_panel, agents_panel];
    for (draw, cell) in panels.iter().zip(grid(body, pad)) {
        draw(&mut ops, world, cell);
    }

    ops.push(text(
        canvas.x,
        canvas.bottom(),
        format!(
            "seed {} - {} agents - tax {:.1}%/tick - flex {:.2} - rendered by econbox-server",
            world.seed,
            world.agents.len(),
            world.params.tax_rate * 100.0,
            world.params.price_flex
        ),
        10.0,
        MUTED,
        Align::Left,
    ));

    Scene {
        tick: world.tick,
        width,
        height,
        background: BG.to_string(),
        ops,
    }
}

/// Up to four panels: two by two when the frame is wide, a single column when
/// it is narrow, and fewer rows when it is short. Returns one rectangle per
/// panel that fits, so a small frame shows a few readable panels instead of
/// four unreadable ones.
fn grid(body: Rect, gap: f64) -> Vec<Rect> {
    let columns = if body.w >= 720.0 { 2 } else { 1 };
    let fits = ((body.h + gap) / (MIN_PANEL + gap)).floor().max(1.0) as usize;
    let rows = fits.min(4 / columns);
    let width = (body.w - gap * (columns - 1) as f64) / columns as f64;
    let height = (body.h - gap * (rows - 1) as f64) / rows as f64;
    (0..rows * columns)
        .map(|i| {
            let (column, row) = (i % columns, i / columns);
            Rect::new(
                body.x + column as f64 * (width + gap),
                body.y + row as f64 * (height + gap),
                width,
                height,
            )
        })
        .collect()
}

fn header_bar(ops: &mut Vec<Op>, world: &World, r: Rect) {
    ops.push(rect(r, PANEL, 6.0));
    ops.push(text(
        r.x + 12.0,
        r.y + 25.0,
        "EconBox",
        17.0,
        TEXT,
        Align::Left,
    ));

    let last = world.last();
    let stats = [
        ("TICK", format!("{}", world.tick)),
        ("PRICE", fmt(world.price)),
        ("VOLUME", fmt(last.map(|s| s.volume).unwrap_or(0.0))),
        ("GINI", format!("{:.3}", world.gini(world.price))),
        ("UNMET", fmt(last.map(|s| s.unmet).unwrap_or(0.0))),
    ];
    // Lay the chips out from the right edge so the title never collides.
    let chip = 78.0;
    let mut x = r.right() - 12.0;
    for (label, value) in stats.iter().rev() {
        if x - chip < r.x + 90.0 {
            break;
        }
        ops.push(text(x, r.y + 16.0, *label, 9.0, MUTED, Align::Right));
        ops.push(text(x, r.y + 31.0, value.clone(), 14.0, TEXT, Align::Right));
        x -= chip;
    }
}

fn price_panel(ops: &mut Vec<Op>, world: &World, r: Rect) {
    let plot = panel(ops, r, "PRICE", &fmt(world.price));
    let series: Vec<f64> = world.history.iter().map(|s| s.price).collect();
    let (lo, hi) = range_of(&series);
    axis(ops, plot, lo, hi);
    if let Some(op) = series_line(plot, &series, lo, hi, PRICE, 2.0) {
        ops.push(op);
    }
}

fn flow_panel(ops: &mut Vec<Op>, world: &World, r: Rect) {
    let plot = panel(ops, r, "DEMAND vs SUPPLY", "last 200 ticks");
    // Flows swing hard from tick to tick; the full history would be a smear.
    let recent = world.history.len().saturating_sub(FLOW_WINDOW);
    let demand: Vec<f64> = world
        .history
        .iter()
        .skip(recent)
        .map(|s| s.demand)
        .collect();
    let supply: Vec<f64> = world
        .history
        .iter()
        .skip(recent)
        .map(|s| s.supply)
        .collect();
    // A shared scale is what makes the gap between the two lines readable.
    let (lo, hi) = range_of_all(&[&demand, &supply]);
    axis(ops, plot, lo, hi);
    if let Some(op) = series_line(plot, &supply, lo, hi, SUPPLY, 1.5) {
        ops.push(op);
    }
    if let Some(op) = series_line(plot, &demand, lo, hi, DEMAND, 1.5) {
        ops.push(op);
    }
    if plot.w > 140.0 {
        ops.push(text(
            plot.right(),
            plot.y + 9.0,
            "demand",
            9.0,
            DEMAND,
            Align::Right,
        ));
        ops.push(text(
            plot.right(),
            plot.y + 21.0,
            "supply",
            9.0,
            SUPPLY,
            Align::Right,
        ));
    }
}

fn wealth_panel(ops: &mut Vec<Op>, world: &World, r: Rect) {
    let plot = panel(ops, r, "WEALTH DISTRIBUTION", "");
    if plot.w < 8.0 || plot.h < 8.0 {
        return;
    }
    let price = world.price;
    let wealth: Vec<f64> = world
        .agents
        .iter()
        .map(|a| a.wealth(price).max(0.0))
        .collect();
    let top = wealth.iter().cloned().fold(0.0_f64, f64::max);
    if top <= 0.0 {
        return;
    }
    let mut buckets = [0.0_f64; BUCKETS];
    for value in &wealth {
        // `min` guards the richest agent, which would otherwise index past the end.
        let index = ((value / top) * BUCKETS as f64) as usize;
        buckets[index.min(BUCKETS - 1)] += 1.0;
    }
    let tallest = buckets.iter().cloned().fold(1.0_f64, f64::max);
    let step = plot.w / BUCKETS as f64;
    for (i, count) in buckets.iter().enumerate() {
        let h = (count / tallest) * plot.h;
        ops.push(rect(
            Rect::new(
                plot.x + i as f64 * step,
                plot.bottom() - h,
                (step - 2.0).max(1.0),
                h,
            ),
            WEALTH,
            1.0,
        ));
    }
    ops.push(text(
        plot.x,
        plot.bottom() + 10.0,
        "poor",
        9.0,
        MUTED,
        Align::Left,
    ));
    ops.push(text(
        plot.right(),
        plot.bottom() + 10.0,
        fmt(top),
        9.0,
        MUTED,
        Align::Right,
    ));
}

fn agents_panel(ops: &mut Vec<Op>, world: &World, r: Rect) {
    let plot = panel(ops, r, "AGENTS  (stock vs cash)", "");
    if plot.w < 8.0 || plot.h < 8.0 || world.agents.is_empty() {
        return;
    }
    let max_goods = world
        .agents
        .iter()
        .map(|a| a.goods)
        .fold(1e-6_f64, f64::max);
    let max_cash = world.agents.iter().map(|a| a.cash).fold(1e-6_f64, f64::max);
    // Round up, so the cap is never exceeded by the remainder.
    let stride = world.agents.len().div_ceil(MAX_POINTS).max(1);
    for agent in world.agents.iter().step_by(stride) {
        let x = plot.x + (agent.goods / max_goods).clamp(0.0, 1.0) * plot.w;
        let y = plot.bottom() - (agent.cash.max(0.0) / max_cash).clamp(0.0, 1.0) * plot.h;
        // Red agents bid in the last clearing, blue ones offered. Colouring by
        // the order rather than by leftover stock keeps the picture readable:
        // after a clearing the filled side sits exactly on its target, so a
        // stock comparison would flip the whole population at once.
        ops.push(Op::Circle {
            x,
            y,
            r: 2.5,
            fill: (if agent.order > 0.0 { DEMAND } else { SUPPLY }).to_string(),
        });
    }
    ops.push(text(
        plot.x,
        plot.bottom() + 10.0,
        "stock",
        9.0,
        MUTED,
        Align::Left,
    ));
    ops.push(text(
        plot.right(),
        plot.y + 9.0,
        "cash",
        9.0,
        MUTED,
        Align::Right,
    ));
    if stride > 1 {
        ops.push(text(
            plot.x,
            plot.y + 9.0,
            format!("1 of {stride} shown"),
            9.0,
            MUTED,
            Align::Left,
        ));
    }
}

/// Draw a panel background plus its title, and return the area left for content.
fn panel(ops: &mut Vec<Op>, r: Rect, title: &str, note: &str) -> Rect {
    ops.push(rect(r, PANEL, 6.0));
    ops.push(text(
        r.x + 10.0,
        r.y + 16.0,
        title,
        10.0,
        MUTED,
        Align::Left,
    ));
    if !note.is_empty() {
        ops.push(text(
            r.right() - 10.0,
            r.y + 16.0,
            note,
            12.0,
            TEXT,
            Align::Right,
        ));
    }
    Rect::new(r.x + 10.0, r.y + 26.0, r.w - 20.0, r.h - 44.0)
}

/// Horizontal grid lines plus the two extreme values.
fn axis(ops: &mut Vec<Op>, plot: Rect, lo: f64, hi: f64) {
    if plot.w < 4.0 || plot.h < 4.0 {
        return;
    }
    for i in 0..=3 {
        let y = plot.bottom() - (i as f64 / 3.0) * plot.h;
        ops.push(Op::Line {
            x1: plot.x,
            y1: y,
            x2: plot.right(),
            y2: y,
            stroke: GRID.to_string(),
            width: 1.0,
        });
    }
    ops.push(text(
        plot.x + 2.0,
        plot.y + 9.0,
        fmt(hi),
        9.0,
        MUTED,
        Align::Left,
    ));
    ops.push(text(
        plot.x + 2.0,
        plot.bottom() - 3.0,
        fmt(lo),
        9.0,
        MUTED,
        Align::Left,
    ));
}

/// Map a series onto the plot area. `None` when there is nothing to draw.
fn series_line(
    plot: Rect,
    series: &[f64],
    lo: f64,
    hi: f64,
    color: &str,
    width: f64,
) -> Option<Op> {
    if series.len() < 2 || plot.w < 4.0 || plot.h < 4.0 {
        return None;
    }
    let span = (hi - lo).max(f64::EPSILON);
    let last = (series.len() - 1) as f64;
    let points = series
        .iter()
        .enumerate()
        .map(|(i, value)| {
            let x = plot.x + (i as f64 / last) * plot.w;
            let y = plot.bottom() - ((value - lo) / span).clamp(0.0, 1.0) * plot.h;
            (x, y)
        })
        .collect();
    Some(Op::Polyline {
        points,
        stroke: color.to_string(),
        width,
    })
}

/// A padded `(low, high)` range that is never empty and never non-finite.
fn range_of(series: &[f64]) -> (f64, f64) {
    range_of_all(&[series])
}

fn range_of_all(series: &[&[f64]]) -> (f64, f64) {
    let mut lo = f64::INFINITY;
    let mut hi = f64::NEG_INFINITY;
    for values in series {
        for value in values.iter().filter(|v| v.is_finite()) {
            lo = lo.min(*value);
            hi = hi.max(*value);
        }
    }
    if !lo.is_finite() || !hi.is_finite() {
        return (0.0, 1.0);
    }
    let pad = ((hi - lo) * 0.08).max(hi.abs().max(1.0) * 0.02);
    // Quantities that cannot be negative should not get a negative axis.
    let low = if lo >= 0.0 {
        (lo - pad).max(0.0)
    } else {
        lo - pad
    };
    (low, hi + pad)
}

fn clamp_side(value: f64, min: f64) -> f64 {
    if value.is_finite() {
        value.clamp(min, MAX_SIDE).round()
    } else {
        min
    }
}

/// Compact human-readable number for labels.
fn fmt(value: f64) -> String {
    if !value.is_finite() {
        return "n/a".to_string();
    }
    let magnitude = value.abs();
    if magnitude >= 1.0e9 {
        format!("{:.1}B", value / 1.0e9)
    } else if magnitude >= 1.0e6 {
        format!("{:.1}M", value / 1.0e6)
    } else if magnitude >= 1.0e3 {
        format!("{:.1}k", value / 1.0e3)
    } else if magnitude >= 10.0 {
        format!("{value:.1}")
    } else {
        format!("{value:.2}")
    }
}

fn rect(r: Rect, fill: &str, radius: f64) -> Op {
    Op::Rect {
        x: r.x,
        y: r.y,
        w: r.w,
        h: r.h,
        fill: fill.to_string(),
        radius,
    }
}

fn text<S: Into<String>>(x: f64, y: f64, body: S, size: f64, fill: &str, align: Align) -> Op {
    Op::Text {
        x,
        y,
        text: body.into(),
        size,
        fill: fill.to_string(),
        align,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::Params;

    fn scene_at(width: f64, height: f64, ticks: u64) -> Scene {
        let mut world = World::new(5, 120, Params::default());
        world.step_many(ticks);
        build(&world, width, height)
    }

    #[test]
    fn every_coordinate_is_finite() {
        for scene in [scene_at(1280.0, 720.0, 300), scene_at(400.0, 300.0, 1)] {
            let json = scene.to_json().to_string();
            assert!(!json.contains("NaN") && !json.contains("inf"));
            assert!(scene.ops.len() > 10);
        }
    }

    #[test]
    fn degenerate_sizes_do_not_panic() {
        for (w, h) in [(0.0, 0.0), (1.0, 1.0), (f64::NAN, 100.0), (1.0e9, 1.0e9)] {
            let scene = scene_at(w, h, 5);
            assert!(scene.width >= MIN_WIDTH && scene.width <= MAX_SIDE);
            assert!(scene.height >= MIN_HEIGHT && scene.height <= MAX_SIDE);
        }
    }

    #[test]
    fn a_short_frame_drops_panels_instead_of_squashing_them() {
        // 400x300 has room for one panel. It should be a real chart, not an
        // empty box with a title.
        let scene = scene_at(400.0, 300.0, 300);
        let polylines = scene
            .ops
            .iter()
            .filter(|op| matches!(op, Op::Polyline { .. }))
            .count();
        assert_eq!(polylines, 1, "expected exactly the price chart");
    }

    #[test]
    fn an_empty_history_still_renders() {
        let scene = scene_at(1024.0, 640.0, 0);
        assert_eq!(scene.tick, 0);
        assert!(!scene.ops.is_empty());
    }

    #[test]
    fn large_populations_are_sampled() {
        let mut world = World::new(1, 5000, Params::default());
        world.step_many(2);
        let scene = build(&world, 1600.0, 900.0);
        let circles = scene
            .ops
            .iter()
            .filter(|op| matches!(op, Op::Circle { .. }))
            .count();
        assert!(circles <= MAX_POINTS, "frame carried {circles} points");
    }
}
