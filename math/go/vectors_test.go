package lunarbasepmm

import (
	"bufio"
	"encoding/json"
	"os"
	"strconv"
	"testing"

	"github.com/holiman/uint256"
	"github.com/stretchr/testify/require"
)

const (
	vectorApplied                   = "Applied"
	vectorSwapImpossible            = "SwapImpossible"
	vectorReserveTransitionOverflow = "ReserveTransitionOverflow"
	vectorMathMulDivRevert          = "MathMulDivRevert"
)

type parityUpdate struct {
	AnchorPrice           string `json:"anchorPrice"`
	FeeAskX24             string `json:"feeAskX24"`
	FeeBidX24             string `json:"feeBidX24"`
	AnchorPriceAfter      string `json:"anchorPriceAfter"`
	FeeAskX24After        string `json:"feeAskX24After"`
	FeeBidX24After        string `json:"feeBidX24After"`
	ReserveXAfter         string `json:"reserveXAfter"`
	ReserveYAfter         string `json:"reserveYAfter"`
	MaxPunishmentX24After string `json:"maxPunishmentX24After"`
}

type parityVector struct {
	SchemaVersion        uint32        `json:"schemaVersion"`
	Name                 string        `json:"name"`
	Seed                 string        `json:"seed"`
	Dir                  string        `json:"dir"`
	AnchorPrice          string        `json:"anchorPrice"`
	FeeAskX24            string        `json:"feeAskX24"`
	FeeBidX24            string        `json:"feeBidX24"`
	ReserveX             string        `json:"reserveX"`
	ReserveY             string        `json:"reserveY"`
	MaxPunishmentX24     string        `json:"maxPunishmentX24"`
	FeeMultiplier        string        `json:"feeMultiplier"`
	AmountIn             string        `json:"amountIn"`
	AmountOut            string        `json:"amountOut"`
	PNext                string        `json:"pNext"`
	FeeAmount            string        `json:"feeAmount"`
	Outcome              string        `json:"outcome"`
	RevertSelector       string        `json:"revertSelector"`
	RevertClass          string        `json:"revertClass"`
	DesiredPunishmentX24 string        `json:"desiredPunishmentX24"`
	EffectiveFeeX24      string        `json:"effectiveFeeX24"`
	AppliedPunishmentX24 string        `json:"appliedPunishmentX24"`
	FeeAskX24After       string        `json:"feeAskX24After"`
	FeeBidX24After       string        `json:"feeBidX24After"`
	ReserveXAfter        string        `json:"reserveXAfter"`
	ReserveYAfter        string        `json:"reserveYAfter"`
	Update               *parityUpdate `json:"update"`
}

func (v parityVector) label(line int) string {
	identity := v.Name
	if identity == "" {
		identity = v.Seed
	}
	if identity == "" {
		identity = v.Dir
	}
	return v.Dir + " " + identity + " line " + strconv.Itoa(line)
}

func parseVectorU32(t *testing.T, value string) uint32 {
	t.Helper()
	parsed, err := strconv.ParseUint(value, 10, 32)
	require.NoError(t, err)
	return uint32(parsed)
}

func clonePoolParams(params *PoolParams) *PoolParams {
	return &PoolParams{
		SqrtPriceX96:     new(uint256.Int).Set(params.SqrtPriceX96),
		FeeAskX24:        params.FeeAskX24,
		FeeBidX24:        params.FeeBidX24,
		ReserveX:         new(uint256.Int).Set(params.ReserveX),
		ReserveY:         new(uint256.Int).Set(params.ReserveY),
		MaxPunishmentX24: params.MaxPunishmentX24,
	}
}

func expectedVectorRevert(t *testing.T, outcome string) (string, string) {
	t.Helper()
	switch outcome {
	case vectorApplied:
		return "0x00000000", "None"
	case vectorSwapImpossible:
		return "0x4a45e749", "SwapImpossible()"
	case vectorReserveTransitionOverflow:
		return "0x6dfcc650", "SafeCastOverflowedUintDowncast(uint8,uint256)"
	case vectorMathMulDivRevert:
		return "0x4e487b71", "Panic(0x11)"
	default:
		t.Fatalf("unknown vector outcome %q", outcome)
		return "", ""
	}
}

func runVectorFile(t *testing.T, path string) {
	t.Helper()
	file, err := os.Open(path)
	require.NoError(t, err)
	defer func() { require.NoError(t, file.Close()) }()

	var total, xToY, yToX, applied, swapImpossible, reserveOverflow, mathRevert, updates int
	scanner := bufio.NewScanner(file)
	scanner.Buffer(make([]byte, 64*1024), 1024*1024)
	for line := 1; scanner.Scan(); line++ {
		if scanner.Text() == "" {
			continue
		}
		var vector parityVector
		require.NoError(t, json.Unmarshal(scanner.Bytes(), &vector), "%s line %d", path, line)
		require.Equal(t, uint32(4), vector.SchemaVersion, "%s line %d schema", path, line)
		label := vector.label(line)
		expectedSelector, expectedClass := expectedVectorRevert(t, vector.Outcome)
		require.Equal(t, expectedSelector, vector.RevertSelector, "%s revertSelector", label)
		require.Equal(t, expectedClass, vector.RevertClass, "%s revertClass", label)

		var direction SwapDirection
		switch vector.Dir {
		case "xToY":
			direction = DirectionXToY
			xToY++
		case "yToX":
			direction = DirectionYToX
			yToX++
		default:
			t.Fatalf("%s: invalid direction %q", label, vector.Dir)
		}

		params := &PoolParams{
			SqrtPriceX96:     u(vector.AnchorPrice),
			FeeAskX24:        parseVectorU32(t, vector.FeeAskX24),
			FeeBidX24:        parseVectorU32(t, vector.FeeBidX24),
			ReserveX:         u(vector.ReserveX),
			ReserveY:         u(vector.ReserveY),
			MaxPunishmentX24: parseVectorU32(t, vector.MaxPunishmentX24),
		}
		amountIn := u(vector.AmountIn)
		multiplier := u(vector.FeeMultiplier)

		var quote *QuoteResult
		if direction == DirectionXToY {
			quote, err = QuoteXToYWithMultiplierChecked(params, amountIn, multiplier)
		} else {
			quote, err = QuoteYToXWithMultiplierChecked(params, amountIn, multiplier)
		}

		effective := clonePoolParams(params)
		simulation, simulationErr := SimulateStandardTokenSwapWithMultiplier(
			effective,
			amountIn,
			multiplier,
			direction,
		)
		if vector.Outcome == vectorMathMulDivRevert {
			require.ErrorIs(t, err, ErrMathOverflow, "%s quote error", label)
			require.Nil(t, quote, "%s quote result", label)
			require.ErrorIs(t, simulationErr, ErrMathOverflow, "%s simulation error", label)
			require.Nil(t, simulation, "%s simulation result", label)
			require.Nil(t, vector.Update, "%s reverted update", label)
			require.Equal(t, "0", vector.DesiredPunishmentX24, "%s desired punishment", label)
			require.Equal(t, "0", vector.EffectiveFeeX24, "%s effective fee", label)
			require.Equal(t, "0", vector.AppliedPunishmentX24, "%s applied punishment", label)
			require.Equal(t, vector.FeeAskX24, vector.FeeAskX24After, "%s ask rollback", label)
			require.Equal(t, vector.FeeBidX24, vector.FeeBidX24After, "%s bid rollback", label)
			require.Equal(t, vector.ReserveX, vector.ReserveXAfter, "%s X rollback", label)
			require.Equal(t, vector.ReserveY, vector.ReserveYAfter, "%s Y rollback", label)
			require.Equal(t, params.SqrtPriceX96, effective.SqrtPriceX96, "%s anchor mutation", label)
			require.Equal(t, params.ReserveX, effective.ReserveX, "%s reserve X mutation", label)
			require.Equal(t, params.ReserveY, effective.ReserveY, "%s reserve Y mutation", label)
			mathRevert++
			total++
			continue
		}

		require.NoError(t, err, "%s quote", label)
		require.Equal(t, vector.AmountOut, quote.AmountOut.Dec(), "%s amountOut", label)
		require.Equal(t, vector.PNext, quote.SqrtPriceNext.Dec(), "%s pNext", label)
		require.Equal(t, vector.FeeAmount, quote.Fee.Dec(), "%s feeAmount", label)
		require.Equal(
			t,
			vector.EffectiveFeeX24,
			strconv.FormatUint(uint64(quote.EffectiveFeeX24), 10),
			"%s effectiveFeeX24",
			label,
		)

		var desired, appliedPunishment uint32
		switch vector.Outcome {
		case vectorApplied:
			require.NoError(t, simulationErr, "%s exact outcome", label)
			require.NotNil(t, simulation, "%s simulation", label)
			require.Equal(t, SimulationStatusApplied, simulation.Status, "%s status", label)
			require.True(t, simulation.Executable, "%s executable", label)
			desired = simulation.DesiredPunishmentX24
			appliedPunishment = simulation.AppliedPunishmentX24
			applied++
		case vectorSwapImpossible:
			require.NoError(t, simulationErr, "%s structured outcome", label)
			require.NotNil(t, simulation, "%s rejected simulation", label)
			require.Equal(t, SimulationStatusSwapImpossible, simulation.Status, "%s status", label)
			require.False(t, simulation.Executable, "%s executable", label)
			desired = simulation.DesiredPunishmentX24
			appliedPunishment = simulation.AppliedPunishmentX24
			swapImpossible++
		case vectorReserveTransitionOverflow:
			require.NoError(t, simulationErr, "%s structured outcome", label)
			require.NotNil(t, simulation, "%s rejected simulation", label)
			require.Equal(t, SimulationStatusReserveTransitionOverflow, simulation.Status, "%s status", label)
			require.False(t, simulation.Executable, "%s executable", label)
			desired = simulation.DesiredPunishmentX24
			appliedPunishment = simulation.AppliedPunishmentX24
			reserveOverflow++
		default:
			t.Fatalf("%s unknown outcome %q", label, vector.Outcome)
		}

		require.Equal(t, quote.EffectiveFeeX24, simulation.EffectiveFeeX24, "%s simulation effective fee", label)
		require.Equal(t, vector.DesiredPunishmentX24, strconv.FormatUint(uint64(desired), 10), "%s desiredPunishmentX24", label)
		require.Equal(t, vector.AppliedPunishmentX24, strconv.FormatUint(uint64(appliedPunishment), 10), "%s appliedPunishmentX24", label)
		require.Equal(t, vector.FeeAskX24After, strconv.FormatUint(uint64(effective.FeeAskX24), 10), "%s feeAskX24After", label)
		require.Equal(t, vector.FeeBidX24After, strconv.FormatUint(uint64(effective.FeeBidX24), 10), "%s feeBidX24After", label)
		require.Equal(t, vector.ReserveXAfter, effective.ReserveX.Dec(), "%s reserveXAfter", label)
		require.Equal(t, vector.ReserveYAfter, effective.ReserveY.Dec(), "%s reserveYAfter", label)

		if vector.Update != nil {
			require.Equal(t, vectorApplied, vector.Outcome, "%s update outcome", label)
			require.NoError(t, ApplyStateUpdate(
				effective,
				u(vector.Update.AnchorPrice),
				parseVectorU32(t, vector.Update.FeeAskX24),
				parseVectorU32(t, vector.Update.FeeBidX24),
			), "%s update", label)
			require.Equal(t, vector.Update.AnchorPriceAfter, effective.SqrtPriceX96.Dec(), "%s update anchor", label)
			require.Equal(t, vector.Update.FeeAskX24After, strconv.FormatUint(uint64(effective.FeeAskX24), 10), "%s update ask", label)
			require.Equal(t, vector.Update.FeeBidX24After, strconv.FormatUint(uint64(effective.FeeBidX24), 10), "%s update bid", label)
			require.Equal(t, vector.Update.ReserveXAfter, effective.ReserveX.Dec(), "%s update reserve X", label)
			require.Equal(t, vector.Update.ReserveYAfter, effective.ReserveY.Dec(), "%s update reserve Y", label)
			require.Equal(t, vector.Update.MaxPunishmentX24After, strconv.FormatUint(uint64(effective.MaxPunishmentX24), 10), "%s update max punishment", label)
			updates++
		}
		total++
	}
	require.NoError(t, scanner.Err())
	require.Positive(t, total, "%s empty corpus", path)
	require.Positive(t, xToY, "%s has no X -> Y rows", path)
	require.Positive(t, yToX, "%s has no Y -> X rows", path)
	require.Positive(t, applied, "%s has no applied rows", path)
	require.Positive(t, swapImpossible+reserveOverflow, "%s has no rollback rows", path)
	if path == "testdata/deterministic_vectors.jsonl" {
		require.Positive(t, mathRevert, "%s has no mulDiv revert rows", path)
		require.Positive(t, updates, "%s has no swap-punishment-update row", path)
	}
	t.Logf(
		"%s: %d bit-exact rows (%d X->Y, %d Y->X, %d applied, %d SwapImpossible, %d reserve overflow, %d mulDiv revert, %d update)",
		path,
		total,
		xToY,
		yToX,
		applied,
		swapImpossible,
		reserveOverflow,
		mathRevert,
		updates,
	)
}

func TestDeterministicVectors(t *testing.T) {
	runVectorFile(t, "testdata/deterministic_vectors.jsonl")
}

func TestFuzzVectors(t *testing.T) {
	runVectorFile(t, "testdata/fuzz_vectors.jsonl")
}
