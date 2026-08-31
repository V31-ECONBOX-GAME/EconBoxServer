//! The economic simulation. This is the only place where state changes.
//!
//! The model is deliberately small — one good, one price, N agents — but it is
//! a real market: prices move because of excess demand, not because of a
//! scripted curve. See `docs/SIMULATION.md` for the equations.

pub mod agent;
pub mod market;
pub mod world;

pub use agent::Agent;
pub use world::{Sample, World};

use crate::json::Json;

/// Tunable knobs. Clients may patch these at runtime via `POST /api/params`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Params {
    /// How strongly the price reacts to excess demand, per tick.
    pub price_flex: f64,
    /// Share of every agent's cash taken each tick and paid straight back out
    /// as an equal dividend. Money is conserved; only its distribution moves.
    pub tax_rate: f64,
    /// Desired inventory, measured in ticks of consumption.
    pub stock_target: f64,
    /// Largest share of its cash an agent commits to buying in one tick.
    pub spend_fraction: f64,
    /// Amplitude of the per-agent productivity shock.
    pub shock: f64,
}

impl Default for Params {
    fn default() -> Params {
        Params {
            price_flex: 0.08,
            tax_rate: 0.0,
            stock_target: 4.0,
            spend_fraction: 0.35,
            shock: 0.15,
        }
    }
}

impl Params {
    /// Clamp every field into a range where the model stays well behaved.
    /// Applied after every patch so no client input can wedge the simulation.
    pub fn sanitize(&mut self) {
        self.price_flex = clamp(self.price_flex, 0.0, 1.0, 0.08);
        self.tax_rate = clamp(self.tax_rate, 0.0, 0.5, 0.0);
        self.stock_target = clamp(self.stock_target, 0.0, 50.0, 4.0);
        self.spend_fraction = clamp(self.spend_fraction, 0.0, 1.0, 0.35);
        self.shock = clamp(self.shock, 0.0, 2.0, 0.15);
    }

    /// Apply the fields present in a JSON object; absent fields are untouched.
    pub fn patch(&mut self, value: &Json) {
        if let Some(v) = value.get("price_flex").as_f64() {
            self.price_flex = v;
        }
        if let Some(v) = value.get("tax_rate").as_f64() {
            self.tax_rate = v;
        }
        if let Some(v) = value.get("stock_target").as_f64() {
            self.stock_target = v;
        }
        if let Some(v) = value.get("spend_fraction").as_f64() {
            self.spend_fraction = v;
        }
        if let Some(v) = value.get("shock").as_f64() {
            self.shock = v;
        }
        self.sanitize();
    }

    pub fn to_json(self) -> Json {
        Json::obj([
            ("price_flex", Json::num(self.price_flex)),
            ("tax_rate", Json::num(self.tax_rate)),
            ("stock_target", Json::num(self.stock_target)),
            ("spend_fraction", Json::num(self.spend_fraction)),
            ("shock", Json::num(self.shock)),
        ])
    }
}

fn clamp(value: f64, low: f64, high: f64, fallback: f64) -> f64 {
    if value.is_finite() {
        value.clamp(low, high)
    } else {
        fallback
    }
}
