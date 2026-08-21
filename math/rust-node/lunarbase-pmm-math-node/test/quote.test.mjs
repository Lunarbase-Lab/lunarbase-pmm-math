import { describe, it } from "node:test";
import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import {
  priceToSqrtPriceX96,
  price_to_sqrt_price_x96,
  quoteXToY,
  quoteYToX,
  sqrtPriceX96ToPrice,
  sqrt_price_x96_to_price,
} from "../wrapper.js";

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
 * Build the `QuoteParams` shape from a JSONL row (Q64.96 design).
 * Each row exercises one direction; the JSONL `fee` field carries the
 * directionally-relevant Q24 fee (bid for xToY, ask for yToX), so the other
 * side is a don't-care set to 0.
 */
function paramsFromVector(vector) {
  const isXToY = vector.dir === "xToY";
  return {
    sqrtPriceX96: String(vector.pX96),
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

// Q64.96 sqrt-price for price = 1.0 (`2^96`). Used by the edge-case suite.
const SQRT_PRICE_X96_ONE = "79228162514264337593543950336";

describe("edge cases", () => {
  it("returns zero output for zero reserves", () => {
    const result = quoteXToY({
      sqrtPriceX96: SQRT_PRICE_X96_ONE, // Q96 = price 1.0
      feeAskX24: 0,
      feeBidX24: 838860, // 5% in Q24
      reserveX: "0",
      reserveY: "0",
      concentrationK: 5000 << 12,
      amountIn: "1000000000000000000",
    });
    assert.equal(result.amountOut, "0");
  });


});

describe("Q64.96 converter helpers", () => {
  it("keeps camelCase and snake_case X96 helpers exported", () => {
    assert.equal(priceToSqrtPriceX96(1.0), SQRT_PRICE_X96_ONE);
    assert.equal(price_to_sqrt_price_x96(1.0), SQRT_PRICE_X96_ONE);
    assert.equal(sqrtPriceX96ToPrice(SQRT_PRICE_X96_ONE), 1.0);
    assert.equal(sqrt_price_x96_to_price(SQRT_PRICE_X96_ONE), 1.0);
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
