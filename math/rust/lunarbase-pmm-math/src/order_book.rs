//! Pure conversion of one coherent LunarBase Pool snapshot into protocol-neutral
//! directional price ladders.
//!
//! This module deliberately contains no RPC, event subscription, cache,
//! signing, or wall-clock code. A caller supplies one atomically published
//! snapshot and explicit cumulative input sizes. Every size is quoted against
//! the same immutable [`PoolParams`].
//!
//! A level price is the floor-rounded chord between two cumulative exact-input
//! quotes, scaled by [`PRICE_SCALE_X18`]. This guarantees that the ladder does
//! not exceed the Pool quote at the sampled cumulative prefixes. The on-chain
//! quote uses nested integer floors and immediate punishment, however, so it is
//! not a mathematically continuous concave curve. Arbitrary partial fills and
//! later fills from a consumed ladder cursor still require execution-size
//! policy and an adapter-side `amountOutMinimum` check.

use core::fmt;

use crate::{
    try_quote_x_to_y_with_multiplier, try_quote_y_to_x_with_multiplier, Direction, MathError,
    PoolParams, U256Ext, MAX_U112, U256,
};

/// Level-price scale: raw token-out units per raw token-in unit,
/// multiplied by `1e18`.
pub const PRICE_SCALE_X18: U256 = U256::from_limbs([1_000_000_000_000_000_000u64, 0, 0, 0]);

/// Maximum number of levels accepted per directional ladder by this library.
pub const MAX_ORDER_BOOK_LEVELS: usize = 20;

const MAX_U48: u64 = (1u64 << 48) - 1;

/// Pool and caller state required for snapshot freshness and the directional
/// quote/reserve/punishment model. This excludes fee-accounting buckets and
/// token-call behavior, which must be checked separately for settlement.
///
/// `fee_multiplier` must belong to the address that will call the Pool (for
/// example the external settlement adapter), not the taker or recipient.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OrderBookState {
    /// Quote-critical Pool storage from the same snapshot.
    pub pool: PoolParams,
    /// Effective caller-specific fee multiplier (`1` for a whitelisted caller).
    pub fee_multiplier: U256,
    /// Block number identifying the complete cached snapshot.
    pub snapshot_block: u64,
    /// Latest block in which the publisher will allow this ladder to execute.
    /// It must cover the signed quote lifetime, not only the next block.
    pub max_execution_block: u64,
    /// Block in which the operator most recently called `upd`.
    pub latest_update_block: u64,
    /// Number of blocks for which the operator state remains fresh.
    pub block_delay: u64,
    /// Current OpenZeppelin pause state from the same snapshot.
    pub paused: bool,
}

impl OrderBookState {
    /// Validate Solidity widths and cache-coherence invariants.
    pub fn validate(&self) -> Result<(), OrderBookError> {
        self.pool.validate().map_err(OrderBookError::Math)?;
        if self.latest_update_block > MAX_U48 {
            return Err(OrderBookError::LatestUpdateBlockExceedsUint48);
        }
        if self.block_delay == 0 {
            return Err(OrderBookError::BlockDelayIsZero);
        }
        if self.block_delay > MAX_U48 {
            return Err(OrderBookError::BlockDelayExceedsUint48);
        }
        if self.fee_multiplier.is_zero() {
            return Err(OrderBookError::FeeMultiplierIsZero);
        }
        if self.max_execution_block < self.snapshot_block {
            return Err(OrderBookError::ValidityPrecedesSnapshot);
        }
        if self.snapshot_block < self.latest_update_block {
            return Err(OrderBookError::SnapshotPrecedesLatestUpdate);
        }
        Ok(())
    }

    /// Return the fail-closed publication status for this snapshot.
    pub fn status(&self) -> Result<OrderBookStatus, OrderBookError> {
        self.validate()?;
        if self.paused {
            return Ok(OrderBookStatus::Paused);
        }
        let stale_at = self.latest_update_block + self.block_delay;
        if self.max_execution_block >= stale_at {
            return Ok(OrderBookStatus::Stale);
        }
        Ok(OrderBookStatus::Active)
    }
}

/// Explicit size policy for both independently streamed directions.
///
/// Entries are cumulative raw token-in amounts. An empty slice disables that
/// direction. Non-empty slices must contain at most 20 strictly increasing,
/// non-zero values.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OrderBookConfig<'a> {
    /// Cumulative token-X input sizes for X→Y.
    pub x_to_y_sizes: &'a [U256],
    /// Cumulative token-Y input sizes for Y→X.
    pub y_to_x_sizes: &'a [U256],
}

/// Whether a cached snapshot passes the pause and freshness publication gate.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OrderBookStatus {
    /// The Pool is unpaused and its operator update is still fresh.
    Active,
    /// Swaps are paused; both returned ladders are empty.
    Paused,
    /// `max_execution_block >= latest_update_block + block_delay`; both ladders are
    /// empty.
    Stale,
}

/// Scope of the mathematical execution guarantee, independent of freshness.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OrderBookSafety {
    /// Sampled-prefix pricing only; arbitrary partial fills are not certified.
    Indicative,
    /// Every reachable fill under the returned finite lot policy was checked
    /// in the quote/reserve/punishment model. Fully credited fees, accounting
    /// headroom and standard-token behavior are additional preconditions;
    /// external Pool changes and other quote generations are not covered.
    ExhaustiveLotPolicy,
}

/// One protocol-neutral, cumulative-size order-book level.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OrderBookLevel {
    /// Cumulative raw token-in volume available through this level.
    pub size: U256,
    /// Marginal raw token-out/token-in price, scaled by `1e18`.
    pub price: U256,
}

/// Levels for one exact-input direction.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct DirectionalLadder {
    /// Ascending cumulative sizes with monotone non-improving prices.
    pub levels: Vec<OrderBookLevel>,
    /// True when requested depth was stopped before the end because it crossed
    /// input-reserve headroom, exhausted output liquidity, produced a
    /// non-increasing quote, or rounded the next price to zero.
    pub truncated: bool,
}

/// Pricing projection for both exact-input trading directions.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OrderBook {
    /// Snapshot publication status. Non-active books never contain levels;
    /// an active side can still be empty when it has no executable liquidity.
    pub status: OrderBookStatus,
    /// Mathematical coverage; `Active` alone is not an execution guarantee.
    pub safety: OrderBookSafety,
    /// Block number of the input snapshot, echoed for cache/version checks.
    pub snapshot_block: u64,
    /// Latest execution block covered by the freshness gate.
    pub max_execution_block: u64,
    /// Always true: a settlement adapter must pass the executing system's exact
    /// promised output to the Pool as `amountOutMinimum`, including for unmodeled
    /// state changes. The system's ladder semantics must be validated separately.
    pub requires_amount_out_minimum: bool,
    /// Token X in, token Y out.
    pub x_to_y: DirectionalLadder,
    /// Token Y in, token X out.
    pub y_to_x: DirectionalLadder,
}

/// Invalid snapshot, size grid, or checked arithmetic failure.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OrderBookError {
    /// Underlying bit-exact Pool quote failure.
    Math(MathError),
    /// A directional size grid contains more than 20 levels.
    TooManyLevels {
        /// Direction containing the invalid grid.
        direction: Direction,
        /// Supplied level count.
        count: usize,
    },
    /// A cumulative size is zero.
    ZeroSize {
        /// Direction containing the invalid value.
        direction: Direction,
        /// Zero-based index in the supplied grid.
        index: usize,
    },
    /// Cumulative sizes are not strictly increasing.
    SizesNotStrictlyIncreasing {
        /// Direction containing the invalid pair.
        direction: Direction,
        /// Zero-based index of the second value in the invalid pair.
        index: usize,
    },
    /// A geometric grid requested zero levels.
    LevelCountIsZero,
    /// A geometric grid used a zero cap.
    CapIsZero,
    /// A price `mulDiv` result did not fit in `uint256`.
    PriceOverflow,
    /// Summing swept output overflowed `uint256`.
    OutputOverflow,
    /// Cached `latestUpdateBlock` exceeds its on-chain `uint48` width.
    LatestUpdateBlockExceedsUint48,
    /// Cached `blockDelay` is zero, which the Pool setter rejects.
    BlockDelayIsZero,
    /// Cached `blockDelay` exceeds its on-chain `uint48` width.
    BlockDelayExceedsUint48,
    /// Resolved caller-specific fee multiplier is zero.
    FeeMultiplierIsZero,
    /// The snapshot claims to precede the operator update it contains.
    SnapshotPrecedesLatestUpdate,
    /// The promised maximum execution block precedes the cached snapshot.
    ValidityPrecedesSnapshot,
    /// A freely constructed ladder does not have strictly ascending sizes.
    InvalidLadderSizeOrder {
        /// Zero-based index of the invalid level.
        index: usize,
    },
    /// A freely constructed ladder exceeds the protocol's level count.
    TooManyLadderLevels,
    /// A freely constructed ladder has a zero or improving price.
    InvalidLadderPrice {
        /// Zero-based index of the invalid price.
        index: usize,
    },
}

impl fmt::Display for OrderBookError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Math(error) => write!(f, "pool quote failed: {error}"),
            Self::TooManyLevels { direction, count } => write!(
                f,
                "{direction:?} size grid contains {count} levels; maximum is {MAX_ORDER_BOOK_LEVELS}"
            ),
            Self::ZeroSize { direction, index } => {
                write!(f, "{direction:?} cumulative size at index {index} is zero")
            }
            Self::SizesNotStrictlyIncreasing { direction, index } => write!(
                f,
                "{direction:?} cumulative sizes are not strictly increasing at index {index}"
            ),
            Self::LevelCountIsZero => f.write_str("geometric level count must be non-zero"),
            Self::CapIsZero => f.write_str("geometric size cap must be non-zero"),
            Self::PriceOverflow => f.write_str("order-book price exceeds uint256"),
            Self::OutputOverflow => f.write_str("swept order-book output exceeds uint256"),
            Self::LatestUpdateBlockExceedsUint48 => {
                f.write_str("latest_update_block exceeds uint48")
            }
            Self::BlockDelayIsZero => f.write_str("block_delay must be non-zero"),
            Self::BlockDelayExceedsUint48 => f.write_str("block_delay exceeds uint48"),
            Self::FeeMultiplierIsZero => f.write_str("fee_multiplier must be non-zero"),
            Self::SnapshotPrecedesLatestUpdate => {
                f.write_str("snapshot_block precedes latest_update_block")
            }
            Self::ValidityPrecedesSnapshot => {
                f.write_str("max_execution_block precedes snapshot_block")
            }
            Self::InvalidLadderSizeOrder { index } => write!(
                f,
                "ladder sizes are not strictly increasing at index {index}"
            ),
            Self::TooManyLadderLevels => write!(f, "ladder exceeds {MAX_ORDER_BOOK_LEVELS} levels"),
            Self::InvalidLadderPrice { index } => write!(f, "ladder price is zero or improving at index {index}"),
        }
    }
}

impl std::error::Error for OrderBookError {}

impl From<MathError> for OrderBookError {
    fn from(value: MathError) -> Self {
        Self::Math(value)
    }
}

fn validate_sizes(direction: Direction, sizes: &[U256]) -> Result<(), OrderBookError> {
    if sizes.len() > MAX_ORDER_BOOK_LEVELS {
        return Err(OrderBookError::TooManyLevels {
            direction,
            count: sizes.len(),
        });
    }
    let mut previous = U256::ZERO;
    for (index, size) in sizes.iter().copied().enumerate() {
        if size.is_zero() {
            return Err(OrderBookError::ZeroSize { direction, index });
        }
        if index > 0 && size <= previous {
            return Err(OrderBookError::SizesNotStrictlyIncreasing { direction, index });
        }
        previous = size;
    }
    Ok(())
}

/// Produce a power-of-two cumulative size grid from `cap / 2^(levels-1)` through
/// `cap`, rounding down and dropping zeros and duplicates.
pub fn geometric_sizes(cap: U256, levels: usize) -> Result<Vec<U256>, OrderBookError> {
    if cap.is_zero() {
        return Err(OrderBookError::CapIsZero);
    }
    if levels == 0 {
        return Err(OrderBookError::LevelCountIsZero);
    }
    if levels > MAX_ORDER_BOOK_LEVELS {
        return Err(OrderBookError::TooManyLevels {
            direction: Direction::XToY,
            count: levels,
        });
    }

    let mut sizes = Vec::with_capacity(levels);
    for shift in (0..levels).rev() {
        let size = cap >> shift;
        if !size.is_zero() && sizes.last().copied() != Some(size) {
            sizes.push(size);
        }
    }
    Ok(sizes)
}

/// Build one directional ladder from cumulative quotes against an immutable
/// Pool snapshot.
fn try_build_directional_ladder(
    pool: &PoolParams,
    direction: Direction,
    fee_multiplier: U256,
    cumulative_sizes: &[U256],
) -> Result<DirectionalLadder, OrderBookError> {
    pool.validate()?;
    if fee_multiplier.is_zero() {
        return Err(OrderBookError::FeeMultiplierIsZero);
    }
    validate_sizes(direction, cumulative_sizes)?;

    let input_headroom = match direction {
        Direction::XToY => MAX_U112 - pool.reserve_x,
        Direction::YToX => MAX_U112 - pool.reserve_y,
    };
    let input_headroom = U256::from(input_headroom);

    let mut ladder = DirectionalLadder {
        levels: Vec::with_capacity(cumulative_sizes.len()),
        truncated: false,
    };
    let mut previous_size = U256::ZERO;
    let mut previous_output = U256::ZERO;
    let mut previous_price = U256::ZERO;

    for size in cumulative_sizes.iter().copied() {
        if size > input_headroom {
            ladder.truncated = true;
            break;
        }

        let quote = match direction {
            Direction::XToY => try_quote_x_to_y_with_multiplier(pool, size, fee_multiplier)?,
            Direction::YToX => try_quote_y_to_x_with_multiplier(pool, size, fee_multiplier)?,
        };
        if quote.amount_out <= previous_output {
            ladder.truncated = true;
            break;
        }

        let marginal_input = size - previous_size;
        let marginal_output = quote.amount_out - previous_output;
        let mut price = U256::checked_mul_div(marginal_output, PRICE_SCALE_X18, marginal_input)
            .ok_or(OrderBookError::PriceOverflow)?;
        if !previous_price.is_zero() && price > previous_price {
            price = previous_price;
        }
        if price.is_zero() {
            ladder.truncated = true;
            break;
        }

        ladder.levels.push(OrderBookLevel { size, price });
        previous_size = size;
        previous_output = quote.amount_out;
        previous_price = price;
    }
    Ok(ladder)
}

/// Build both directional ladders from one coherent cached snapshot.
///
/// Paused or stale snapshots return empty ladders and a non-active status;
/// malformed snapshots and size policies return an error.
pub fn try_build_order_book(
    state: &OrderBookState,
    config: &OrderBookConfig<'_>,
) -> Result<OrderBook, OrderBookError> {
    validate_sizes(Direction::XToY, config.x_to_y_sizes)?;
    validate_sizes(Direction::YToX, config.y_to_x_sizes)?;
    let status = state.status()?;
    if status != OrderBookStatus::Active {
        return Ok(OrderBook {
            status,
            safety: OrderBookSafety::Indicative,
            snapshot_block: state.snapshot_block,
            max_execution_block: state.max_execution_block,
            requires_amount_out_minimum: true,
            x_to_y: DirectionalLadder::default(),
            y_to_x: DirectionalLadder::default(),
        });
    }

    Ok(OrderBook {
        status,
        safety: OrderBookSafety::Indicative,
        snapshot_block: state.snapshot_block,
        max_execution_block: state.max_execution_block,
        requires_amount_out_minimum: true,
        x_to_y: try_build_directional_ladder(
            &state.pool,
            Direction::XToY,
            state.fee_multiplier,
            config.x_to_y_sizes,
        )?,
        y_to_x: try_build_directional_ladder(
            &state.pool,
            Direction::YToX,
            state.fee_multiplier,
            config.y_to_x_sizes,
        )?,
    })
}

/// Sweep a directional ladder with exact per-tranche integer rounding.
///
/// Each consumed tranche contributes `floor(input * price / 1e18)`. Consumers
/// using this ladder's execution semantics must preserve that exact sum; do
/// not round-trip it through an average price. Different matching or rounding
/// semantics require separate validation by the integrating adapter.
///
/// Returns `None` when `amount_in` exceeds the published top-level depth.
pub fn try_ladder_amount_out(
    ladder: &DirectionalLadder,
    amount_in: U256,
) -> Result<Option<U256>, OrderBookError> {
    try_ladder_amount_out_at_cursor(ladder, U256::ZERO, amount_in)
}

/// Sweep a directional ladder starting at its already-consumed lifetime cursor.
///
/// Returns `None` when `cursor + amount_in` exceeds published depth.
pub fn try_ladder_amount_out_at_cursor(
    ladder: &DirectionalLadder,
    cursor: U256,
    amount_in: U256,
) -> Result<Option<U256>, OrderBookError> {
    if ladder.levels.len() > MAX_ORDER_BOOK_LEVELS {
        return Err(OrderBookError::TooManyLadderLevels);
    }
    let mut previous_size = U256::ZERO;
    let mut previous_price = U256::MAX;
    for (index, level) in ladder.levels.iter().enumerate() {
        if level.size <= previous_size {
            return Err(OrderBookError::InvalidLadderSizeOrder { index });
        }
        if level.price.is_zero() || level.price > previous_price {
            return Err(OrderBookError::InvalidLadderPrice { index });
        }
        previous_size = level.size;
        previous_price = level.price;
    }
    let Some(last) = ladder.levels.last() else {
        return Ok(if cursor.is_zero() && amount_in.is_zero() {
            Some(U256::ZERO)
        } else {
            None
        });
    };
    let Some(fill_end) = cursor.checked_add(amount_in) else {
        return Ok(None);
    };
    if fill_end > last.size {
        return Ok(None);
    }
    if amount_in.is_zero() {
        return Ok(Some(U256::ZERO));
    }

    previous_size = U256::ZERO;
    let mut tranche_sum = U256::ZERO;
    for level in &ladder.levels {
        let overlap_start = cursor.max(previous_size);
        let overlap_end = fill_end.min(level.size);
        if overlap_end > overlap_start {
            let consumed = overlap_end - overlap_start;
            let tranche_output = U256::checked_mul_div(consumed, level.price, PRICE_SCALE_X18)
                .ok_or(OrderBookError::PriceOverflow)?;
            tranche_sum = tranche_sum
                .checked_add(tranche_output)
                .ok_or(OrderBookError::OutputOverflow)?;
        }
        previous_size = level.size;
        if level.size >= fill_end {
            break;
        }
    }

    Ok(Some(tranche_sum))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{try_quote_x_to_y_with_multiplier, Q24, Q96};

    fn active_state() -> OrderBookState {
        OrderBookState {
            pool: PoolParams {
                sqrt_price_x96: Q96,
                fee_ask_x24: 0,
                fee_bid_x24: 0,
                reserve_x: 1_000_000,
                reserve_y: 1_000_000,
                max_punishment_x24: 0,
            },
            fee_multiplier: U256::from(1u64),
            snapshot_block: 100,
            max_execution_block: 101,
            latest_update_block: 99,
            block_delay: 3,
            paused: false,
        }
    }

    #[test]
    fn geometric_grid_matches_reference_and_deduplicates_dust() {
        assert_eq!(
            geometric_sizes(U256::from(64u64), 4).unwrap(),
            [8u64, 16, 32, 64].map(U256::from)
        );
        assert_eq!(
            geometric_sizes(U256::from(3u64), 4).unwrap(),
            [1u64, 3].map(U256::from)
        );
        assert_eq!(
            geometric_sizes(U256::ZERO, 1),
            Err(OrderBookError::CapIsZero)
        );
        assert_eq!(
            geometric_sizes(U256::from(1u64), 0),
            Err(OrderBookError::LevelCountIsZero)
        );
    }

    #[test]
    fn builds_both_flat_directional_ladders() {
        let state = active_state();
        let sizes = [U256::from(100u64), U256::from(1_000u64)];
        let book = try_build_order_book(
            &state,
            &OrderBookConfig {
                x_to_y_sizes: &sizes,
                y_to_x_sizes: &sizes,
            },
        )
        .unwrap();

        assert_eq!(book.status, OrderBookStatus::Active);
        assert_eq!(book.snapshot_block, 100);
        assert_eq!(book.max_execution_block, 101);
        assert!(book.requires_amount_out_minimum);
        assert_eq!(book.x_to_y.levels.len(), 2);
        assert_eq!(book.y_to_x.levels.len(), 2);
        assert!(book
            .x_to_y
            .levels
            .iter()
            .all(|level| level.price == PRICE_SCALE_X18));
        assert!(!book.x_to_y.truncated);
    }

    #[test]
    fn punishment_produces_non_improving_prices_and_safe_sampled_prefixes() {
        let mut state = active_state();
        state.pool.reserve_x = 1_000_000_000;
        state.pool.reserve_y = 1_000_000_000;
        state.pool.fee_bid_x24 = Q24 / 1_000;
        state.pool.max_punishment_x24 = Q24 / 10;
        let sizes = [1_000u64, 10_000, 100_000, 1_000_000].map(U256::from);
        let book = try_build_order_book(
            &state,
            &OrderBookConfig {
                x_to_y_sizes: &sizes,
                y_to_x_sizes: &[],
            },
        )
        .unwrap();

        for pair in book.x_to_y.levels.windows(2) {
            assert!(pair[1].price <= pair[0].price);
        }
        for level in &book.x_to_y.levels {
            let promised = try_ladder_amount_out(&book.x_to_y, level.size)
                .unwrap()
                .unwrap();
            let exact =
                try_quote_x_to_y_with_multiplier(&state.pool, level.size, state.fee_multiplier)
                    .unwrap()
                    .amount_out;
            assert!(
                promised <= exact,
                "{} > {} at {}",
                promised,
                exact,
                level.size
            );
        }
    }

    #[test]
    fn caller_multiplier_is_part_of_the_priced_snapshot() {
        let mut base = active_state();
        base.pool.fee_bid_x24 = Q24 / 100;
        let mut multiplied = base;
        multiplied.fee_multiplier = U256::from(10u64);
        let sizes = [U256::from(10_000u64)];
        let config = OrderBookConfig {
            x_to_y_sizes: &sizes,
            y_to_x_sizes: &[],
        };

        let base_price = try_build_order_book(&base, &config).unwrap().x_to_y.levels[0].price;
        let multiplied_price = try_build_order_book(&multiplied, &config)
            .unwrap()
            .x_to_y
            .levels[0]
            .price;
        assert!(multiplied_price < base_price);
    }

    #[test]
    fn paused_and_stale_snapshots_fail_closed() {
        let sizes = [U256::from(100u64)];
        let config = OrderBookConfig {
            x_to_y_sizes: &sizes,
            y_to_x_sizes: &sizes,
        };
        let mut paused = active_state();
        paused.paused = true;
        let paused_book = try_build_order_book(&paused, &config).unwrap();
        assert_eq!(paused_book.status, OrderBookStatus::Paused);
        assert!(paused_book.x_to_y.levels.is_empty());
        assert!(paused_book.y_to_x.levels.is_empty());

        let mut stale = active_state();
        stale.max_execution_block = 102;
        let stale_book = try_build_order_book(&stale, &config).unwrap();
        assert_eq!(stale_book.status, OrderBookStatus::Stale);
        assert!(stale_book.x_to_y.levels.is_empty());
    }

    #[test]
    fn truncates_before_unexecutable_input_or_output_reserve_bound() {
        let sizes = [U256::from(1u64), U256::from(2u64)];
        let mut no_input_headroom = active_state();
        no_input_headroom.pool.reserve_x = MAX_U112;
        let headroom = try_build_order_book(
            &no_input_headroom,
            &OrderBookConfig {
                x_to_y_sizes: &sizes,
                y_to_x_sizes: &[],
            },
        )
        .unwrap();
        assert!(headroom.x_to_y.levels.is_empty());
        assert!(headroom.x_to_y.truncated);

        let mut output_cliff = active_state();
        output_cliff.pool.reserve_y = 1;
        let cliff = try_build_order_book(
            &output_cliff,
            &OrderBookConfig {
                x_to_y_sizes: &sizes,
                y_to_x_sizes: &[],
            },
        )
        .unwrap();
        assert_eq!(cliff.x_to_y.levels.len(), 1);
        assert!(cliff.x_to_y.truncated);
    }

    #[test]
    fn rejects_invalid_grids_and_incoherent_snapshots() {
        let state = active_state();
        let zero = [U256::ZERO];
        let duplicate = [U256::from(1u64), U256::from(1u64)];
        assert!(matches!(
            try_build_order_book(
                &state,
                &OrderBookConfig {
                    x_to_y_sizes: &zero,
                    y_to_x_sizes: &[],
                }
            ),
            Err(OrderBookError::ZeroSize { .. })
        ));
        assert!(matches!(
            try_build_order_book(
                &state,
                &OrderBookConfig {
                    x_to_y_sizes: &duplicate,
                    y_to_x_sizes: &[],
                }
            ),
            Err(OrderBookError::SizesNotStrictlyIncreasing { .. })
        ));

        let mut incoherent = active_state();
        incoherent.latest_update_block = incoherent.snapshot_block + 1;
        assert_eq!(
            try_build_order_book(
                &incoherent,
                &OrderBookConfig {
                    x_to_y_sizes: &[],
                    y_to_x_sizes: &[],
                }
            ),
            Err(OrderBookError::SnapshotPrecedesLatestUpdate)
        );

        let mut impossible_validity = active_state();
        impossible_validity.max_execution_block = impossible_validity.snapshot_block - 1;
        assert_eq!(
            try_build_order_book(
                &impossible_validity,
                &OrderBookConfig {
                    x_to_y_sizes: &[],
                    y_to_x_sizes: &[],
                }
            ),
            Err(OrderBookError::ValidityPrecedesSnapshot)
        );

        let mut missing_multiplier = active_state();
        missing_multiplier.fee_multiplier = U256::ZERO;
        assert_eq!(
            try_build_order_book(
                &missing_multiplier,
                &OrderBookConfig {
                    x_to_y_sizes: &[],
                    y_to_x_sizes: &[],
                }
            ),
            Err(OrderBookError::FeeMultiplierIsZero)
        );
    }

    #[test]
    fn ladder_sweep_uses_floor_per_consumed_tranche() {
        let ladder = DirectionalLadder {
            levels: vec![
                OrderBookLevel {
                    size: U256::from(3u64),
                    price: PRICE_SCALE_X18 / U256::from(2u64),
                },
                OrderBookLevel {
                    size: U256::from(7u64),
                    price: PRICE_SCALE_X18 / U256::from(4u64),
                },
            ],
            truncated: false,
        };
        assert_eq!(
            try_ladder_amount_out(&ladder, U256::from(7u64)).unwrap(),
            Some(U256::from(2u64))
        );
        // A VWAP round-trip would incorrectly lose this one raw output unit.
        assert_eq!(
            try_ladder_amount_out(&ladder, U256::from(3u64)).unwrap(),
            Some(U256::from(1u64))
        );
        assert_eq!(
            try_ladder_amount_out(&ladder, U256::from(8u64)).unwrap(),
            None
        );
        assert_eq!(
            try_ladder_amount_out_at_cursor(&ladder, U256::from(3u64), U256::from(4u64)).unwrap(),
            Some(U256::from(1u64))
        );

        let malformed = DirectionalLadder {
            levels: vec![
                OrderBookLevel {
                    size: U256::from(2u64),
                    price: PRICE_SCALE_X18,
                },
                OrderBookLevel {
                    size: U256::from(1u64),
                    price: PRICE_SCALE_X18,
                },
            ],
            truncated: false,
        };
        assert_eq!(
            try_ladder_amount_out(&malformed, U256::from(1u64)),
            Err(OrderBookError::InvalidLadderSizeOrder { index: 1 })
        );
        let mut malformed = ladder.clone();
        malformed.levels[1].price = PRICE_SCALE_X18;
        assert_eq!(
            try_ladder_amount_out(&malformed, U256::from(1)),
            Err(OrderBookError::InvalidLadderPrice { index: 1 })
        );
        malformed.levels[1].price = U256::ZERO;
        assert_eq!(
            try_ladder_amount_out(&malformed, U256::from(1)),
            Err(OrderBookError::InvalidLadderPrice { index: 1 })
        );
        malformed.levels = vec![ladder.levels[0]; MAX_ORDER_BOOK_LEVELS + 1];
        assert_eq!(
            try_ladder_amount_out(&malformed, U256::ZERO),
            Err(OrderBookError::TooManyLadderLevels)
        );
    }

    #[test]
    fn sampled_projection_keeps_adapter_guard_explicit_for_integer_edge_cases() {
        let mut partial = active_state();
        partial.pool.sqrt_price_x96 = Q96 * U256::from(3u64) / U256::from(2u64);
        let partial_sizes = [U256::from(2u64)];
        let partial_book = try_build_order_book(
            &partial,
            &OrderBookConfig {
                x_to_y_sizes: &partial_sizes,
                y_to_x_sizes: &[],
            },
        )
        .unwrap();
        let promised = try_ladder_amount_out(&partial_book.x_to_y, U256::from(1u64))
            .unwrap()
            .unwrap();
        let executable = try_quote_x_to_y_with_multiplier(
            &partial.pool,
            U256::from(1u64),
            partial.fee_multiplier,
        )
        .unwrap()
        .amount_out;
        assert!(promised > executable);
        assert!(partial_book.requires_amount_out_minimum);

        let mut cursor = active_state();
        cursor.pool.sqrt_price_x96 = Q96 / U256::from(3u64);
        cursor.pool.reserve_x = 1;
        cursor.pool.reserve_y = 10;
        let cursor_sizes = [U256::from(40u64), U256::from(60u64)];
        let cursor_book = try_build_order_book(
            &cursor,
            &OrderBookConfig {
                x_to_y_sizes: &cursor_sizes,
                y_to_x_sizes: &[],
            },
        )
        .unwrap();
        let later_promised = try_ladder_amount_out_at_cursor(
            &cursor_book.x_to_y,
            U256::from(40u64),
            U256::from(20u64),
        )
        .unwrap()
        .unwrap();
        let first = crate::try_simulate_successful_swap(
            &cursor.pool,
            U256::from(40u64),
            Direction::XToY,
            cursor.fee_multiplier,
        )
        .unwrap();
        let later_executable = try_quote_x_to_y_with_multiplier(
            first.effective_params(),
            U256::from(20u64),
            cursor.fee_multiplier,
        )
        .unwrap()
        .amount_out;
        assert!(later_promised > later_executable);
        assert!(cursor_book.requires_amount_out_minimum);
    }
}
