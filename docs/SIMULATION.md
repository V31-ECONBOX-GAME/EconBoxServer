# The economic model

One good, one price, `N` agents. Every agent produces the good, consumes it, and
trades the difference. There is no money creation, no credit, no labour market
and no firm — the whole model is about four hundred lines in `src/sim/`, and it
is meant to be replaced.

What makes it worth watching is that it is a real market rather than a scripted
curve: there is a supply curve, a demand curve, and a price that moves because
buyers and sellers disagree.

## An agent

| Field          | Meaning                                              |
| -------------- | ---------------------------------------------------- |
| `cash`         | Money held                                           |
| `goods`        | Units of the good in stock                           |
| `productivity` | Units produced per tick, before shocks               |
| `need`         | Units consumed per tick                              |
| `order`        | The order placed in the last clearing                |
| `unmet`        | Lifetime consumption the agent could not cover       |

At startup each agent draws one number, `scale ~ U(0.6, 1.4)`, and sets **both**
`productivity` and `need` to it. Cash starts at `U(50, 150)` and stock at
`scale * stock_target`.

Pairing production and need is the single most important modelling choice here,
and it is not cosmetic. Drawing them independently creates agents that
structurally consume more than they make. Goods only ever flow one way in this
model, so those agents pay out cash every tick and never earn it back: they go
broke, stop bidding, and the market deadlocks with unsold stock on one side and
starvation on the other. Pairing them means agents differ in *size* but all can
sustain themselves, so inequality has to emerge from luck rather than from a
built-in handicap. Any richer version of this model needs a return flow of money
— wages, dividends, interest — before it can support structural debtors.

## One tick

`World::step` runs seven steps in a fixed order. Let `p` be the current price
and `N` the population.

### 0. The economy-wide mood

```
cycle <- 0.92 * cycle + 0.08 * gauss() * shock * 2
```

An AR(1) process. A lucky or unlucky spell persists for tens of ticks, which is
what turns per-agent noise into booms and slumps visible in the price chart
rather than a flat jitter.

Two responses to the price are computed once per tick, each clamped to
`[0.25, 4]`:

```
stock_factor  = (1 / p) ^ 0.3      demand side: cheap goods are worth holding
supply_factor = (p / 1) ^ 0.5      supply side: a high price is worth producing for
```

The `1` in both is `PRICE_ANCHOR`, the numeraire — the price agents treat as
normal.

### 1. Produce, then consume

```
goods_i += max(0, productivity_i * (1 + cycle + shock * gauss_i) * supply_factor)
eaten_i  = min(need_i, goods_i)
goods_i -= eaten_i
unmet_i += need_i - eaten_i
```

Consumption that cannot be covered is simply lost. It is not carried forward as
debt, and it is counted so you can see it.

### 2. State orders

Each agent compares its stock against a target:

```
target_i = need_i * stock_target * stock_factor
```

If it is short, it bids for the gap, limited by the cash it will part with this
tick. If it has a surplus, it offers all of it:

```
bid_i = max(0, min(target_i - goods_i, cash_i * spend_fraction / p))   if short
ask_i = goods_i - target_i                                             otherwise
D = sum of bids        S = sum of asks
```

The cash limit is what makes it impossible to spend money you do not have.

### 3. Clear

One price, no order book. The short side of the market is filled completely and
the long side is rationed proportionally:

```
V         = min(D, S)
buy_fill  = V / D
sell_fill = V / S
```

### 4. Settle

Every filled order trades at the same price `p`. Buyers pay
`bid_i * buy_fill * p` and receive the goods; sellers receive
`ask_i * sell_fill * p` and give them up.

### 5. Tax and dividend

```
due_i = cash_i * tax_rate          pool = sum of due
cash_i -= due_i
cash_i += pool / N
```

A tax on *holdings*, not on trades, paid straight back out in equal shares.
Taxing trades would do nothing here: who happens to sell in a given tick is
luck, not wealth, so a sales tax would shuffle money between equals and leave
the distribution exactly where it was. Taxing holdings is what makes the knob
bite.

### 6. Move the price

```
excess = (D - S) / (D + S)
p'     = clamp(p * (1 + price_flex * excess), 0.01, 1e6)
```

Normalising by `D + S` means one `price_flex` value behaves the same in a tiny
economy and a large one. The clamps are a safety net, not part of the model — a
run that reaches them has gone wrong.

### 7. Record

A sample is appended to the history at the price that produced it. The history
holds the last 1024 ticks.

## Where the equilibrium comes from

Two nested loops, on different timescales.

**Fast: the price clears the flow.** If more people want to buy than sell, the
price rises next tick, which shrinks desired stock and so shrinks demand.

**Slow: production sets the level.** Consumption is fixed at `need`, but
production scales with `supply_factor = (p / 1) ^ 0.5`. So when the price sits
above 1, production exceeds consumption, aggregate stock accumulates, agents run
surpluses, and the price is pushed back down. Below 1, the reverse. The economy
has somewhere to return to.

The slow loop is the important one. An earlier version of this model had only
the fast loop, and aggregate stock was then a pure random walk with nothing
pinning it. The price, which tracks scarcity, followed it — down to the clamp in
one run and up past 1500 in another. A demand curve alone does not make a market.

### Why the demand-side elasticity is small

`STOCK_ELASTICITY` is `0.3`, much smaller than the supply side. Every agent
reprices its target off the same price, so they all change their mind at the same
moment. At an elasticity of `1.0`, a single tick's price move shifts aggregate
desired stock by more than a whole tick's trading volume — demand and supply then
take turns being near zero, only the short side ever clears, and the flow chart
becomes a smear. `tests/economy.rs` measures this as clearing efficiency.

## Invariants

These hold at every tick, and there are tests for the first two:

- **Money is conserved exactly.** Settlement is a transfer between two agents;
  the tax is a transfer through a pool that is emptied in the same tick. No path
  creates or destroys cash. Relative drift after 500 ticks is under `1e-9`.
- **The run is deterministic.** The same seed and the same sequence of calls
  produce bit-identical results. The generator is seeded explicitly and stored in
  the world; nothing iterates an unordered collection.
- **Cash never goes negative.** A bid is capped at `cash * spend_fraction / p`,
  so the most an agent can pay is the cash it holds.
- **Stock never goes negative.** An ask is capped at the surplus above target,
  which is at most everything the agent has.
- **Goods are only created by production and destroyed by consumption.** Trade
  moves them; it never mints them.

If you change `step`, keep these. They are what make a run reproducible and a bug
findable.

## What gets recorded

Each tick appends a `Sample`, available from `/api/history` and as `last` in
`/api/state`:

| Field    | Meaning                                                      |
| -------- | ------------------------------------------------------------ |
| `tick`   | Tick number                                                   |
| `price`  | The price this tick traded at                                 |
| `demand` | Units agents wanted to buy                                    |
| `supply` | Units agents offered to sell                                  |
| `volume` | Units actually traded, `min(demand, supply)`                  |
| `stock`  | Total goods held by everyone at the end of the tick           |
| `cash`   | Total money held by everyone — constant by construction       |
| `gini`   | Gini coefficient of wealth, `0` is perfect equality           |
| `unmet`  | Consumption that went unsatisfied this tick                   |

Wealth is `cash + goods * price`. The Gini coefficient is computed on the sorted
wealth vector:

```
G = 2 * sum(i * w_i) / (n * sum(w)) - (n + 1) / n        for i = 1..n, w sorted ascending
```

Because cash starts at `U(50, 150)` and dominates the value of stock, an
untouched world starts near `G = 0.17` and stays there. That is the baseline the
tax slider moves.

## Parameters

Patch these live with `POST /api/params`. Every one is clamped on the way in.

| Parameter        | Default | Range     | What to watch                                                     |
| ---------------- | ------- | --------- | ----------------------------------------------------------------- |
| `price_flex`     | `0.08`  | `0..=1`   | Raise it and the price gets jumpy; lower it and imbalances persist for many ticks and less of what people want to trade actually clears |
| `tax_rate`       | `0`     | `0..=0.5` | Charged **every tick**. At `0.01` the spread in wealth halves in about 70 ticks; the histogram visibly collapses toward one bar |
| `stock_target`   | `4`     | `0..=50`  | How much buffer agents keep. Small values mean unmet consumption on any bad draw; large values mean a lot of idle stock |
| `spend_fraction` | `0.35`  | `0..=1`   | Only binds when the price is high enough that agents cannot afford their gap. Lower it far enough and poor agents are locked out |
| `shock`          | `0.15`  | `0..=2`   | Drives both the per-agent noise and the size of the business cycle |

## Model constants

Not exposed over the API — change them in `src/sim/world.rs` and rebuild. They
are the shape of the model rather than knobs to turn while it runs.

| Constant             | Value  | Role                                             |
| -------------------- | ------ | ------------------------------------------------ |
| `CYCLE_PERSISTENCE`  | `0.92` | How long a boom or slump lasts                   |
| `CYCLE_GAIN`         | `2.0`  | How far the economy-wide process swings          |
| `PRICE_ANCHOR`       | `1.0`  | The numeraire both curves are written against    |
| `STOCK_ELASTICITY`   | `0.3`  | Demand side: desired stock against price         |
| `SUPPLY_ELASTICITY`  | `0.5`  | Supply side: production against price            |
| `RESPONSE_CAP`       | `4.0`  | Ceiling on both responses                        |
| `HISTORY_CAP`        | `1024` | Ticks of history kept                            |

## What a default run looks like

300 agents, seed 42, 2000 ticks, default parameters:

```
price 1.28 (ranged 0.71..1.34)   volume 21.6   demand 25.1   supply 21.6
gini 0.169   unmet 0.00
```

Reproduce it with:

```bash
cargo test --test economy report -- --nocapture
```

## What this model is not

Worth being explicit about, because it bounds what conclusions the picture can
support:

- **One good.** No relative prices, no substitution, no sectors.
- **No firms, no labour, no credit.** Every agent is a household that is also its
  own factory. There is no wage, so there is no channel for money to flow back to
  someone who consumes more than they produce — see the note on pairing above.
- **No expectations.** Agents react to today's price. Nobody speculates,
  forecasts, or holds a view about tomorrow. Bubbles cannot form here.
- **Perfect information, one price.** Everyone trades at the same price with a
  costless auctioneer. There are no bilateral trades, no search, no spread.
- **No entry or exit.** The population is fixed. Nobody is born, dies or goes
  bankrupt; an agent with nothing simply stops bidding.
- **The Gini baseline is an artifact of the starting draw,** not an emergent
  result. Inequality drifts here; it does not explode.

## Things to try

- Set `tax_rate` to `0.01` and watch the wealth histogram collapse, then set it
  back to `0` and watch it spread again.
- Raise `shock` to `0.6`. The business cycle gets violent and unmet consumption
  starts appearing — the buffer stops being enough.
- Drop `stock_target` to `1`. Agents run hand to mouth and a single bad draw
  means going hungry.
- Set `price_flex` to `0` to freeze the price and watch stock accumulate or
  drain with nothing to correct it.
- Give agents heterogeneous `stock_target` values and see whether the flow
  alternation goes away entirely.
- Add a second good and a relative price. This is where the model stops being a
  toy, and where `Sample`, `Op` and the panels all need real thought.
