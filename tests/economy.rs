//! End-to-end checks on the economy itself.
//!
//! The unit tests in `src/sim` check mechanics — money is conserved, the
//! history is capped. These check that the *model* stays healthy over a long
//! run, which is the property most easily broken by an innocent-looking tweak
//! to the market rules.

use econbox_server::sim::{Params, World};

const TICKS: u64 = 2_000;

fn run(params: Params, seed: u64) -> World {
    let mut world = World::new(seed, 300, params);
    world.step_many(TICKS);
    world
}

#[test]
fn the_price_level_stays_anchored() {
    // A price that walks to a clamp means the demand curve stopped working.
    for seed in [1, 7, 99, 2024] {
        let world = run(Params::default(), seed);
        let prices: Vec<f64> = world.history.iter().map(|s| s.price).collect();
        let low = prices.iter().cloned().fold(f64::INFINITY, f64::min);
        let high = prices.iter().cloned().fold(0.0_f64, f64::max);
        assert!(
            low > 0.2 && high < 5.0,
            "seed {seed}: price ranged {low:.3}..{high:.3}"
        );
    }
}

#[test]
fn the_market_keeps_trading() {
    let world = run(Params::default(), 3);
    let recent: Vec<&econbox_server::sim::Sample> = world.history.iter().rev().take(200).collect();
    let traded: f64 = recent.iter().map(|s| s.volume).sum();
    assert!(traded > 0.0, "the market stopped clearing");
    // Deadlock shows up as demand collapsing while supply piles up.
    let demand: f64 = recent.iter().map(|s| s.demand).sum();
    let supply: f64 = recent.iter().map(|s| s.supply).sum();
    let imbalance = (demand - supply).abs() / (demand + supply);
    assert!(imbalance < 0.5, "demand {demand:.1} vs supply {supply:.1}");
}

#[test]
fn almost_everyone_stays_fed() {
    let world = run(Params::default(), 11);
    let need: f64 = world.agents.iter().map(|a| a.need).sum::<f64>() * TICKS as f64;
    let unmet: f64 = world.agents.iter().map(|a| a.unmet).sum();
    assert!(
        unmet / need < 0.01,
        "{:.1}% of consumption went unmet",
        100.0 * unmet / need
    );
}

#[test]
fn redistribution_lowers_inequality() {
    // The headline result the tax slider is there to demonstrate.
    let untaxed = run(Params::default(), 5);
    let taxed = run(
        Params {
            tax_rate: 0.5,
            ..Params::default()
        },
        5,
    );
    let before = untaxed.gini(untaxed.price);
    let after = taxed.gini(taxed.price);
    assert!(after < before, "gini {before:.3} -> {after:.3} with tax");
}

#[test]
fn report() {
    // Not an assertion, a description: run with `--nocapture` to see the shape
    // of a default run.
    let world = run(Params::default(), 42);
    let prices: Vec<f64> = world.history.iter().map(|s| s.price).collect();
    let low = prices.iter().cloned().fold(f64::INFINITY, f64::min);
    let high = prices.iter().cloned().fold(0.0_f64, f64::max);
    let last = world.last().expect("history");
    println!(
        "tick {} price {:.3} (range {:.3}..{:.3}) volume {:.1} demand {:.1} supply {:.1} gini {:.3} unmet {:.2}",
        world.tick,
        world.price,
        low,
        high,
        last.volume,
        last.demand,
        last.supply,
        last.gini,
        last.unmet
    );
}

#[test]
fn most_of_what_people_want_to_trade_actually_trades() {
    // Both sides of the market should be present at the same time. When every
    // agent reprices its target off the same tick's price, the population
    // changes its mind in unison: demand and supply take turns being near zero,
    // only the short side clears, and the flow chart turns into a smear. That
    // failure mode shows up here as clearing efficiency collapsing.
    let world = run(Params::default(), 13);
    let ticks = world.history.len() as f64;
    let volume: f64 = world.history.iter().map(|s| s.volume).sum::<f64>() / ticks;
    let offered: f64 = world
        .history
        .iter()
        .map(|s| 0.5 * (s.demand + s.supply))
        .sum::<f64>()
        / ticks;
    let efficiency = volume / offered;
    assert!(
        efficiency > 0.6,
        "only {:.0}% of what agents wanted to trade cleared",
        efficiency * 100.0
    );
}
