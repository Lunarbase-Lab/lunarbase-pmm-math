import { access, readFile } from "node:fs/promises";
import { fileURLToPath } from "node:url";

const declarations = fileURLToPath(new URL("../index.d.ts", import.meta.url));
const loader = fileURLToPath(new URL("../index.cjs", import.meta.url));
const legacyLoader = fileURLToPath(new URL("../index.js", import.meta.url));
const generated = await readFile(declarations, "utf8");
const runtimeEnum = "export enum SwapSimulationStatus";
const occurrences = generated.split(runtimeEnum).length - 1;

if (occurrences !== 1) {
  throw new Error(
    `expected exactly one generated runtime SwapSimulationStatus enum, found ${occurrences}`,
  );
}

if (generated.includes("export const enum SwapSimulationStatus")) {
  throw new Error("ambient const enum is incompatible with isolatedModules");
}

await access(loader);

try {
  await access(legacyLoader);
  throw new Error("unexpected legacy index.js; generate the loader directly as index.cjs");
} catch (error) {
  if (error?.code !== "ENOENT") throw error;
}
