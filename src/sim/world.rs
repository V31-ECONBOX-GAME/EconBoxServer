//! The world: every piece of mutable simulation state lives here.

use std::collections::VecDeque;

use crate::json::Json;
use crate::rng::Rng;
use crate::sim::Params;
use crate::sim::agent::Agent;
use crate::sim::market;

/// How many ticks of history the world keeps for charts.
pub const HISTORY_CAP: usize = 1024;

/// Memory of the economy-wide productivity process. Higher means booms and
/// slumps that last longer.
const CYCLE_PERSISTENCE: f64 = 0.92;
/// How far the economy-wide process swings, as a multiple of `Params::shock`.
const CYCLE_GAIN: f64 = 2.0;
/// The numeraire: the price agents treat as normal. It gives the price level
/// something to return to instead of wandering off on a random walk.
const PRICE_ANCHOR: f64 = 1.0;
/// Demand side: how strongly cheap goods tempt agents into holding more of
/// them. Kept small on purpose. Every agent reprices its target from the same
/// price, so a large value moves aggregate desired stock further in one tick
/// than the whole day's trading volume, and the two sides of the market then
/// alternate instead of overlapping.
const STOCK_ELASTICITY: f64 = 0.3;
/// Supply side: how strongly a high price pulls extra production out of the
/// agents. This is what gives the *level* of aggregate stock somewhere to
/// return to; without it stock is a pure random walk and the price, which
/// tracks scarcity, follows it anywhere.
const SUPPLY_ELASTICITY: f64 = 0.5;
/// Both responses are capped at this factor, so no shock can make production
/// or desired inventory run away.
const RESPONSE_CAP: f64 = 4.0;

/// Smallest and largest population the server will build.
pub const MIN_AGENTS: usize = 1;
pub const MAX_AGENTS: usize = 20_000;

/// One tick of recorded history.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Sample {
    pub tick: u64,
    pub price: f64,
    pub demand: f64,
    pub supply: f64,
    pub volume: f64,
    /// Total goods held by everyone at the end of the tick.
    pub stock: f64,
    /// Total money held by everyone; constant by construction.
    pub cash: f64,
    /// Gini coefficient of wealth, in `[0, 1]`. 0 is perfect equality.
    pub gini: f64,
    /// Consumption that went unsatisfied during this tick.
    pub unmet: f64,
}

impl Sample {
    pub fn to_json(&self) -> Json {
        Json::obj([
            ("tick", Json::num(self.tick as f64)),
            ("price", Json::num2(self.price)),
            ("demand", Json::num2(self.demand)),
            ("supply", Json::num2(self.supply)),
            ("volume", Json::num2(self.volume)),
            ("stock", Json::num2(self.stock)),
            ("cash", Json::num2(self.cash)),
            ("gini", Json::num(round4(self.gini))),
            ("unmet", Json::num2(self.unmet)),
        ])
    }
}

#[derive(Debug)]
pub struct World {
    pub tick: u64,
    pub price: f64,
    pub params: Params,
    pub agents: Vec<Agent>,
    pub history: VecDeque<Sample>,
    /// The seed the world was built from, echoed back so a run can be repeated.
    pub seed: u64,
    /// Economy-wide productivity relative to normal. Positive is a boom.
    pub cycle: f64,
    /// How much the current price scales desired inventory. Recomputed every
    /// tick and kept here so the renderer can judge agents against the same
    /// target the market used.
    pub stock_factor: f64,
    rng: Rng,
}

impl World {
    /// Build a world. `agent_count` is clamped to `MIN_AGENTS..=MAX_AGENTS`.
    pub fn new(seed: u64, agent_count: usize, params: Params) -> World {
        let mut params = params;
        params.sanitize();
        let count = agent_count.clamp(MIN_AGENTS, MAX_AGENTS);
        let mut rng = Rng::new(seed);
        let agents = (0..count)
            .map(|_| Agent::random(&mut rng, params.stock_target))
            .collect::<Vec<_>>();
        World {
            tick: 0,
            price: 1.0,
            params,
            agents,
            history: VecDeque::with_capacity(HISTORY_CAP),
            seed,
            cycle: 0.0,
            stock_factor: 1.0,
            rng,
        }
    }

    /// Throw the world away and build a fresh one, keeping the parameters.
    pub fn reset(&mut self, seed: u64, agent_count: usize) {
        *self = World::new(seed, agent_count, self.params);
    }

    /// Advance the economy by one tick.
    ///
    /// Order matters: agents produce, then eat, then trade what is left over,
    /// then the price reacts to how lopsided the trading was.
    pub fn step(&mut self) {
        let params = self.params;
        let price = self.price;

        // Economy-wide productivity follows an AR(1) process, so a lucky or
        // unlucky spell lasts many ticks. This is what turns per-agent noise
        // into booms and slumps you can see in the price chart.
        self.cycle = CYCLE_PERSISTENCE * self.cycle
            + (1.0 - CYCLE_PERSISTENCE) * self.rng.gauss() * params.shock * CYCLE_GAIN;
        let cycle = self.cycle;

        // A downward-sloping demand curve. When goods are cheap against the
        // numeraire, agents want to hold more of them; when they are dear,
        // less. Without this the price level has nothing to return to and
        // random-walks into the clamps.
        self.stock_factor = (PRICE_ANCHOR / price)
            .powf(STOCK_ELASTICITY)
            .clamp(1.0 / RESPONSE_CAP, RESPONSE_CAP);
        let stock_factor = self.stock_factor;

        // The matching supply curve: production is worth more when the price is
        // high, so agents make more. Supply and demand curves together pin the
        // equilibrium at PRICE_ANCHOR.
        let supply_factor = (price / PRICE_ANCHOR)
            .powf(SUPPLY_ELASTICITY)
            .clamp(1.0 / RESPONSE_CAP, RESPONSE_CAP);

        let agents = &mut self.agents;
        let rng = &mut self.rng;

        // 1. Produce, then consume. Consumption that cannot be covered is lost,
        //    not carried over as debt.
        let mut unmet = 0.0;
        for agent in agents.iter_mut() {
            let shock = 1.0 + cycle + params.shock * rng.gauss();
            agent.goods += (agent.productivity * shock * supply_factor).max(0.0);
            let eaten = agent.need.min(agent.goods);
            agent.goods -= eaten;
            let missed = agent.need - eaten;
            agent.unmet += missed;
            unmet += missed;
        }

        // 2. State orders. Positive is a bid, negative is an ask. Buyers are
        //    limited by both the inventory they lack and the cash they will
        //    part with this tick, so nobody can spend money they do not have.
        let mut demand = 0.0;
        let mut supply = 0.0;
        for agent in agents.iter_mut() {
            let target = agent.target_stock(params.stock_target) * stock_factor;
            if agent.goods < target {
                let affordable = agent.cash.max(0.0) * params.spend_fraction / price;
                let quantity = (target - agent.goods).min(affordable).max(0.0);
                agent.order = quantity;
                demand += quantity;
            } else {
                let quantity = agent.goods - target;
                agent.order = -quantity;
                supply += quantity;
            }
        }

        // 3. Match the two sides.
        let clearing = market::clear(demand, supply);

        // 4. Settle every filled order at the single market price.
        for agent in agents.iter_mut() {
            if agent.order > 0.0 {
                let quantity = agent.order * clearing.buy_fill;
                agent.cash -= quantity * price;
                agent.goods += quantity;
            } else if agent.order < 0.0 {
                let quantity = -agent.order * clearing.sell_fill;
                agent.cash += quantity * price;
                agent.goods -= quantity;
            }
        }

        // 5. Tax cash holdings and pay the proceeds back as an equal dividend.
        //    Taxing holdings rather than trades is what makes the knob bite:
        //    who sells in a given tick is luck, so a sales tax would just
        //    shuffle money between equals and leave the distribution alone.
        if params.tax_rate > 0.0 {
            let mut pool = 0.0;
            for agent in agents.iter_mut() {
                let due = agent.cash.max(0.0) * params.tax_rate;
                agent.cash -= due;
                pool += due;
            }
            let dividend = pool / agents.len() as f64;
            for agent in agents.iter_mut() {
                agent.cash += dividend;
            }
        }

        // 6. The price the next tick will trade at.
        self.price = market::next_price(price, &clearing, params.price_flex);
        self.tick += 1;

        // 7. Record what happened, at the price that produced it.
        let sample = Sample {
            tick: self.tick,
            price,
            demand: clearing.demand,
            supply: clearing.supply,
            volume: clearing.volume,
            stock: self.total_goods(),
            cash: self.total_cash(),
            gini: self.gini(price),
            unmet,
        };
        if self.history.len() == HISTORY_CAP {
            self.history.pop_front();
        }
        self.history.push_back(sample);
    }

    /// Advance several ticks. Returns the number actually run.
    pub fn step_many(&mut self, ticks: u64) -> u64 {
        for _ in 0..ticks {
            self.step();
        }
        ticks
    }

    pub fn total_cash(&self) -> f64 {
        self.agents.iter().map(|a| a.cash).sum()
    }

    pub fn total_goods(&self) -> f64 {
        self.agents.iter().map(|a| a.goods).sum()
    }

    /// Gini coefficient of wealth at the given price.
    pub fn gini(&self, price: f64) -> f64 {
        let mut wealth: Vec<f64> = self
            .agents
            .iter()
            .map(|a| a.wealth(price).max(0.0))
            .collect();
        let n = wealth.len() as f64;
        let total: f64 = wealth.iter().sum();
        if total <= f64::EPSILON || wealth.len() < 2 {
            return 0.0;
        }
        wealth.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let weighted: f64 = wealth
            .iter()
            .enumerate()
            .map(|(i, w)| (i as f64 + 1.0) * w)
            .sum();
        ((2.0 * weighted) / (n * total) - (n + 1.0) / n).clamp(0.0, 1.0)
    }

    /// The most recent tick, if the world has run at all.
    pub fn last(&self) -> Option<&Sample> {
        self.history.back()
    }

    /// A summary of the whole world, without the per-agent detail.
    pub fn to_json(&self) -> Json {
        Json::obj([
            ("tick", Json::num(self.tick as f64)),
            ("price", Json::num2(self.price)),
            ("seed", Json::num(self.seed as f64)),
            ("agents", Json::num(self.agents.len() as f64)),
            ("params", self.params.to_json()),
            (
                "totals",
                Json::obj([
                    ("cash", Json::num2(self.total_cash())),
                    ("goods", Json::num2(self.total_goods())),
                    ("gini", Json::num(round4(self.gini(self.price)))),
                ]),
            ),
            (
                "last",
                self.last().map(|s| s.to_json()).unwrap_or(Json::Null),
            ),
        ])
    }

    /// The last `limit` recorded ticks, oldest first.
    pub fn history_json(&self, limit: usize) -> Json {
        let skip = self.history.len().saturating_sub(limit);
        Json::arr(self.history.iter().skip(skip).map(|s| s.to_json()))
    }
}

fn round4(value: f64) -> f64 {
    (value * 10_000.0).round() / 10_000.0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn world() -> World {
        World::new(1234, 200, Params::default())
    }

    #[test]
    fn same_seed_same_history() {
        let mut a = world();
        let mut b = world();
        a.step_many(200);
        b.step_many(200);
        assert_eq!(a.tick, 200);
        assert_eq!(a.price, b.price);
        assert_eq!(a.history, b.history);
    }

    #[test]
    fn money_is_conserved() {
        let mut w = world();
        let start = w.total_cash();
        w.step_many(500);
        assert!(
            (w.total_cash() - start).abs() / start < 1e-9,
            "cash drifted"
        );
    }

    #[test]
    fn taxes_do_not_leak_money() {
        let mut w = World::new(
            7,
            150,
            Params {
                tax_rate: 0.2,
                ..Params::default()
            },
        );
        let start = w.total_cash();
        w.step_many(300);
        assert!((w.total_cash() - start).abs() / start < 1e-9, "tax leaked");
    }

    #[test]
    fn state_stays_finite_and_non_negative() {
        let mut w = world();
        w.step_many(1000);
        assert!(w.price.is_finite() && w.price > 0.0);
        for agent in &w.agents {
            assert!(
                agent.cash.is_finite() && agent.cash >= -1e-9,
                "cash {}",
                agent.cash
            );
            assert!(
                agent.goods.is_finite() && agent.goods >= -1e-9,
                "goods {}",
                agent.goods
            );
        }
    }

    #[test]
    fn history_is_capped() {
        let mut w = World::new(3, 20, Params::default());
        w.step_many(HISTORY_CAP as u64 + 50);
        assert_eq!(w.history.len(), HISTORY_CAP);
        assert_eq!(w.history.back().unwrap().tick, HISTORY_CAP as u64 + 50);
    }

    #[test]
    fn gini_is_zero_for_equals() {
        let mut w = World::new(1, 4, Params::default());
        for agent in &mut w.agents {
            agent.cash = 100.0;
            agent.goods = 10.0;
        }
        assert!(w.gini(1.0) < 1e-9);
    }

    #[test]
    fn reset_rewinds_the_world() {
        let mut w = world();
        w.step_many(50);
        w.reset(99, 10);
        assert_eq!(w.tick, 0);
        assert_eq!(w.agents.len(), 10);
        assert!(w.history.is_empty());
    }

    #[test]
    fn agent_count_is_clamped() {
        assert_eq!(World::new(1, 0, Params::default()).agents.len(), MIN_AGENTS);
        assert_eq!(
            World::new(1, MAX_AGENTS + 1, Params::default())
                .agents
                .len(),
            MAX_AGENTS
        );
    }
}
