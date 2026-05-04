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

describe("deterministic vectors (from Solidity)", () => {
  if (deterministicVectors.length === 0) {
    it("(skipped — no deterministic_vectors.jsonl)", () => {});
    return;
  }

  for (const vector of deterministicVectors) {
    it(`${vector.name}: ${vector.dir}`, () => {
      const params = {
        sqrtPriceX48: vector.pX48,
        feeQ48: vector.fee,
        reserveX: vector.resX,
        reserveY: vector.resY,
        concentrationK: vector.k,
        amountIn: vector.dir === "xToY" ? vector.dx : vector.dy,
      };

      const result = vector.dir === "xToY" ? quoteXToY(params) : quoteYToX(params);
      const expectedOut = vector.dir === "xToY" ? vector.dy : vector.dx;

      assert.equal(result.amountOut, expectedOut, `${vector.name}: amountOut mismatch`);
      assert.equal(result.sqrtPriceNext, vector.pNext, `${vector.name}: sqrtPriceNext mismatch`);
      assert.equal(result.fee, vector.feeAmt, `${vector.name}: fee mismatch`);
    });
  }
});

describe("edge cases", () => {
  it("returns zero output for zero reserves", () => {
    const result = quoteXToY({
      sqrtPriceX48: "281474976710656",
      feeQ48: "14073748835532",
      reserveX: "0",
      reserveY: "0",
      concentrationK: 5000,
      amountIn: "1000000000000000000",
    });
    assert.equal(result.amountOut, "0");
  });

  it("accepts hex input strings", () => {
    const result = quoteXToY({
      sqrtPriceX48: "0x" + BigInt("281474976710656").toString(16),
      feeQ48: "0x" + BigInt("14073748835532").toString(16),
      reserveX: "0x" + BigInt("1000000000000000000000").toString(16),
      reserveY: "0x" + BigInt("1000000000000000000000").toString(16),
      concentrationK: 5000,
      amountIn: "0x" + BigInt("1000000000000000000").toString(16),
    });

    assert.equal(result.amountOut, "949975824130540819");
    assert.equal(result.sqrtPriceNext, "281467813676027");
    assert.equal(result.fee, "49998727585814946");
  });

  it("quoteXToY and quoteYToX match the deterministic price=1 vectors", () => {
    const params = {
      sqrtPriceX48: "281474976710656",
      feeQ48: "14073748835532",
      reserveX: "1000000000000000000000",
      reserveY: "1000000000000000000000",
      concentrationK: 5000,
      amountIn: "1000000000000000000",
    };

    const xToY = quoteXToY(params);
    const yToX = quoteYToX(params);

    assert.equal(xToY.amountOut, "949975824130540819");
    assert.equal(xToY.sqrtPriceNext, "281467813676027");
    assert.equal(xToY.fee, "49998727585814946");

    assert.equal(yToX.amountOut, "949975824123168435");
    assert.equal(yToX.sqrtPriceNext, "281482139927576");
    assert.equal(yToX.fee, "49998727585426925");
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
      const params = {
        sqrtPriceX48: vector.pX48,
        feeQ48: vector.fee,
        reserveX: vector.resX,
        reserveY: vector.resY,
        concentrationK: vector.k,
        amountIn: vector.dir === "xToY" ? vector.dx : vector.dy,
      };

      const result = vector.dir === "xToY" ? quoteXToY(params) : quoteYToX(params);
      const expectedOut = vector.dir === "xToY" ? vector.dy : vector.dx;

      if (
        result.amountOut !== expectedOut
        || result.sqrtPriceNext !== vector.pNext
        || result.fee !== vector.feeAmt
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
