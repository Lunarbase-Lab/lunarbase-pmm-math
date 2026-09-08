# lunarbase-pmm-math

[![crates.io](https://img.shields.io/crates/v/lunarbase-pmm-math.svg)](https://crates.io/crates/lunarbase-pmm-math)
[![docs.rs](https://docs.rs/lunarbase-pmm-math/badge.svg)](https://docs.rs/lunarbase-pmm-math)

Pure Rust mirror of the current on-chain LunarBase linear anchor quotes and
directional punishment state transitions (**Q64.96** sqrt-price design).

- No `unsafe`, no FFI, no allocations on the hot path.
- Single dependency: [`ruint`](https://crates.io/crates/ruint) for 256-bit ints.
- Sqrt-prices use `U256` so the complete Solidity `uint160` domain is available.
- Fees and maximum punishment are validated as `uint24`; reserves are validated
  as `uint112`.

## Quick start

```toml
[dependencies]
lunarbase-pmm-math = "0.4.1"
```

```rust
use lunarbase_pmm_math::{quote_x_to_y, quote_x_to_y_with_multiplier, PoolParams, Q24, Q96, U256};

let params = PoolParams {
    sqrt_price_x96: Q96,                    // price = 1.0
    fee_ask_x24: 0,                         // Q24, charged on Y→X
    fee_bid_x24: (1u32 << 24) / 100,        // 1%, charged on X→Y
    reserve_x: 1_000_000_000_000_000_000,
    reserve_y: 1_000_000_000_000_000_000,
    max_punishment_x24: Q24 / 1_000,        // maximum 0.10% increment
};

let r = quote_x_to_y(&params, U256::from(1_000_000_000_000_000_000u128));
let non_whitelisted = quote_x_to_y_with_multiplier(
    &params,
    U256::from(1_000_000_000_000_000_000u128),
    U256::from(100u64),
);
let _ = (r.amount_out, r.sqrt_price_next, r.fee, r.effective_fee_x24);
let _ = non_whitelisted.amount_out;
```

`quote_x_to_y` and `quote_y_to_x` use `fee_multiplier = 1`, matching a
whitelisted/base-fee caller. The public Pool `quoteXToY`/`quoteYToX` methods
derive the multiplier from `msg.sender`; for a non-whitelisted caller, pass
`pool.blacklistFeeMultiplier()` into `quote_x_to_y_with_multiplier` or
`quote_y_to_x_with_multiplier`.

## API surface

| Item                                                                      | Purpose                                                    |
| ------------------------------------------------------------------------- | ---------------------------------------------------------- |
| `PoolParams` / `PoolParams::validate()`                                   | Solidity-width-checked pool snapshot.                      |
| `try_quote_x_to_y` / `try_quote_y_to_x`                                   | Checked immediate-punishment linear quotes.                |
| `quote_x_to_y_with_multiplier` / `quote_y_to_x_with_multiplier`           | Panicking convenience wrappers for caller fee paths.       |
| `try_punishment_x24` / `try_apply_punishment`                             | Desired ceil-rounded increment and saturating transition.  |
| `try_apply_update`                                                        | Replace anchor and both effective fees like operator `upd`. |
| `try_simulate_successful_swap`                                            | Immediate fee/reserve transition with rollback marker.     |
| `try_build_order_book`                                                    | Pure directional `{size, price}` ladder projection.        |
| `try_build_validated_order_book`                                          | Exhaustive bounded mixed-direction fill-policy guarantee.  |
| `try_build_precise_order_book`                                            | Adaptive multilevel fitting with measured precision.       |
| `try_validate_fee_accounting_capacity`                                    | Fully credited fee and conservative uint112 bucket preflight. |
| `geometric_sizes` / `try_ladder_amount_out`                               | Power-of-two size grid and exact ladder sweep helper.       |
| `price_to_sqrt_price_x96` / `sqrt_price_x96_to_price`                     | Lossy `f64` decimal price ↔ Q64.96 `U256`.                  |

## Order-book projection

`OrderBookState` combines `PoolParams` with the cached snapshot block,
freshness fields, pause state, and the fee multiplier of the actual execution
caller. `try_build_order_book` returns empty paused/stale sides or two active
directional ladders, independent of any particular order-book system. Each
level's size is cumulative raw input and its price is marginal raw output/input
scaled by `1e18`. Size grids are explicit risk policy; use `geometric_sizes`
for a power-of-two grid. Protocol-specific adapters translate this format and
must preserve or separately revalidate its rounding and execution semantics.

At emitted cumulative prefixes the floor-rounded ladder never promises more
than the exact Pool quote. Nested integer rounding means this does not by itself
guarantee every arbitrary partial or lifetime-cursor fill. A production
adapter must preserve `amountOutMinimum`, and the publisher should use tested
lot sizes, short expiries, and rebuild after Pool events.

`try_build_validated_order_book` accepts `FillPolicy` for both directions and
an explicit transition budget. It checks every reachable lot-aligned fill and
exact Pool state transition, including interleavings, then chooses a safe flat
price per direction. `OrderBookSafety::ExhaustiveLotPolicy` covers this finite
domain only; external updates or concurrently live generations require the
adapter guard. The result contains its source state, policy, and coverage
counters. See the repository's `docs/order-book.md` for the proof and limits.
The certificate concerns the price/reserve/punishment model. Before publication,
preflight `FeeAccountingState` from the same snapshot with
`try_validate_fee_accounting_capacity`: partner share uses `BPS=1_000_000`, a
positive share requires a partner operator, and relevant treasury/global
partner/per-router fee buckets need conservative uint112 headroom. Standard
ERC20 behavior and successful external calls remain separate preconditions.
Initial stored reserves must reconcile with token balances minus pending
deposit escrow and global fee buckets; unsynced donations are not modeled.

`try_build_precise_order_book` fits up to 20 levels per direction under the same
finite policy. `OrderBookPrecision` sets the level cap, target underquote in
basis points and a separate fitting-work budget. Results report achieved
precision and `target_met`; require that flag, active status and the same
accounting/state preconditions before publishing at the requested tolerance.
The guarantee applies to the exact raw levels, not automatically to a target
system's packed, rounded or merged representation.

## Testing

```sh
cargo test -p lunarbase-pmm-math
```

The focused suite covers nested-floor pricing, uint160/uint112 validation,
fee and punishment sentinels, ceil rounding, directional saturation, reserve
transitions, checked `mulDiv` failures, and atomic rollback. It also replays
49 deterministic and 10,000 seeded-fuzz rows generated from the production
Solidity `Pool`:

```sh
DARK_POOLS_DIR=/path/to/dark_pools ./scripts/regenerate-vectors.sh
```

Every quote, punishment, fee-state, reserve-state, and rollback field is
compared exactly.

## Companion N-API binding

The companion N-API package
[`@lunarbase-lab/pmm-math`](https://www.npmjs.com/package/@lunarbase-lab/pmm-math)
binds this crate. Its generated types and cross-language vectors are maintained
separately from this pure-Rust API.

## Quote and punishment arithmetic

Both directions quote at the unchanged anchor. X→Y values X in Y with two
sequential round-down `mulDiv` operations; Y→X performs the inverse with the
same nested-floor ordering. Before pricing the current output, punishment is
computed from pre-swap active reserves as
`ceil(effectiveMaxX24 * swapWealth / inventoryWealth)`, capped at the configured
maximum. `uint24::MAX` is expanded to conceptual `2^24` before this calculation.
The increment saturating-adds to bid for X→Y or ask for Y→X; that effective fee
is exposed as `QuoteResult::effective_fee_x24`, charges the current quote, and
is persisted only after successful settlement. An operator update replaces
both effective fees.

The quote functions do not mutate `PoolParams`. Reusing the same snapshot for
every chunk therefore does not represent a sequential split. Chain
`try_simulate_successful_swap` calls with each applied simulation's
`effective_params()` when comparing a single swap with multiple chunks; under
the current immediate-punishment mechanism, splitting can produce more total
output.

## License

Dual-licensed under MIT or Apache-2.0.
