import { describe, it } from "node:test";
import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { quoteXToY, quoteYToX } from "../wrapper.js";

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const vectorsDir = path.join(__dirname, "..", "..", "..", "rust", "lunarbase-pmm-math");
const deterministicVectorsPath = path.join(vectorsDir, "deterministic_vectors.jsonl");
const fuzzVectorsPath = path.join(vectorsDir, "fuzz_vectors.jsonl");

function readJsonl(filePath) {
  try {
    return fs
      .readFileSync(filePath, "utf-8")
      .split("\n")
      .filter((line) => line.trim().length > 0)
      .map((line) => JSON.parse(line));
  } catch {
    return [];
  }
}

const deterministicVectors = readJsonl(deterministicVectorsPath);
const fuzzVectors = readJsonl(fuzzVectorsPath);

/**
 * Build the `QuoteParams` shape from a JSONL row (single-price Q32.48 design).
 * Each row exercises one direction; the JSONL `fee` field carries the
 * directionally-relevant Q24 fee (bid for xToY, ask for yToX), so the other
 * side is a don't-care set to 0.
 */
function paramsFromVector(vector) {
  const isXToY = vector.dir === "xToY";
  return {
    sqrtPriceX48: String(vector.pX48),
    feeAskX24: isXToY ? 0 : Number(vector.fee),
    feeBidX24: isXToY ? Number(vector.fee) : 0,
    reserveX: String(vector.resX),
    reserveY: String(vector.resY),
    concentrationK: Number(vector.k),
    amountIn: String(isXToY ? vector.dx : vector.dy),
  };
}

describe("deterministic vectors (from Solidity)", () => {
  if (deterministicVectors.length === 0) {
    it("(skipped — no deterministic_vectors.jsonl)", () => {});
    return;
  }

  for (const vector of deterministicVectors) {
    it(`${vector.name}: ${vector.dir}`, () => {
      const params = paramsFromVector(vector);
      const result = vector.dir === "xToY" ? quoteXToY(params) : quoteYToX(params);
      const expectedOut = vector.dir === "xToY" ? String(vector.dy) : String(vector.dx);

      assert.equal(result.amountOut, expectedOut, `${vector.name}: amountOut mismatch`);
      assert.equal(result.sqrtPriceNext, String(vector.pNext), `${vector.name}: sqrtPriceNext mismatch`);
      assert.equal(result.fee, String(vector.feeAmt), `${vector.name}: fee mismatch`);
    });
  }
});

// Q32.48 sqrt-price for price = 1.0 (`2^48`). Used by the edge-case suite.
const SQRT_PRICE_X48_ONE = "281474976710656";

describe("edge cases", () => {
  it("returns zero output for zero reserves", () => {
    const result = quoteXToY({
      sqrtPriceX48: SQRT_PRICE_X48_ONE, // Q48 = price 1.0
      feeAskX24: 0,
      feeBidX24: 838860, // 5% in Q24
      reserveX: "0",
      reserveY: "0",
      concentrationK: 5000,
      amountIn: "1000000000000000000",
    });
    assert.equal(result.amountOut, "0");
  });

  it("accepts hex input strings", () => {
    const result = quoteXToY({
      sqrtPriceX48: "0x" + BigInt(SQRT_PRICE_X48_ONE).toString(16),
      feeAskX24: 0,
      feeBidX24: 838860,
      reserveX: "0x" + BigInt("1000000000000000000000").toString(16),
      reserveY: "0x" + BigInt("1000000000000000000000").toString(16),
      concentrationK: 5000,
      amountIn: "0x" + BigInt("1000000000000000000").toString(16),
    });

    // V1 deterministic baseline: pX48=Q48, 5% bid, eq reserves, k=5000, dx=1e18.
    assert.equal(result.amountOut, "949987816809994001");
    assert.equal(result.sqrtPriceNext, "281474976660325");
    assert.equal(result.fee, "49999308586734514");
  });

  it("quoteXToY and quoteYToX match the deterministic price=1 vectors", () => {
    const baseParams = {
      sqrtPriceX48: SQRT_PRICE_X48_ONE,
      reserveX: "1000000000000000000000",
      reserveY: "1000000000000000000000",
      concentrationK: 5000,
      amountIn: "1000000000000000000",
    };

    const xToY = quoteXToY({ ...baseParams, feeAskX24: 0, feeBidX24: 838860 });
    const yToX = quoteYToX({ ...baseParams, feeAskX24: 838860, feeBidX24: 0 });

    // V1 (xToY) and V12 (yToX) from the deterministic generator.
    assert.equal(xToY.amountOut, "949987816809994001");
    assert.equal(xToY.sqrtPriceNext, "281474976660325");
    assert.equal(xToY.fee, "49999308586734514");

    assert.equal(yToX.amountOut, "949987816640155504");
    assert.equal(yToX.sqrtPriceNext, "281474976760987");
    assert.equal(yToX.fee, "49999308577795655");
  });
});

describe("fuzz vectors (from Solidity)", () => {
  if (fuzzVectors.length === 0) {
    it("(skipped — no fuzz_vectors.jsonl)", () => {});
    return;
  }

  it(`validates all ${fuzzVectors.length} fuzz vectors`, () => {
    const failures = [];

    for (let i = 0; i < fuzzVectors.length; i += 1) {
      const vector = fuzzVectors[i];
      const params = paramsFromVector(vector);

      const result = vector.dir === "xToY" ? quoteXToY(params) : quoteYToX(params);
      const expectedOut = vector.dir === "xToY" ? String(vector.dy) : String(vector.dx);

      if (
        result.amountOut !== expectedOut
        || result.sqrtPriceNext !== String(vector.pNext)
        || result.fee !== String(vector.feeAmt)
      ) {
        failures.push(
          `Line ${i + 1} (${vector.dir}): out=${result.amountOut} expected=${expectedOut}, `
            + `pNext=${result.sqrtPriceNext} expected=${vector.pNext}, `
            + `fee=${result.fee} expected=${vector.feeAmt}`,
        );
      }
    }

    if (failures.length > 0) {
      const sample = failures.slice(0, 10).join("\n");
      assert.fail(
        `${failures.length}/${fuzzVectors.length} vectors failed.\n${sample}`
          + (failures.length > 10 ? `\n... and ${failures.length - 10} more` : ""),
      );
    }
  });
});
