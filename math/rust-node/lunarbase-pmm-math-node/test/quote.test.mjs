import { describe, it } from "node:test";
import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
const binding = await import(process.env.PMM_MATH_BINDING ?? "../wrapper.js");
const {
  OrderBookStatus,
  OrderBookSafety,
  SwapSimulationStatus,
  buildOrderBook,
  buildValidatedOrderBook,
  buildPreciseOrderBook,
  validateFeeAccountingCapacity,
  ladderAmountOut,
  geometricSizes,
  priceToSqrtPriceX96,
  price_to_sqrt_price_x96,
  quoteXToY,
  quoteYToX,
  simulateXToY,
  simulateYToX,
  sqrtPriceX96ToPrice,
  sqrt_price_x96_to_price,
} = binding;

describe("active treasury reserve accounting", () => {
  const fees = { partnerFee: 0, partnerOperatorPresent: true, treasuryX: "0", treasuryY: "0",
    partnerX: "0", partnerY: "0", routerPartnerX: "0", routerPartnerY: "0" };
  it("retains treasury and removes only credited partner fees in both directions", () => {
    const params = { sqrtPriceX96: "79228162514264337593543950336", feeAskX24: 1677721, feeBidX24: 1677721,
      reserveX: "1000000", reserveY: "1000000", maxPunishmentX24: 167772,
      maxDecayMultiplierX24: "0", currentBlock: "100", latestUpdateBlock: "100", blockDelay: "3",
      feeMultiplier: "1", amountIn: "100003" };
    for (const [simulate, reserve] of [[simulateXToY,"reserveYAfter"],[simulateYToX,"reserveXAfter"]]) {
      for (const share of [0,1,333333,500000,1000000]) {
        for (const present of [false,true]) {
          const result = simulate(params,{ ...fees,partnerFee:share,partnerOperatorPresent:present });
          const partner = present ? BigInt(result.fee)*BigInt(share)/1000000n : 0n;
          assert.equal(BigInt(result[reserve]),1000000n-BigInt(result.amountOut)-partner);
        }
      }
    }
  });
});

function activeOrderBookParams(overrides = {}) {
  return {
    sqrtPriceX96: "79228162514264337593543950336",
    feeAskX24: 0,
    feeBidX24: 0,
    reserveX: "1000000",
    reserveY: "1000000",
    maxPunishmentX24: 0,
    maxDecayMultiplierX24: "0",
    feeMultiplier: "1",
    snapshotBlock: "100",
    maxExecutionBlock: "101",
    latestUpdateBlock: "99",
    blockDelay: "3",
    paused: false,
    xToYSizes: ["100", "1000"],
    yToXSizes: ["100", "1000"],
    ...overrides,
  };
}

describe("order-book builder", () => {
  it("builds two directional cumulative-size ladders", () => {
    assert.deepEqual(geometricSizes("64", 4), ["8", "16", "32", "64"]);
    const book = buildOrderBook(activeOrderBookParams());

    assert.equal(book.status, OrderBookStatus.Active);
    assert.equal(book.safety, OrderBookSafety.Indicative);
    assert.equal(book.snapshotBlock, "100");
    assert.equal(book.maxExecutionBlock, "101");
    assert.equal(book.requiresAmountOutMinimum, true);
    assert.deepEqual(book.xToY, {
      levels: [
        { size: "100", price: "1000000000000000000" },
        { size: "1000", price: "1000000000000000000" },
      ],
      truncated: false,
    });
    assert.deepEqual(book.yToX, book.xToY);
  });

  it("fails closed for paused and stale snapshots", () => {
    const paused = buildOrderBook(activeOrderBookParams({ paused: true }));
    assert.equal(paused.status, OrderBookStatus.Paused);
    assert.deepEqual(paused.xToY.levels, []);
    assert.deepEqual(paused.yToX.levels, []);

    const stale = buildOrderBook(activeOrderBookParams({ maxExecutionBlock: "103" }));
    assert.equal(stale.status, OrderBookStatus.Stale);
    assert.deepEqual(stale.xToY.levels, []);
  });

  it("validates cached state and cumulative size grids strictly", () => {
    assert.throws(
      () => buildOrderBook(activeOrderBookParams({ xToYSizes: ["100", "100"] })),
      /not strictly increasing/,
    );
    assert.throws(
      () => buildOrderBook(activeOrderBookParams({ blockDelay: "0" })),
      /block_delay must be in/,
    );
    assert.throws(
      () => buildOrderBook(activeOrderBookParams({ feeMultiplier: "0" })),
      /fee_multiplier must be non-zero/,
    );
    assert.throws(
      () => geometricSizes("100", 21),
      /levels must be an integer/,
    );
  });
});

describe("validated order-book policy", () => {
  const lot = { minInput: "10", lotInput: "10", maxInput: "20", totalInput: "30" };
  const config = { xToY: lot, yToX: lot, maxTransitions: 10000 };
  const accounting = { partnerFee: 0, partnerOperatorPresent: false,
    treasuryX: "0", treasuryY: "0", partnerX: "0", partnerY: "0",
    routerPartnerX: "0", routerPartnerY: "0" };

  it("requires coherent credited fees and bounds cumulative bucket growth", () => {
    const snapshot = activeOrderBookParams();
    const validate = (fees) => validateFeeAccountingCapacity({ ...snapshot, feeAccounting: fees }, config);
    assert.throws(() => validateFeeAccountingCapacity(snapshot, config), /feeAccounting is required/);
    assert.doesNotThrow(() => validate(accounting));
    assert.throws(() => validate({ ...accounting, partnerFee: 1 }), /requires an operator/);
    for (const partnerFee of [-1, 1.5, NaN, Infinity, 1000001]) {
      assert.throws(() => validate({ ...accounting, partnerFee }), /partnerFee must be an integer/);
    }
    const maximum = (1n << 112n) - 1n;
    assert.throws(() => validate({ ...accounting, treasuryY: String(maximum) }), /headroom/);
    assert.throws(() => validate({ ...accounting, partnerFee: 500000, partnerOperatorPresent: true,
      routerPartnerX: String(maximum) }), /headroom/);
  });

  it("certifies and independently replays every mixed-direction fill sequence", () => {
    const snapshot = activeOrderBookParams({ maxPunishmentX24: 1000000, feeBidX24: 1234 });
    const original = structuredClone(snapshot);
    validateFeeAccountingCapacity({ ...snapshot, feeAccounting: accounting }, config);
    const result = buildValidatedOrderBook(snapshot, config);
    assert.equal(result.book.safety, OrderBookSafety.ExhaustiveLotPolicy);
    assert.equal(result.book.requiresAmountOutMinimum, true);
    assert.ok(result.checkedStates > 1);
    assert.ok(result.checkedTransitions > 1);
    assert.deepEqual(snapshot, original);

    function replay(state, cursors) {
      for (const [side, simulate] of [["xToY", simulateXToY], ["yToX", simulateYToX]]) {
        for (const amount of [10n, 20n]) {
          if (cursors[side] + amount > 30n) continue;
          const promised = ladderAmountOut(result.book[side].levels, String(amount), String(cursors[side]));
          const actual = simulate({ ...state, currentBlock: state.currentBlock ?? state.snapshotBlock, amountIn: String(amount) });
          assert.equal(actual.status, SwapSimulationStatus.Applied);
          assert.ok(BigInt(promised) > 0n);
          assert.ok(BigInt(promised) <= BigInt(actual.amountOut));
          replay({ ...state, reserveX: actual.reserveXAfter, reserveY: actual.reserveYAfter,
            feeAskX24: actual.feeAskX24After, feeBidX24: actual.feeBidX24After },
          { ...cursors, [side]: cursors[side] + amount });
        }
      }
    }
    replay(snapshot, { xToY: 0n, yToX: 0n });
  });

  it("fails closed on malformed policies, budget exhaustion, pause and stale state", () => {
    const snapshot = activeOrderBookParams();
    assert.throws(() => buildValidatedOrderBook(snapshot, { ...config, maxTransitions: 1 }), /budget exceeded/);
    for (const maxTransitions of [0, -1, 1.5, NaN, Infinity, 100001]) {
      assert.throws(() => buildValidatedOrderBook(snapshot, { ...config, maxTransitions }), /maxTransitions/);
    }
    assert.throws(() => buildValidatedOrderBook(snapshot, { ...config,
      xToY: { ...lot, minInput: "11" } }), /aligned/);
    const paused = buildValidatedOrderBook({ ...snapshot, paused: true }, config);
    assert.equal(paused.book.status, OrderBookStatus.Paused);
    assert.equal(paused.book.safety, OrderBookSafety.Indicative);
    assert.equal(paused.checkedStates, 0);
    assert.deepEqual(paused.book.xToY.levels, []);
    assert.equal(buildValidatedOrderBook({ ...snapshot, maxExecutionBlock: "103" }, config).book.status, OrderBookStatus.Stale);
  });

  it("uses exact tranche sums without a lossy VWAP round-trip", () => {
    const levels = [{ size: "3", price: "500000000000000000" },
      { size: "7", price: "250000000000000000" }];
    assert.equal(ladderAmountOut(levels, "3"), "1");
    assert.equal(ladderAmountOut(levels, "7"), "2");
    assert.equal(ladderAmountOut(levels, "4", "3"), "1");
    assert.equal(ladderAmountOut(levels, "5", "3"), null);
    assert.throws(() => ladderAmountOut([...levels].reverse(), "1"), /not strictly increasing|improving/);
    assert.throws(() => ladderAmountOut([{ size: "1", price: "0" }], "1"), /price is zero/);
    assert.throws(() => ladderAmountOut(Array(21).fill(levels[0]), "1"), /exceeds 20 levels/);
  });
});

describe("precise multilevel order-book policy", () => {
  const snapshot = activeOrderBookParams({ maxPunishmentX24: 8_388_608 });
  const lot = { minInput: "50000", lotInput: "50000", maxInput: "50000", totalInput: "200000" };
  const config = { xToY: lot, maxTransitions: 10000 };
  const precision = { maxLevels: 4, targetUnderquoteBps: 0, maxWork: 5_000_000 };

  it("fits distinct prices and reports the actual worst-state and fresh-snapshot errors", () => {
    const original = structuredClone({ snapshot, config, precision });
    const result = buildPreciseOrderBook(snapshot, config, precision);
    const book = result.validated.book;
    assert.equal(book.status, OrderBookStatus.Active);
    assert.equal(book.safety, OrderBookSafety.ExhaustiveLotPolicy);
    assert.ok(book.xToY.levels.length >= 2);
    assert.ok(book.xToY.levels.length <= precision.maxLevels);
    assert.ok(new Set(book.xToY.levels.map((level) => level.price)).size >= 2);
    assert.equal(result.targetMet, true);
    assert.equal(result.worstUnderquoteBps, 0);
    // Zero discretization error is not zero discount to the initial quote:
    // each subsequent swap raises the Pool's directional fee.
    assert.ok(result.worstFreshSnapshotDiscountBps > 0);
    assert.ok(result.workUsed > 0 && result.workUsed <= precision.maxWork);
    assert.ok(result.constraintCount > 0);
    assert.deepEqual({ snapshot, config, precision }, original);
  });

  it("returns an honest target-unmet result for a coarse level limit", () => {
    const result = buildPreciseOrderBook(snapshot, config, { ...precision, maxLevels: 1 });
    assert.equal(result.validated.book.xToY.levels.length, 1);
    assert.equal(result.targetMet, false);
    assert.ok(result.worstUnderquoteBps > 0);
    assert.equal(result.validated.book.safety, OrderBookSafety.ExhaustiveLotPolicy);
    const oldApi = buildValidatedOrderBook(snapshot, config);
    assert.deepEqual(oldApi.book.xToY.levels, [{ size: "200000", price: "999980000000000000" }]);
    // Exact floor-aware fitting can increase the encoded scalar while keeping
    // the same safe delivered amount; the legacy API retains its old chord.
    for (const cursor of ["0", "50000", "100000", "150000"]) {
      assert.ok(BigInt(ladderAmountOut(result.validated.book.xToY.levels, "50000", cursor)) >=
        BigInt(ladderAmountOut(oldApi.book.xToY.levels, "50000", cursor)));
    }
  });

  it("independently checks every mixed-direction fill and recomputes both error metrics", () => {
    const mixedLot = { ...lot, maxInput: "100000", totalInput: "150000" };
    const mixedConfig = { xToY: mixedLot, yToX: mixedLot, maxTransitions: 10000 };
    const result = buildPreciseOrderBook(snapshot, mixedConfig, { ...precision, maxLevels: 3 });
    const constraints = new Map();
    function replay(state, cursors) {
      for (const [side, simulate, freshQuote] of [
        ["xToY", simulateXToY, quoteXToY], ["yToX", simulateYToX, quoteYToX],
      ]) {
        const levels = result.validated.book[side].levels;
        for (const amount of [50000n, 100000n]) {
          if (cursors[side] + amount > 150000n) continue;
          const promised = BigInt(ladderAmountOut(levels, String(amount), String(cursors[side])));
          const actual = simulate({ ...state, currentBlock: state.currentBlock ?? state.snapshotBlock, amountIn: String(amount) });
          assert.equal(actual.status, SwapSimulationStatus.Applied);
          assert.ok(promised > 0n && promised <= BigInt(actual.amountOut));
          const key = `${side}:${cursors[side]}:${amount}`;
          const previous = constraints.get(key);
          const output = BigInt(actual.amountOut);
          constraints.set(key, {
            promised,
            minimum: previous && previous.minimum < output ? previous.minimum : output,
            fresh: BigInt(freshQuote({ ...snapshot, currentBlock: snapshot.snapshotBlock, amountIn: String(amount) }).amountOut),
          });
          replay({ ...state, reserveX: actual.reserveXAfter, reserveY: actual.reserveYAfter,
            feeAskX24: actual.feeAskX24After, feeBidX24: actual.feeBidX24After },
          { ...cursors, [side]: cursors[side] + amount });
        }
      }
    }
    replay(snapshot, { xToY: 0n, yToX: 0n });
    const gapBps = (promised, reference) => promised >= reference ? 0 :
      Number(((reference - promised) * 10000n + reference - 1n) / reference);
    assert.equal(result.constraintCount, constraints.size);
    assert.equal(result.worstUnderquoteBps,
      Math.max(...[...constraints.values()].map(({ promised, minimum }) => gapBps(promised, minimum))));
    assert.equal(result.worstFreshSnapshotDiscountBps,
      Math.max(...[...constraints.values()].map(({ promised, fresh }) => gapBps(promised, fresh))));
    assert.equal(result.targetMet, result.worstUnderquoteBps === 0);
  });

  it("validates precision fields and fails closed when either work budget is exhausted", () => {
    for (const maxLevels of [0, -1, 1.5, NaN, Infinity, 21]) {
      assert.throws(() => buildPreciseOrderBook(snapshot, config, { ...precision, maxLevels }), /maxLevels/);
    }
    for (const targetUnderquoteBps of [-1, 0.5, NaN, Infinity, 10001]) {
      assert.throws(() => buildPreciseOrderBook(snapshot, config, { ...precision, targetUnderquoteBps }), /targetUnderquoteBps/);
    }
    for (const maxWork of [0, -1, 1.5, NaN, Infinity, 5_000_001]) {
      assert.throws(() => buildPreciseOrderBook(snapshot, config, { ...precision, maxWork }), /maxWork/);
    }
    assert.throws(() => buildPreciseOrderBook(snapshot, config, { ...precision, maxWork: 1 }), /budget/i);
    assert.throws(() => buildPreciseOrderBook(snapshot, { ...config, maxTransitions: 1 }, precision), /budget/i);
  });

  it("never marks a paused or stale snapshot as meeting a precision target", () => {
    for (const unavailable of [{ ...snapshot, paused: true }, { ...snapshot, maxExecutionBlock: "103" }]) {
      const result = buildPreciseOrderBook(unavailable, config, precision);
      assert.equal(result.targetMet, false);
      assert.equal(result.validated.book.safety, OrderBookSafety.Indicative);
      assert.deepEqual(result.validated.book.xToY.levels, []);
      assert.equal(result.constraintCount, 0);
    }
  });
});

const directory = path.dirname(fileURLToPath(import.meta.url));
const vectorsDirectory =
  process.env.PMM_MATH_VECTORS_DIR ??
  path.join(directory, "..", "..", "..", "rust", "lunarbase-pmm-math");
const vectorFiles = [
  path.join(vectorsDirectory, "deterministic_vectors.jsonl"),
  path.join(vectorsDirectory, "fuzz_vectors.jsonl"),
];

const INTEGER_FIELDS = [
  "maxDecayMultiplierX24", "currentBlock", "latestUpdateBlock", "blockDelay",
  "anchorPrice",
  "feeAskX24",
  "feeBidX24",
  "reserveX",
  "reserveY",
  "maxPunishmentX24",
  "feeMultiplier",
  "amountIn",
  "amountOut",
  "pNext",
  "feeAmount",
  "desiredPunishmentX24",
  "effectiveFeeX24",
  "appliedPunishmentX24",
  "feeAskX24After",
  "feeBidX24After",
  "reserveXAfter",
  "reserveYAfter",
];
const UPDATE_INTEGER_FIELDS = [
  "anchorPrice",
  "feeAskX24",
  "feeBidX24",
  "anchorPriceAfter",
  "feeAskX24After",
  "feeBidX24After",
  "reserveXAfter",
  "reserveYAfter",
  "maxPunishmentX24After",
];
const OUTCOME_METADATA = Object.freeze({
  Applied: {
    selector: "0x00000000",
    className: "None",
    status: SwapSimulationStatus.Applied,
  },
  SwapImpossible: {
    selector: "0x4a45e749",
    className: "SwapImpossible()",
    status: SwapSimulationStatus.SwapImpossible,
  },
  ReserveTransitionOverflow: {
    selector: "0x6dfcc650",
    className: "SafeCastOverflowedUintDowncast(uint8,uint256)",
    status: SwapSimulationStatus.ReserveTransitionOverflow,
  },
  StalePrice: { selector: "0x19abf40e", className: "StalePrice()", status: SwapSimulationStatus.StalePrice },
  MathMulDivRevert: {
    selector: "0x4e487b71",
    className: "Panic(0x11)",
    status: null,
  },
});

function assertCanonicalDecimalString(value, label) {
  assert.equal(typeof value, "string", `${label} must be a decimal string`);
  assert.match(value, /^(0|[1-9][0-9]*)$/, `${label} must be canonical decimal`);
}

function validateVector(row, label) {
  assert.equal(row.schemaVersion, 6, `${label}: schemaVersion`);
  assert.ok(row.dir === "xToY" || row.dir === "yToX", `${label}: dir`);
  if (row.seed !== undefined) assertCanonicalDecimalString(row.seed, `${label}: seed`);
  for (const field of INTEGER_FIELDS) {
    assertCanonicalDecimalString(row[field], `${label}: ${field}`);
  }
  if (row.update !== undefined) {
    assert.equal(typeof row.update, "object", `${label}: update object`);
    assert.notEqual(row.update, null, `${label}: update object`);
    for (const field of UPDATE_INTEGER_FIELDS) {
      assertCanonicalDecimalString(row.update[field], `${label}: update.${field}`);
    }
  }

  const metadata = OUTCOME_METADATA[row.outcome];
  assert.ok(metadata, `${label}: unsupported outcome ${row.outcome}`);
  assert.equal(row.revertSelector, metadata.selector, `${label}: revertSelector`);
  assert.equal(row.revertClass, metadata.className, `${label}: revertClass`);
  return row;
}

function readJsonl(filePath) {
  const contents = fs.readFileSync(filePath, "utf8");
  const rows = contents
    .split("\n")
    .filter((line) => line.trim().length > 0)
    .map((line, index) => {
      let row;
      try {
        row = JSON.parse(line);
      } catch (error) {
        throw new Error(`${filePath}:${index + 1}: invalid JSON: ${error.message}`);
      }
      return validateVector(row, `${filePath}:${index + 1}`);
    });
  assert.ok(rows.length > 0, `${filePath} must contain at least one vector`);
  return rows;
}

function paramsFromVector(vector) {
  return {
    sqrtPriceX96: vector.anchorPrice,
    feeAskX24: Number(vector.feeAskX24),
    feeBidX24: Number(vector.feeBidX24),
    reserveX: vector.reserveX,
    reserveY: vector.reserveY,
    maxPunishmentX24: Number(vector.maxPunishmentX24),
    maxDecayMultiplierX24: vector.maxDecayMultiplierX24,
    currentBlock: vector.currentBlock,
    latestUpdateBlock: vector.latestUpdateBlock,
    blockDelay: vector.blockDelay,
    amountIn: vector.amountIn,
    feeMultiplier: vector.feeMultiplier,
  };
}

function assertExactMathMulDivError(operation, label) {
  assert.throws(operation, (error) => {
    assert.equal(error.message, "mulDiv result exceeds uint256", `${label}: error message`);
    return true;
  });
}

function assertUpdateVector(vector, simulation, quoteOperation, label) {
  const update = vector.update;
  if (update === undefined) return;

  assert.equal(vector.outcome, "Applied", `${label}: update outcome`);
  assert.equal(update.anchorPriceAfter, update.anchorPrice, `${label}: updated anchor`);
  assert.equal(update.feeAskX24After, update.feeAskX24, `${label}: updated ask`);
  assert.equal(update.feeBidX24After, update.feeBidX24, `${label}: updated bid`);
  assert.equal(update.reserveXAfter, simulation.reserveXAfter, `${label}: update reserve X`);
  assert.equal(update.reserveYAfter, simulation.reserveYAfter, `${label}: update reserve Y`);
  assert.equal(
    update.maxPunishmentX24After,
    vector.maxPunishmentX24,
    `${label}: update max punishment`,
  );

  const postUpdateQuote = quoteOperation({
    sqrtPriceX96: update.anchorPriceAfter,
    maxDecayMultiplierX24: vector.maxDecayMultiplierX24,
    currentBlock: vector.currentBlock,
    latestUpdateBlock: vector.currentBlock,
    blockDelay: vector.blockDelay,
    feeAskX24: Number(update.feeAskX24After),
    feeBidX24: Number(update.feeBidX24After),
    reserveX: update.reserveXAfter,
    reserveY: update.reserveYAfter,
    maxPunishmentX24: Number(update.maxPunishmentX24After),
    amountIn: "0",
    feeMultiplier: "1",
  });
  assert.equal(postUpdateQuote.sqrtPriceNext, update.anchorPriceAfter, `${label}: update replay`);
}

function assertVector(vector, index, source) {
  const params = paramsFromVector(vector);
  const quoteOperation = vector.dir === "xToY" ? quoteXToY : quoteYToX;
  const simulateOperation = vector.dir === "xToY" ? simulateXToY : simulateYToX;
  const label = `${source}:${index + 1} ${vector.name ?? vector.seed ?? vector.dir}`;

  if (vector.outcome === "MathMulDivRevert") {
    assertExactMathMulDivError(() => quoteOperation(params), `${label}: quote`);
    assertExactMathMulDivError(() => simulateOperation(params), `${label}: simulation`);
    assert.equal(vector.update, undefined, `${label}: reverted update`);
    assert.equal(vector.desiredPunishmentX24, "0", `${label}: desired punishment`);
    assert.equal(vector.appliedPunishmentX24, "0", `${label}: applied punishment`);
    assert.equal(vector.effectiveFeeX24, "0", `${label}: effective fee`);
    assert.equal(vector.feeAskX24After, vector.feeAskX24, `${label}: ask rollback`);
    assert.equal(vector.feeBidX24After, vector.feeBidX24, `${label}: bid rollback`);
    assert.equal(vector.reserveXAfter, vector.reserveX, `${label}: X rollback`);
    assert.equal(vector.reserveYAfter, vector.reserveY, `${label}: Y rollback`);
    return;
  }

  const quote = quoteOperation(params);
  const simulation = simulateOperation(params);
  const metadata = OUTCOME_METADATA[vector.outcome];

  assert.equal(quote.amountOut, vector.amountOut, `${label}: amountOut`);
  assert.equal(quote.sqrtPriceNext, vector.pNext, `${label}: pNext`);
  assert.equal(quote.fee, vector.feeAmount, `${label}: feeAmount`);
  assert.equal(quote.effectiveFeeX24, Number(vector.effectiveFeeX24), `${label}: effectiveFeeX24`);

  assert.equal(simulation.amountOut, vector.amountOut, `${label}: simulated amountOut`);
  assert.equal(simulation.sqrtPriceNext, vector.pNext, `${label}: simulated pNext`);
  assert.equal(simulation.fee, vector.feeAmount, `${label}: simulated feeAmount`);
  assert.equal(
    simulation.effectiveFeeX24,
    Number(vector.effectiveFeeX24),
    `${label}: simulated effectiveFeeX24`,
  );
  assert.equal(simulation.status, metadata.status, `${label}: exact outcome`);
  assert.equal(simulation.executable, vector.outcome === "Applied", `${label}: executable`);
  assert.equal(
    simulation.desiredPunishmentX24,
    Number(vector.desiredPunishmentX24),
    `${label}: desiredPunishmentX24`,
  );
  assert.equal(
    simulation.appliedPunishmentX24,
    Number(vector.appliedPunishmentX24),
    `${label}: appliedPunishmentX24`,
  );
  assert.equal(
    simulation.feeAskX24After,
    Number(vector.feeAskX24After),
    `${label}: feeAskX24After`,
  );
  assert.equal(
    simulation.feeBidX24After,
    Number(vector.feeBidX24After),
    `${label}: feeBidX24After`,
  );
  assert.equal(simulation.reserveXAfter, vector.reserveXAfter, `${label}: reserveXAfter`);
  assert.equal(simulation.reserveYAfter, vector.reserveYAfter, `${label}: reserveYAfter`);
  assertUpdateVector(vector, simulation, quoteOperation, label);
}

for (const vectorFile of vectorFiles) {
  describe(`Solidity parity: ${path.basename(vectorFile)}`, () => {
    const vectors = readJsonl(vectorFile);
    it(`matches all ${vectors.length} vectors bit-for-bit`, () => {
      for (const [index, vector] of vectors.entries()) {
        assertVector(vector, index, vectorFile);
      }
    });
  });
}

const Q96 = "79228162514264337593543950336";
const MAX_U24 = 16_777_215;
const BASE_PARAMS = {
  sqrtPriceX96: Q96,
  feeAskX24: 0,
  feeBidX24: 0,
  reserveX: "1000000",
  reserveY: "1000000",
  maxPunishmentX24: 0,
  maxDecayMultiplierX24: "0", currentBlock: "100", latestUpdateBlock: "100", blockDelay: "10",
  amountIn: "1000",
};

describe("immediate-punishment split regression", () => {
  it("matches the Solidity single versus ten-chunk outputs exactly", () => {
    const total = "1000000000000000000000000";
    const chunk = "100000000000000000000000";
    const base = {
      sqrtPriceX96: Q96,
      feeAskX24: 0,
      feeBidX24: 0,
      reserveX: total,
      reserveY: total,
      maxPunishmentX24: MAX_U24,
      maxDecayMultiplierX24: "0", currentBlock: "100", latestUpdateBlock: "100", blockDelay: "10",
      feeMultiplier: "1",
    };

    const single = simulateXToY({ ...base, amountIn: total });
    assert.equal(single.status, SwapSimulationStatus.Applied);
    assert.equal(single.amountOut, "875000000000000000000000");
    assert.equal(single.feeBidX24After, 2_097_152);

    let splitParams = { ...base, amountIn: chunk };
    let splitTotalOut = 0n;
    let finalSplit;
    for (let i = 0; i < 10; i += 1) {
      finalSplit = simulateXToY(splitParams);
      assert.equal(finalSplit.status, SwapSimulationStatus.Applied);
      splitTotalOut += BigInt(finalSplit.amountOut);
      splitParams = {
        ...splitParams,
        feeAskX24: finalSplit.feeAskX24After,
        feeBidX24: finalSplit.feeBidX24After,
        reserveX: finalSplit.reserveXAfter,
        reserveY: finalSplit.reserveYAfter,
      };
    }

    assert.equal(splitTotalOut, 999_312_406_778_335_571_289_065n);
    assert.equal(finalSplit.feeBidX24After, 20_971);
    assert.ok(splitTotalOut > BigInt(single.amountOut));
  });
});

describe("strict JavaScript boundary", () => {
  it("implements the Q24 full-fee sentinel exactly", () => {
    const result = quoteXToY({ ...BASE_PARAMS, feeBidX24: MAX_U24 });
    const simulation = simulateXToY({ ...BASE_PARAMS, feeBidX24: MAX_U24 });
    assert.equal(result.amountOut, "0");
    assert.equal(result.fee, "1000");
    assert.equal(result.effectiveFeeX24, MAX_U24);
    assert.equal(result.sqrtPriceNext, Q96);
    assert.equal(simulation.executable, false);
    assert.equal(simulation.status, SwapSimulationStatus.SwapImpossible);
  });

  for (const [name, override] of [
    ["empty amount", { amountIn: "" }],
    ["empty hex amount", { amountIn: "0x" }],
    ["leading-zero decimal", { amountIn: "00" }],
    ["overlong decimal", { amountIn: "0".repeat(79) }],
    ["uint256 overflow", { amountIn: (1n << 256n).toString() }],
    ["oversized hex", { amountIn: `0x1${"0".repeat(64)}` }],
    ["uint112 reserve overflow", { reserveX: (1n << 112n).toString() }],
    ["uint160 anchor overflow", { sqrtPriceX96: (1n << 160n).toString() }],
    ["fractional fee", { feeAskX24: 1.5 }],
    ["negative fee", { feeAskX24: -1 }],
    ["oversized fee", { feeAskX24: MAX_U24 + 1 }],
    ["NaN fee", { feeAskX24: Number.NaN }],
    ["infinite fee", { feeAskX24: Number.POSITIVE_INFINITY }],
  ]) {
    it(`rejects ${name}`, () => {
      assert.throws(() => quoteXToY({ ...BASE_PARAMS, ...override }));
    });
  }

  it("rejects numeric and noncanonical literals in every vector integer field", () => {
    const base = readJsonl(vectorFiles[0])[0];
    for (const field of INTEGER_FIELDS) {
      assert.throws(
        () => validateVector({ ...base, [field]: 9_007_199_254_740_993 }, `numeric ${field}`),
        `${field} numeric literal`,
      );
      assert.throws(
        () => validateVector({ ...base, [field]: "01" }, `noncanonical ${field}`),
        `${field} noncanonical string`,
      );
    }
    assert.throws(() => validateVector({ ...base, seed: 1 }, "numeric seed"));
  });

  it("rejects numeric and noncanonical update integer fields", () => {
    const base = readJsonl(vectorFiles[0]).find((vector) => vector.update !== undefined);
    assert.ok(base, "deterministic corpus must include an update vector");
    for (const field of UPDATE_INTEGER_FIELDS) {
      assert.throws(
        () => validateVector({ ...base, update: { ...base.update, [field]: 1 } }, `numeric ${field}`),
        `update.${field} numeric literal`,
      );
      assert.throws(
        () => validateVector({ ...base, update: { ...base.update, [field]: "00" } }, `noncanonical ${field}`),
        `update.${field} noncanonical string`,
      );
    }
  });
});

describe("Q64.96 converter helpers", () => {
  it("keeps camelCase and snake_case helpers aligned", () => {
    assert.equal(priceToSqrtPriceX96(1), Q96);
    assert.equal(price_to_sqrt_price_x96(1), Q96);
    assert.equal(sqrtPriceX96ToPrice(Q96), 1);
    assert.equal(sqrt_price_x96_to_price(Q96), 1);
  });
});


describe("cubic price age", () => {
  const aged = { ...BASE_PARAMS, reserveX: "100000000", reserveY: "100000000",
    feeAskX24: 10000, feeBidX24: 10000, amountIn: "16777216",
    maxDecayMultiplierX24: "50331648", latestUpdateBlock: "100", blockDelay: "10" };
  it("matches contract golden fees in both directions through the inclusive deadline", () => {
    for (const quote of [quoteXToY, quoteYToX]) {
      for (const [age, fee] of [[0,10000], [1,10020], [5,12500], [9,24580], [10,30000]]) {
        const result = quote({ ...aged, currentBlock: String(100 + age) });
        assert.equal(result.fee, String(fee));
        assert.equal(result.effectiveFeeX24, 10000);
      }
    }
    assert.equal(quoteXToY({ ...aged, currentBlock: "111" }).amountOut, "0");
    const stale = simulateXToY({ ...aged, currentBlock: "111" });
    assert.equal(stale.status, SwapSimulationStatus.StalePrice);
    assert.equal(stale.appliedPunishmentX24, 0);
    assert.equal(stale.reserveXAfter, aged.reserveX);
    assert.equal(stale.feeBidX24After, aged.feeBidX24);
  });
  it("persists the unaged punishment and accepts the full uint64 multiplier", () => {
    const result = simulateXToY({ ...aged, reserveX: "500000", reserveY: "500000",
      currentBlock: "105", amountIn: "1000", maxPunishmentX24: 16778 });
    assert.equal(result.feeBidX24After, 10001);
    const saturated = simulateXToY({ ...aged, currentBlock: "101", maxDecayMultiplierX24: "18446744073709551615" });
    assert.equal(saturated.status, SwapSimulationStatus.SwapImpossible);
    assert.throws(() => quoteXToY({ ...aged, maxDecayMultiplierX24: "18446744073709551616" }), /uint64/);
    assert.throws(() => quoteXToY({ ...aged, currentBlock: "99" }), /precedes/);
    assert.throws(() => quoteXToY({ ...aged, blockDelay: "281474976710656" }), /uint48/);
  });
  it("prices and certifies the last execution block without aging stored fees", () => {
    const snapshot = activeOrderBookParams({ reserveX: "100000000", reserveY: "100000000",
      feeAskX24: 10000, feeBidX24: 10000, maxDecayMultiplierX24: "50331648",
      snapshotBlock: "100", latestUpdateBlock: "100", maxExecutionBlock: "110", blockDelay: "10",
      xToYSizes: ["16777216"], yToXSizes: [] });
    const book = buildOrderBook(snapshot);
    assert.equal(book.status, OrderBookStatus.Active);
    assert.equal(book.xToY.levels[0].price, "998211860656738281");
    const policy = { xToY: { minInput: "1000", lotInput: "1000", maxInput: "2000", totalInput: "3000" }, maxTransitions: 1000 };
    for (const result of [buildValidatedOrderBook(snapshot, policy), buildPreciseOrderBook(snapshot, policy,
      { maxLevels: 4, targetUnderquoteBps: 1, maxWork: 100000 }) .validated]) {
      for (const age of [0, 5, 10]) {
        const quote = quoteXToY({ ...snapshot, currentBlock: String(100 + age), amountIn: "1000" });
        assert.ok(BigInt(ladderAmountOut(result.book.xToY.levels, "1000")) <= BigInt(quote.amountOut));
      }
    }
  });
});
