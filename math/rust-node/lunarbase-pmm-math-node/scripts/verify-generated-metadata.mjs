import { access, readFile } from "node:fs/promises";
import { fileURLToPath } from "node:url";

const declarations = fileURLToPath(new URL("../index.d.ts", import.meta.url));
const loader = fileURLToPath(new URL("../index.cjs", import.meta.url));
const legacyLoader = fileURLToPath(new URL("../index.js", import.meta.url));
const generated = await readFile(declarations, "utf8");
for (const enumName of ["SwapSimulationStatus", "OrderBookStatus", "OrderBookSafety"]) {
  const occurrences = generated.split(`export enum ${enumName}`).length - 1;
  if (occurrences !== 1) {
    throw new Error(`expected exactly one generated runtime ${enumName} enum, found ${occurrences}`);
  }
  if (generated.includes(`export const enum ${enumName}`)) {
    throw new Error("ambient const enum is incompatible with isolatedModules");
  }
}

await access(loader);

const commonJs = await readFile(loader, "utf8");
const esm = await readFile(new URL("../wrapper.js", import.meta.url), "utf8");
for (const name of ["buildOrderBook", "buildValidatedOrderBook", "buildPreciseOrderBook",
  "validateFeeAccountingCapacity", "ladderAmountOut"]) {
  if (!generated.includes(`export declare function ${name}(`)) {
    throw new Error(`missing generated declaration for ${name}`);
  }
  if (!commonJs.includes(`module.exports.${name} = ${name}`)) {
    throw new Error(`missing generated CommonJS export for ${name}`);
  }
  if (!esm.includes(`export const ${name} = binding.${name};`)) {
    throw new Error(`missing ESM export for ${name}`);
  }
}

try {
  await access(legacyLoader);
  throw new Error("unexpected legacy index.js; generate the loader directly as index.cjs");
} catch (error) {
  if (error?.code !== "ENOENT") throw error;
}
