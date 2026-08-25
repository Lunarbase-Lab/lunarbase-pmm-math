// Package lunarbasepmm is the integer-exact Go mirror of the on-chain
// LunarBase Pool quote path.
//
// The pool has a single operator-published Q64.96 sqrt-price. Swaps are quoted
// linearly at that anchor, charge a directional Q24 fee, and leave the
// informational next price equal to the anchor. Every quote includes the
// triggering swap's punishment in its effective directional fee; successful
// swaps persist that same fee. See punishment.go.
package lunarbasepmm

import (
	"errors"
	"fmt"

	"github.com/holiman/uint256"
)

const (
	// Q24Scale is the conceptual Q24 value representing 100%.
	Q24Scale uint32 = 1 << 24
	// MaxUint24 is the largest storable uint24. For fees and maximum
	// punishment it is a sentinel representing conceptual Q24 (100%).
	MaxUint24 uint32 = Q24Scale - 1
)

var (
	// ErrInvalidArgument reports a nil, aliased, or otherwise malformed Go API value.
	ErrInvalidArgument = errors.New("invalid argument")
	// ErrValueOutOfRange reports a value outside its Solidity ABI or storage width.
	ErrValueOutOfRange = errors.New("value out of Solidity range")
	// ErrDivisionByZero mirrors a Solidity division-by-zero revert.
	ErrDivisionByZero = errors.New("division by zero")
	// ErrMathOverflow mirrors a Solidity uint256 arithmetic revert.
	ErrMathOverflow = errors.New("uint256 math overflow")
	// ErrSwapImpossible identifies PeripheryLib's zero-output SwapImpossible
	// revert. Standard-token simulations expose this as a structured status;
	// the error remains available for callers classifying on-chain reverts.
	ErrSwapImpossible = errors.New("swap impossible")
	// ErrReserveTransition identifies a post-swap active reserve outside
	// uint112. Standard-token simulations expose this as a structured status.
	ErrReserveTransition = errors.New("reserve transition out of uint112 range")
)

// SwapDirection selects the on-chain directional fee and punishment state.
type SwapDirection uint8

const (
	// DirectionXToY charges and widens the bid fee.
	DirectionXToY SwapDirection = iota
	// DirectionYToX charges and widens the ask fee.
	DirectionYToX
)

func validateDirection(direction SwapDirection) error {
	if direction != DirectionXToY && direction != DirectionYToX {
		return fmt.Errorf("%w: unknown swap direction %d", ErrInvalidArgument, direction)
	}
	return nil
}

// PoolParams is the input snapshot needed to quote a swap.
//
// Runtime validation follows the contract widths exactly: SqrtPriceX96 is a
// uint160 Q64.96 anchor, FeeAskX24/FeeBidX24/MaxPunishmentX24 are uint24, and
// ReserveX/ReserveY are active uint112 reserves.
type PoolParams struct {
	SqrtPriceX96     *uint256.Int
	FeeAskX24        uint32
	FeeBidX24        uint32
	ReserveX         *uint256.Int
	ReserveY         *uint256.Int
	MaxPunishmentX24 uint32
}

func validateMutablePoolParams(params *PoolParams) error {
	if err := ValidatePoolParams(params); err != nil {
		return err
	}
	if params.SqrtPriceX96 == params.ReserveX || params.SqrtPriceX96 == params.ReserveY || params.ReserveX == params.ReserveY {
		return fmt.Errorf("%w: mutable PoolParams numeric fields must not alias", ErrInvalidArgument)
	}
	return nil
}

// QuoteResult holds the output of QuoteXToY / QuoteYToX.
//
// AmountOut is net of Fee. SqrtPriceNext always equals SqrtPriceX96, including
// rejected quotes.
type QuoteResult struct {
	AmountOut     *uint256.Int
	SqrtPriceNext *uint256.Int
	Fee           *uint256.Int
	// EffectiveFeeX24 is the saturating directional fee used by this quote
	// before the caller multiplier. It remains informative for rejected quotes.
	EffectiveFeeX24 uint32
}

func newQuoteResult() *QuoteResult {
	return &QuoteResult{
		AmountOut:     new(uint256.Int),
		SqrtPriceNext: new(uint256.Int),
		Fee:           new(uint256.Int),
	}
}

func resetQuoteResult(out *QuoteResult) {
	out.AmountOut.Clear()
	out.SqrtPriceNext.Clear()
	out.Fee.Clear()
	out.EffectiveFeeX24 = 0
}

// ValidatePoolParams checks every runtime ABI/storage width used by the Go
// mirror. A zero anchor or zero reserve is valid, exactly as in Solidity.
func ValidatePoolParams(params *PoolParams) error {
	if params == nil {
		return fmt.Errorf("%w: nil PoolParams", ErrInvalidArgument)
	}
	if err := validateUintWidth("SqrtPriceX96", params.SqrtPriceX96, 160); err != nil {
		return err
	}
	if err := validateUint24("FeeAskX24", params.FeeAskX24); err != nil {
		return err
	}
	if err := validateUint24("FeeBidX24", params.FeeBidX24); err != nil {
		return err
	}
	if err := validateUintWidth("ReserveX", params.ReserveX, 112); err != nil {
		return err
	}
	if err := validateUintWidth("ReserveY", params.ReserveY, 112); err != nil {
		return err
	}
	return validateUint24("MaxPunishmentX24", params.MaxPunishmentX24)
}

func validateUintWidth(name string, value *uint256.Int, bits int) error {
	if value == nil {
		return fmt.Errorf("%w: nil %s", ErrInvalidArgument, name)
	}
	if value.BitLen() > bits {
		return fmt.Errorf("%w: %s exceeds uint%d", ErrValueOutOfRange, name, bits)
	}
	return nil
}

func validateUint24(name string, value uint32) error {
	if value > MaxUint24 {
		return fmt.Errorf("%w: %s exceeds uint24", ErrValueOutOfRange, name)
	}
	return nil
}

func validateQuoteDestination(out *QuoteResult, params *PoolParams) error {
	if out == nil || out.AmountOut == nil || out.SqrtPriceNext == nil || out.Fee == nil {
		return fmt.Errorf("%w: QuoteResult and all numeric fields must be non-nil", ErrInvalidArgument)
	}
	if out.AmountOut == out.SqrtPriceNext || out.AmountOut == out.Fee || out.SqrtPriceNext == out.Fee {
		return fmt.Errorf("%w: QuoteResult fields must not alias", ErrInvalidArgument)
	}
	if params != nil &&
		(out.AmountOut == params.SqrtPriceX96 || out.AmountOut == params.ReserveX || out.AmountOut == params.ReserveY ||
			out.SqrtPriceNext == params.SqrtPriceX96 || out.SqrtPriceNext == params.ReserveX || out.SqrtPriceNext == params.ReserveY ||
			out.Fee == params.SqrtPriceX96 || out.Fee == params.ReserveX || out.Fee == params.ReserveY) {
		return fmt.Errorf("%w: QuoteResult must not alias PoolParams", ErrInvalidArgument)
	}
	return nil
}

func validateQuoteCall(out *QuoteResult, params *PoolParams, amountIn, feeMultiplier *uint256.Int) error {
	// Validate the destination first. Once this succeeds it is safe for the
	// checked Into APIs to clear a previous result on every later error without
	// accidentally mutating aliased inputs or pool state.
	if err := validateQuoteDestination(out, params); err != nil {
		return err
	}
	if amountIn != nil && (out.AmountOut == amountIn || out.SqrtPriceNext == amountIn || out.Fee == amountIn) {
		return fmt.Errorf("%w: QuoteResult must not alias amountIn", ErrInvalidArgument)
	}
	if feeMultiplier != nil && (out.AmountOut == feeMultiplier || out.SqrtPriceNext == feeMultiplier || out.Fee == feeMultiplier) {
		return fmt.Errorf("%w: QuoteResult must not alias feeMultiplier", ErrInvalidArgument)
	}
	resetQuoteResult(out)
	if err := ValidatePoolParams(params); err != nil {
		return err
	}
	if amountIn == nil {
		return fmt.Errorf("%w: nil amountIn", ErrInvalidArgument)
	}
	if feeMultiplier == nil {
		return fmt.Errorf("%w: nil feeMultiplier", ErrInvalidArgument)
	}
	return nil
}

// QuoteXToY quotes an exact-input X -> Y swap with feeMultiplier=1. It keeps
// the ergonomic API and panics only for an invalid off-chain domain or a
// Solidity-equivalent arithmetic revert. Use QuoteXToYChecked when inputs are
// not already ABI-validated.
func QuoteXToY(params *PoolParams, dx *uint256.Int) *QuoteResult {
	out, err := QuoteXToYChecked(params, dx)
	if err != nil {
		panic(err)
	}
	return out
}

// QuoteYToX is the reverse-direction counterpart of QuoteXToY.
func QuoteYToX(params *PoolParams, dy *uint256.Int) *QuoteResult {
	out, err := QuoteYToXChecked(params, dy)
	if err != nil {
		panic(err)
	}
	return out
}

// QuoteXToYWithMultiplier quotes X -> Y with the caller's fee multiplier.
// Multipliers 0 and 1 both select the base-fee path, matching SwapLib.applyFee.
func QuoteXToYWithMultiplier(params *PoolParams, dx, feeMultiplier *uint256.Int) *QuoteResult {
	out, err := QuoteXToYWithMultiplierChecked(params, dx, feeMultiplier)
	if err != nil {
		panic(err)
	}
	return out
}

// QuoteYToXWithMultiplier is the reverse-direction multiplier variant.
func QuoteYToXWithMultiplier(params *PoolParams, dy, feeMultiplier *uint256.Int) *QuoteResult {
	out, err := QuoteYToXWithMultiplierChecked(params, dy, feeMultiplier)
	if err != nil {
		panic(err)
	}
	return out
}

// QuoteXToYChecked is the allocating, error-returning quote API.
func QuoteXToYChecked(params *PoolParams, dx *uint256.Int) (*QuoteResult, error) {
	return QuoteXToYWithMultiplierChecked(params, dx, one)
}

// QuoteYToXChecked is the allocating, error-returning quote API.
func QuoteYToXChecked(params *PoolParams, dy *uint256.Int) (*QuoteResult, error) {
	return QuoteYToXWithMultiplierChecked(params, dy, one)
}

// QuoteXToYWithMultiplierChecked allocates a result and returns arithmetic or
// runtime-domain reverts as Go errors.
func QuoteXToYWithMultiplierChecked(params *PoolParams, dx, feeMultiplier *uint256.Int) (*QuoteResult, error) {
	out := newQuoteResult()
	if err := QuoteXToYWithMultiplierIntoChecked(out, params, dx, feeMultiplier); err != nil {
		return nil, err
	}
	return out, nil
}

// QuoteYToXWithMultiplierChecked is the reverse-direction checked variant.
func QuoteYToXWithMultiplierChecked(params *PoolParams, dy, feeMultiplier *uint256.Int) (*QuoteResult, error) {
	out := newQuoteResult()
	if err := QuoteYToXWithMultiplierIntoChecked(out, params, dy, feeMultiplier); err != nil {
		return nil, err
	}
	return out, nil
}

// QuoteXToYInto computes the quote and writes the result into out.
// Allocation-free on the hot path. The caller owns out and its three
// distinct `*uint256.Int` fields; they must not alias PoolParams.
func QuoteXToYInto(out *QuoteResult, params *PoolParams, dx *uint256.Int) *QuoteResult {
	if err := QuoteXToYIntoChecked(out, params, dx); err != nil {
		panic(err)
	}
	return out
}

// QuoteYToXInto mirrors [QuoteXToYInto] for the reverse direction.
func QuoteYToXInto(out *QuoteResult, params *PoolParams, dy *uint256.Int) *QuoteResult {
	if err := QuoteYToXIntoChecked(out, params, dy); err != nil {
		panic(err)
	}
	return out
}

// QuoteXToYWithMultiplierInto is the allocation-free multiplier API.
func QuoteXToYWithMultiplierInto(out *QuoteResult, params *PoolParams, dx, feeMultiplier *uint256.Int) *QuoteResult {
	if err := QuoteXToYWithMultiplierIntoChecked(out, params, dx, feeMultiplier); err != nil {
		panic(err)
	}
	return out
}

// QuoteYToXWithMultiplierInto is the reverse-direction allocation-free
// multiplier API.
func QuoteYToXWithMultiplierInto(out *QuoteResult, params *PoolParams, dy, feeMultiplier *uint256.Int) *QuoteResult {
	if err := QuoteYToXWithMultiplierIntoChecked(out, params, dy, feeMultiplier); err != nil {
		panic(err)
	}
	return out
}

// QuoteXToYIntoChecked computes the quote and returns Solidity-equivalent
// arithmetic reverts as Go errors.
func QuoteXToYIntoChecked(out *QuoteResult, params *PoolParams, dx *uint256.Int) error {
	return QuoteXToYWithMultiplierIntoChecked(out, params, dx, one)
}

// QuoteYToXIntoChecked is the reverse-direction checked Into API.
func QuoteYToXIntoChecked(out *QuoteResult, params *PoolParams, dy *uint256.Int) error {
	return QuoteYToXWithMultiplierIntoChecked(out, params, dy, one)
}

// QuoteXToYWithMultiplierIntoChecked mirrors SwapLib.quoteXToY. The triggering
// swap's punishment is included in the effective bid fee before applyFee, and
// the price conversion preserves Solidity's two separate floor operations.
func QuoteXToYWithMultiplierIntoChecked(out *QuoteResult, params *PoolParams, dx, feeMultiplier *uint256.Int) error {
	_, _, err := quoteXToYWithMultiplierIntoChecked(out, params, dx, feeMultiplier)
	return err
}

func quoteXToYWithMultiplierIntoChecked(
	out *QuoteResult,
	params *PoolParams,
	dx, feeMultiplier *uint256.Int,
) (desiredPunishmentX24, effectiveFeeX24 uint32, err error) {
	if err := validateQuoteCall(out, params, dx, feeMultiplier); err != nil {
		return 0, 0, err
	}
	defer func() {
		if err != nil {
			resetQuoteResult(out)
		}
	}()

	desiredPunishmentX24, effectiveFeeX24, err = immediateFeeX24Validated(params, dx, DirectionXToY)
	if err != nil {
		return 0, 0, err
	}
	out.EffectiveFeeX24 = effectiveFeeX24

	var scratch, grossOutput, amountOut, fee uint256.Int
	if err := mulDivDownChecked(&scratch, dx, params.SqrtPriceX96, q96); err != nil {
		return 0, 0, err
	}
	if err := mulDivDownChecked(&grossOutput, &scratch, params.SqrtPriceX96, q96); err != nil {
		return 0, 0, err
	}
	if grossOutput.IsZero() || grossOutput.Gt(params.ReserveY) {
		writeRejected(out, params.SqrtPriceX96)
		return desiredPunishmentX24, effectiveFeeX24, nil
	}
	if err := applyFeeInto(&amountOut, &fee, &grossOutput, effectiveFeeX24, feeMultiplier); err != nil {
		return 0, 0, err
	}
	writeQuote(out, &amountOut, params.SqrtPriceX96, &fee)
	return desiredPunishmentX24, effectiveFeeX24, nil
}

// QuoteYToXWithMultiplierIntoChecked mirrors SwapLib.quoteYToX and applies the
// immediate punishment to the effective ask fee.
func QuoteYToXWithMultiplierIntoChecked(out *QuoteResult, params *PoolParams, dy, feeMultiplier *uint256.Int) error {
	_, _, err := quoteYToXWithMultiplierIntoChecked(out, params, dy, feeMultiplier)
	return err
}

func quoteYToXWithMultiplierIntoChecked(
	out *QuoteResult,
	params *PoolParams,
	dy, feeMultiplier *uint256.Int,
) (desiredPunishmentX24, effectiveFeeX24 uint32, err error) {
	if err := validateQuoteCall(out, params, dy, feeMultiplier); err != nil {
		return 0, 0, err
	}
	defer func() {
		if err != nil {
			resetQuoteResult(out)
		}
	}()

	desiredPunishmentX24, effectiveFeeX24, err = immediateFeeX24Validated(params, dy, DirectionYToX)
	if err != nil {
		return 0, 0, err
	}
	out.EffectiveFeeX24 = effectiveFeeX24
	if params.SqrtPriceX96.IsZero() {
		writeRejected(out, params.SqrtPriceX96)
		return desiredPunishmentX24, effectiveFeeX24, nil
	}

	var scratch, grossOutput, amountOut, fee uint256.Int
	if err := mulDivDownChecked(&scratch, dy, q96, params.SqrtPriceX96); err != nil {
		return 0, 0, err
	}
	if err := mulDivDownChecked(&grossOutput, &scratch, q96, params.SqrtPriceX96); err != nil {
		return 0, 0, err
	}
	if grossOutput.IsZero() || grossOutput.Gt(params.ReserveX) {
		writeRejected(out, params.SqrtPriceX96)
		return desiredPunishmentX24, effectiveFeeX24, nil
	}
	if err := applyFeeInto(&amountOut, &fee, &grossOutput, effectiveFeeX24, feeMultiplier); err != nil {
		return 0, 0, err
	}
	writeQuote(out, &amountOut, params.SqrtPriceX96, &fee)
	return desiredPunishmentX24, effectiveFeeX24, nil
}

func applyFeeInto(amountOut, fee, grossOutput *uint256.Int, feeQ24 uint32, feeMultiplier *uint256.Int) error {
	if feeQ24 == MaxUint24 {
		amountOut.Clear()
		fee.Set(grossOutput)
		return nil
	}

	var feeValue, baseFee uint256.Int
	feeValue.SetUint64(uint64(feeQ24))
	if err := mulDivDownChecked(&baseFee, grossOutput, &feeValue, q24); err != nil {
		return err
	}
	if !feeMultiplier.Gt(one) || baseFee.IsZero() {
		fee.Set(&baseFee)
		amountOut.Sub(grossOutput, &baseFee)
		return nil
	}

	var scaledFee uint256.Int
	if _, overflow := scaledFee.MulOverflow(&baseFee, feeMultiplier); overflow || !scaledFee.Lt(grossOutput) {
		amountOut.Clear()
		fee.Set(grossOutput)
		return nil
	}
	fee.Set(&scaledFee)
	amountOut.Sub(grossOutput, &scaledFee)
	return nil
}

func writeQuote(out *QuoteResult, amountOut, anchor, fee *uint256.Int) {
	out.AmountOut.Set(amountOut)
	out.SqrtPriceNext.Set(anchor)
	out.Fee.Set(fee)
}

func writeRejected(out *QuoteResult, anchor *uint256.Int) {
	out.AmountOut.Clear()
	out.SqrtPriceNext.Set(anchor)
	out.Fee.Clear()
}
