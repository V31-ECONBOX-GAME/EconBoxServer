//! A single economic actor. Every agent both produces and consumes, so the
//! same type covers households and firms; who buys and who sells is decided
//! each tick by inventory, not by a fixed role.

use crate::rng::Rng;

#[derive(Debug, Clone)]
pub struct Agent {
    /// Money held. The sum over all agents is conserved by [`World::step`].
    pub cash: f64,
    /// Units of the good in stock.
    pub goods: f64,
    /// The order this agent placed in the most recent clearing: positive is a
    /// bid, negative an ask, zero means it sat the round out.
    pub order: f64,
    /// Units produced per tick, before the productivity shock.
    pub productivity: f64,
    /// Units consumed per tick.
    pub need: f64,
    /// Units of consumption the agent could not cover, accumulated over its
    /// lifetime. A rising figure means the economy is failing this agent.
    pub unmet: f64,
}

impl Agent {
    /// Draw an agent from the starting distribution.
    ///
    /// Productivity and need are drawn *together*: agents differ in size, not
    /// in whether they can sustain themselves. Drawing them independently
    /// creates a class of agents that consume more than they make, and since
    /// goods only ever flow one way their cash drains until they can no longer
    /// bid at all — the market then deadlocks with unsold stock on one side
    /// and starvation on the other. Inequality here has to emerge from luck,
    /// not from a built-in handicap.
    pub fn random(rng: &mut Rng, stock_target: f64) -> Agent {
        let scale = rng.range(0.6, 1.4);
        Agent {
            cash: rng.range(50.0, 150.0),
            goods: scale * stock_target,
            order: 0.0,
            productivity: scale,
            need: scale,
            unmet: 0.0,
        }
    }

    /// Inventory the agent tries to hold.
    pub fn target_stock(&self, stock_target: f64) -> f64 {
        self.need * stock_target
    }

    /// Total worth at the given price, used for the wealth statistics.
    pub fn wealth(&self, price: f64) -> f64 {
        self.cash + self.goods * price
    }
}
