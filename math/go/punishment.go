package lunarbasepmm

import (
	"fmt"

	"github.com/holiman/uint256"
)

// SwapSimulationStatus is the atomic outcome of a standard-token swap model.
// The zero value is reserved for a checked Into call that returned a math or
// domain error before it could produce a modeled swap outcome.
type SwapSimulationStatus uint8

const (
	// SimulationStatusUnknown means no modeled outcome was produced.
	SimulationStatusUnknown SwapSimulationStatus = iota
	// SimulationStatusApplied means quote, punishment, and reserves committed.
	SimulationStatusApplied
	// SimulationStatusSwapImpossible means the zero-output quote would revert.
	SimulationStatusSwapImpossible
	// SimulationStatusReserveTransitionOverflow means a post-transfer active
	// reserve would not fit into Solidity's uint112 storage.
	SimulationStatusReserveTransitionOverflow
	// SimulationStatusLaterRevert means a caller marked a later transfer or
	// accounting failure and the previously applied state was restored.
	SimulationStatusLaterRevert
)

// SwapRollbackReason identifies the Solidity revert whose transaction
// atomicity restores the pre-swap pool state.
type SwapRollbackReason uint8

const (
	// RollbackReasonSwapImpossible identifies a zero-output revert.
	RollbackReasonSwapImpossible SwapRollbackReason = iota + 1
	// RollbackReasonReserveTransitionOverflow identifies a uint112 reserve
	// downcast revert.
	RollbackReasonReserveTransitionOverflow
	// RollbackReasonLaterRevert identifies an external transfer or accounting
	// failure after the pure standard-token transition was modeled.
	RollbackReasonLaterRevert
)

type poolStateSnapshot struct {
	sqrtPriceX96     uint256.Int
	feeAskX24        uint32
	feeBidX24        uint32
	reserveX         uint256.Int
	reserveY         uint256.Int
	maxPunishmentX24 uint32
}

func snapshotPoolState(params *PoolParams) (snapshot poolStateSnapshot) {
	snapshot.sqrtPriceX96.Set(params.SqrtPriceX96)
	snapshot.feeAskX24 = params.FeeAskX24
	snapshot.feeBidX24 = params.FeeBidX24
	snapshot.reserveX.Set(params.ReserveX)
	snapshot.reserveY.Set(params.ReserveY)
	snapshot.maxPunishmentX24 = params.MaxPunishmentX24
	return snapshot
}

func poolStateMatches(params *PoolParams, snapshot *poolStateSnapshot) bool {
	return params.SqrtPriceX96.Cmp(&snapshot.sqrtPriceX96) == 0 &&
		params.FeeAskX24 == snapshot.feeAskX24 &&
		params.FeeBidX24 == snapshot.feeBidX24 &&
		params.ReserveX.Cmp(&snapshot.reserveX) == 0 &&
		params.ReserveY.Cmp(&snapshot.reserveY) == 0 &&
		params.MaxPunishmentX24 == snapshot.maxPunishmentX24
}

func restorePoolState(params *PoolParams, snapshot *poolStateSnapshot) {
	params.SqrtPriceX96.Set(&snapshot.sqrtPriceX96)
	params.FeeAskX24 = snapshot.feeAskX24
	params.FeeBidX24 = snapshot.feeBidX24
	params.ReserveX.Set(&snapshot.reserveX)
	params.ReserveY.Set(&snapshot.reserveY)
	params.MaxPunishmentX24 = snapshot.maxPunishmentX24
}

// SwapSimulationResult contains the triggering immediate-punishment quote,
// the desired and committed directional fee increments, and the atomic model
// outcome. SwapImpossible and reserve overflow are structured non-error
// outcomes: their quote, EffectiveFeeX24, and desired punishment remain
// available while AppliedPunishmentX24 is zero and PoolParams is unchanged.
type SwapSimulationResult struct {
	QuoteResult
	DesiredPunishmentX24 uint32
	AppliedPunishmentX24 uint32
	Executable           bool
	Status               SwapSimulationStatus

	rollbackPool *PoolParams
	preSwap      poolStateSnapshot
	committed    poolStateSnapshot
}

func newSwapSimulationResult() *SwapSimulationResult {
	return &SwapSimulationResult{QuoteResult: *newQuoteResult()}
}

func resetSwapSimulationResult(out *SwapSimulationResult) {
	resetQuoteResult(&out.QuoteResult)
	out.DesiredPunishmentX24 = 0
	out.AppliedPunishmentX24 = 0
	out.Executable = false
	out.Status = SimulationStatusUnknown
	out.rollbackPool = nil
	out.preSwap = poolStateSnapshot{}
	out.committed = poolStateSnapshot{}
}

func statusForRollback(reason SwapRollbackReason) (SwapSimulationStatus, error) {
	switch reason {
	case RollbackReasonSwapImpossible:
		return SimulationStatusSwapImpossible, nil
	case RollbackReasonReserveTransitionOverflow:
		return SimulationStatusReserveTransitionOverflow, nil
	case RollbackReasonLaterRevert:
		return SimulationStatusLaterRevert, nil
	default:
		return SimulationStatusUnknown, fmt.Errorf("%w: unknown rollback reason %d", ErrInvalidArgument, reason)
	}
}

// MarkRolledBack restores the pre-swap PoolParams captured by a previously
// applied simulation, preserving the counterfactual quote, EffectiveFeeX24,
// and desired punishment. It mirrors a later EVM transaction revert; callers
// normally pass RollbackReasonLaterRevert.
//
// The method refuses to overwrite the pool if it has changed since this
// result committed, so a stale simulation cannot roll back newer state.
func (out *SwapSimulationResult) MarkRolledBack(reason SwapRollbackReason) error {
	if out == nil {
		return fmt.Errorf("%w: nil SwapSimulationResult", ErrInvalidArgument)
	}
	status, err := statusForRollback(reason)
	if err != nil {
		return err
	}
	if out.Status != SimulationStatusApplied || out.rollbackPool == nil {
		return fmt.Errorf("%w: only an applied simulation can be rolled back", ErrInvalidArgument)
	}
	if err := validateMutablePoolParams(out.rollbackPool); err != nil {
		return err
	}
	if !poolStateMatches(out.rollbackPool, &out.committed) {
		return fmt.Errorf("%w: PoolParams changed after simulation commit", ErrInvalidArgument)
	}

	restorePoolState(out.rollbackPool, &out.preSwap)
	out.AppliedPunishmentX24 = 0
	out.Executable = false
	out.Status = status
	out.rollbackPool = nil
	out.committed = poolStateSnapshot{}
	return nil
}

// DesiredPunishmentX24 returns SwapLib.punishmentX24 for a hypothetical
// successful exact-input swap. The denominator uses pre-swap active reserves.
// MaxUint24 is interpreted as conceptual Q24 and any conceptual-Q24 result is
// encoded back to the uint24 sentinel.
func DesiredPunishmentX24(params *PoolParams, amountIn *uint256.Int, direction SwapDirection) (uint32, error) {
	if err := ValidatePoolParams(params); err != nil {
		return 0, err
	}
	if amountIn == nil {
		return 0, fmt.Errorf("%w: nil amountIn", ErrInvalidArgument)
	}
	if err := validateDirection(direction); err != nil {
		return 0, err
	}
	return desiredPunishmentX24Validated(params, amountIn, direction)
}

func desiredPunishmentX24Validated(params *PoolParams, amountIn *uint256.Int, direction SwapDirection) (uint32, error) {
	if amountIn.IsZero() || params.MaxPunishmentX24 == 0 || params.SqrtPriceX96.IsZero() {
		return 0, nil
	}

	var xInventoryValue, inventoryWealth uint256.Int
	if err := xValueInYInto(&xInventoryValue, params.ReserveX, params.SqrtPriceX96); err != nil {
		return 0, err
	}
	if _, overflow := inventoryWealth.AddOverflow(&xInventoryValue, params.ReserveY); overflow {
		return 0, ErrMathOverflow
	}
	if inventoryWealth.IsZero() {
		return 0, nil
	}

	var swapWealth uint256.Int
	if direction == DirectionXToY {
		if err := xValueInYInto(&swapWealth, amountIn, params.SqrtPriceX96); err != nil {
			return 0, err
		}
	} else {
		swapWealth.Set(amountIn)
	}
	if swapWealth.IsZero() {
		return 0, nil
	}

	var effectiveMaximum, calculated uint256.Int
	if params.MaxPunishmentX24 == MaxUint24 {
		effectiveMaximum.SetUint64(uint64(Q24Scale))
	} else {
		effectiveMaximum.SetUint64(uint64(params.MaxPunishmentX24))
	}
	if !swapWealth.Lt(&inventoryWealth) {
		calculated.Set(&effectiveMaximum)
	} else if err := mulDivUpChecked(&calculated, &effectiveMaximum, &swapWealth, &inventoryWealth); err != nil {
		return 0, err
	}

	if !calculated.Lt(q24) {
		return MaxUint24, nil
	}
	return uint32(calculated.Uint64()), nil
}

func xValueInYInto(dst, amountX, anchor *uint256.Int) error {
	var scratch uint256.Int
	if err := mulDivDownChecked(&scratch, amountX, anchor, q96); err != nil {
		return err
	}
	return mulDivDownChecked(dst, &scratch, anchor, q96)
}

// immediateFeeX24Validated computes the desired punishment and the saturating
// directional fee charged by the triggering quote. Callers must validate the
// pool, amount, and direction first.
func immediateFeeX24Validated(
	params *PoolParams,
	amountIn *uint256.Int,
	direction SwapDirection,
) (desiredPunishmentX24, effectiveFeeX24 uint32, err error) {
	desiredPunishmentX24, err = desiredPunishmentX24Validated(params, amountIn, direction)
	if err != nil {
		return 0, 0, err
	}

	storedFeeX24 := params.FeeBidX24
	if direction == DirectionYToX {
		storedFeeX24 = params.FeeAskX24
	}
	return desiredPunishmentX24, saturatingFeeX24(storedFeeX24, desiredPunishmentX24), nil
}

// saturatingFeeX24 mirrors SwapLib.saturatingFeeX24 exactly, including the
// zero-headroom case where stored and effective fee are both the sentinel.
func saturatingFeeX24(storedFeeX24, desiredPunishmentX24 uint32) uint32 {
	available := MaxUint24 - storedFeeX24
	if desiredPunishmentX24 >= available {
		return MaxUint24
	}
	return storedFeeX24 + desiredPunishmentX24
}

// TransitionDirectionalFees computes the saturating fee used by the current
// quote. The result is pure: callers decide whether to persist it after the
// rest of a swap has succeeded.
func TransitionDirectionalFees(
	feeAskX24, feeBidX24 uint32,
	desiredPunishmentX24 uint32,
	direction SwapDirection,
) (nextFeeAskX24, nextFeeBidX24, appliedPunishmentX24 uint32, err error) {
	if err = validateUint24("FeeAskX24", feeAskX24); err != nil {
		return 0, 0, 0, err
	}
	if err = validateUint24("FeeBidX24", feeBidX24); err != nil {
		return 0, 0, 0, err
	}
	if err = validateUint24("desiredPunishmentX24", desiredPunishmentX24); err != nil {
		return 0, 0, 0, err
	}
	if err = validateDirection(direction); err != nil {
		return 0, 0, 0, err
	}

	nextFeeAskX24, nextFeeBidX24 = feeAskX24, feeBidX24
	currentFee := feeBidX24
	if direction == DirectionYToX {
		currentFee = feeAskX24
	}
	effectiveFeeX24 := saturatingFeeX24(currentFee, desiredPunishmentX24)
	appliedPunishmentX24 = effectiveFeeX24 - currentFee
	if direction == DirectionXToY {
		nextFeeBidX24 = effectiveFeeX24
	} else {
		nextFeeAskX24 = effectiveFeeX24
	}
	return nextFeeAskX24, nextFeeBidX24, appliedPunishmentX24, nil
}

// ApplyDirectionalPunishment commits only the saturating directional fee
// transition. It is useful when reserve and token movement are modeled by the
// caller.
func ApplyDirectionalPunishment(params *PoolParams, desiredPunishmentX24 uint32, direction SwapDirection) (uint32, error) {
	if err := ValidatePoolParams(params); err != nil {
		return 0, err
	}
	nextAsk, nextBid, applied, err := TransitionDirectionalFees(
		params.FeeAskX24,
		params.FeeBidX24,
		desiredPunishmentX24,
		direction,
	)
	if err != nil {
		return 0, err
	}
	params.FeeAskX24 = nextAsk
	params.FeeBidX24 = nextBid
	return applied, nil
}

// ApplyStateUpdate mirrors Pool.upd for the math state: it atomically replaces
// the anchor and both effective directional fees, thereby resetting accumulated
// punishment. Reserves and MaxPunishmentX24 are preserved.
func ApplyStateUpdate(params *PoolParams, anchor *uint256.Int, feeAskX24, feeBidX24 uint32) error {
	if params == nil {
		return fmt.Errorf("%w: nil PoolParams", ErrInvalidArgument)
	}
	if params.SqrtPriceX96 == nil {
		return fmt.Errorf("%w: nil PoolParams.SqrtPriceX96", ErrInvalidArgument)
	}
	if params.SqrtPriceX96 == params.ReserveX || params.SqrtPriceX96 == params.ReserveY || params.ReserveX == params.ReserveY {
		return fmt.Errorf("%w: mutable PoolParams numeric fields must not alias", ErrInvalidArgument)
	}
	candidate := *params
	candidate.SqrtPriceX96 = anchor
	candidate.FeeAskX24 = feeAskX24
	candidate.FeeBidX24 = feeBidX24
	if err := ValidatePoolParams(&candidate); err != nil {
		return err
	}
	params.SqrtPriceX96.Set(anchor)
	params.FeeAskX24 = feeAskX24
	params.FeeBidX24 = feeBidX24
	return nil
}

// SimulateStandardTokenSwap models one standard-token swap with
// feeMultiplier=1. The quote charges the triggering punishment. A successful
// result mutates params atomically. SwapImpossible and reserve overflow return
// structured rollback statuses with nil error and leave params unchanged;
// arithmetic and invalid-domain failures still return errors.
func SimulateStandardTokenSwap(
	params *PoolParams,
	amountIn *uint256.Int,
	direction SwapDirection,
) (*SwapSimulationResult, error) {
	return SimulateStandardTokenSwapWithMultiplier(params, amountIn, one, direction)
}

// SimulateStandardTokenSwapWithMultiplier is the allocating multiplier
// variant of SimulateStandardTokenSwap. A non-nil result is returned for every
// completed quote, including modeled rollback outcomes.
func SimulateStandardTokenSwapWithMultiplier(
	params *PoolParams,
	amountIn, feeMultiplier *uint256.Int,
	direction SwapDirection,
) (*SwapSimulationResult, error) {
	out := newSwapSimulationResult()
	if err := SimulateStandardTokenSwapInto(out, params, amountIn, feeMultiplier, direction); err != nil {
		return nil, err
	}
	return out, nil
}

// SimulateStandardTokenSwapInto is the allocation-free state-transition API.
// out's numeric fields must be preallocated and must not alias params. After a
// math or domain error, a structurally valid out is reset to deterministic zero
// values with SimulationStatusUnknown; it never retains a partial quote from a
// previous call.
func SimulateStandardTokenSwapInto(
	out *SwapSimulationResult,
	params *PoolParams,
	amountIn, feeMultiplier *uint256.Int,
	direction SwapDirection,
) (err error) {
	if out == nil {
		return fmt.Errorf("%w: nil SwapSimulationResult", ErrInvalidArgument)
	}
	if err := validateQuoteDestination(&out.QuoteResult, params); err != nil {
		return err
	}
	if params != nil && (amountIn == params.SqrtPriceX96 || amountIn == params.ReserveX || amountIn == params.ReserveY ||
		feeMultiplier == params.SqrtPriceX96 || feeMultiplier == params.ReserveX || feeMultiplier == params.ReserveY) {
		return fmt.Errorf("%w: swap inputs must not alias mutable PoolParams", ErrInvalidArgument)
	}
	if amountIn != nil && (out.AmountOut == amountIn || out.SqrtPriceNext == amountIn || out.Fee == amountIn) {
		return fmt.Errorf("%w: SwapSimulationResult must not alias amountIn", ErrInvalidArgument)
	}
	if feeMultiplier != nil && (out.AmountOut == feeMultiplier || out.SqrtPriceNext == feeMultiplier || out.Fee == feeMultiplier) {
		return fmt.Errorf("%w: SwapSimulationResult must not alias feeMultiplier", ErrInvalidArgument)
	}
	resetSwapSimulationResult(out)
	defer func() {
		if err != nil {
			resetSwapSimulationResult(out)
		}
	}()
	if err := validateDirection(direction); err != nil {
		return err
	}
	if err := validateMutablePoolParams(params); err != nil {
		return err
	}

	var desired, effective uint32
	var quoteErr error
	if direction == DirectionXToY {
		desired, effective, quoteErr = quoteXToYWithMultiplierIntoChecked(
			&out.QuoteResult,
			params,
			amountIn,
			feeMultiplier,
		)
	} else {
		desired, effective, quoteErr = quoteYToXWithMultiplierIntoChecked(
			&out.QuoteResult,
			params,
			amountIn,
			feeMultiplier,
		)
	}
	if quoteErr != nil {
		return quoteErr
	}
	out.DesiredPunishmentX24 = desired
	if out.AmountOut.IsZero() {
		out.Status = SimulationStatusSwapImpossible
		return nil
	}

	storedFeeX24 := params.FeeBidX24
	if direction == DirectionYToX {
		storedFeeX24 = params.FeeAskX24
	}
	applied := effective - storedFeeX24

	var grossOutput uint256.Int
	if _, overflow := grossOutput.AddOverflow(out.AmountOut, out.Fee); overflow {
		return ErrMathOverflow
	}
	var nextReserveX, nextReserveY uint256.Int
	if direction == DirectionXToY {
		if _, overflow := nextReserveX.AddOverflow(params.ReserveX, amountIn); overflow || nextReserveX.BitLen() > 112 {
			out.Status = SimulationStatusReserveTransitionOverflow
			return nil
		}
		if _, underflow := nextReserveY.SubOverflow(params.ReserveY, &grossOutput); underflow {
			out.Status = SimulationStatusReserveTransitionOverflow
			return nil
		}
	} else {
		if _, overflow := nextReserveY.AddOverflow(params.ReserveY, amountIn); overflow || nextReserveY.BitLen() > 112 {
			out.Status = SimulationStatusReserveTransitionOverflow
			return nil
		}
		if _, underflow := nextReserveX.SubOverflow(params.ReserveX, &grossOutput); underflow {
			out.Status = SimulationStatusReserveTransitionOverflow
			return nil
		}
	}

	preSwap := snapshotPoolState(params)
	params.ReserveX.Set(&nextReserveX)
	params.ReserveY.Set(&nextReserveY)
	if direction == DirectionXToY {
		params.FeeBidX24 = effective
	} else {
		params.FeeAskX24 = effective
	}
	out.AppliedPunishmentX24 = applied
	out.Executable = true
	out.Status = SimulationStatusApplied
	out.rollbackPool = params
	out.preSwap = preSwap
	out.committed = snapshotPoolState(params)
	return nil
}
