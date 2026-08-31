//! Market clearing.
//!
//! One good, one price, no order book: every agent states how much it wants to
//! buy or sell at the current price, the short side of the market is filled
//! completely, and the long side is rationed proportionally. The price then
//! moves toward whatever would have balanced the two.

/// The outcome of one clearing round.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Clearing {
    /// Units agents wanted to buy.
    pub demand: f64,
    /// Units agents offered to sell.
    pub supply: f64,
    /// Units actually traded: `min(demand, supply)`.
    pub volume: f64,
    /// Share of each buy order that was filled, in `[0, 1]`.
    pub buy_fill: f64,
    /// Share of each sell order that was filled, in `[0, 1]`.
    pub sell_fill: f64,
}

/// Match aggregate demand against aggregate supply.
pub fn clear(demand: f64, supply: f64) -> Clearing {
    let volume = demand.min(supply).max(0.0);
    Clearing {
        demand,
        supply,
        volume,
        buy_fill: ratio(volume, demand),
        sell_fill: ratio(volume, supply),
    }
}

/// The price for the next tick: it rises while buyers outnumber sellers and
/// falls otherwise. Excess demand is normalised to `[-1, 1]` so a single
/// `flex` value behaves the same in a tiny economy and a large one.
pub fn next_price(price: f64, clearing: &Clearing, flex: f64) -> f64 {
    let total = clearing.demand + clearing.supply;
    if total <= f64::EPSILON {
        return price;
    }
    let excess = (clearing.demand - clearing.supply) / total;
    // Keep the price positive and finite whatever the client sets `flex` to.
    (price * (1.0 + flex * excess)).clamp(0.01, 1.0e6)
}

fn ratio(part: f64, whole: f64) -> f64 {
    if whole > f64::EPSILON {
        (part / whole).clamp(0.0, 1.0)
    } else {
        0.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_side_sets_the_volume() {
        let c = clear(10.0, 4.0);
        assert_eq!(c.volume, 4.0);
        assert_eq!(c.sell_fill, 1.0);
        assert!((c.buy_fill - 0.4).abs() < 1e-12);
    }

    #[test]
    fn empty_market_is_inert() {
        let c = clear(0.0, 0.0);
        assert_eq!(c.volume, 0.0);
        assert_eq!(next_price(3.0, &c, 0.5), 3.0);
    }

    #[test]
    fn price_follows_excess_demand() {
        let up = clear(10.0, 1.0);
        let down = clear(1.0, 10.0);
        assert!(next_price(1.0, &up, 0.1) > 1.0);
        assert!(next_price(1.0, &down, 0.1) < 1.0);
    }

    #[test]
    fn price_stays_in_bounds() {
        let crash = clear(0.0, 100.0);
        assert!(next_price(0.02, &crash, 1.0) >= 0.01);
    }
}
