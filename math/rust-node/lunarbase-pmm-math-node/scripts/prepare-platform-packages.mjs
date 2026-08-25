import { copyFile, readFile, readdir, writeFile } from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";

const packageRoot = fileURLToPath(new URL("..", import.meta.url));
const platformRoot = path.resolve(
  packageRoot,
  process.env.NAPI_PACKAGE_DIR ?? "npm",
);
const licenseFiles = ["LICENSE-MIT", "LICENSE-APACHE"];
const entries = await readdir(platformRoot, { withFileTypes: true });
const packageDirectories = entries.filter((entry) => entry.isDirectory());

if (packageDirectories.length === 0) {
  throw new Error(`no platform packages found below ${platformRoot}`);
}

for (const entry of packageDirectories) {
  const directory = path.join(platformRoot, entry.name);
  const manifestPath = path.join(directory, "package.json");
  const manifest = JSON.parse(await readFile(manifestPath, "utf8"));
  const files = new Set(manifest.files ?? []);

  for (const license of licenseFiles) {
    files.add(license);
    await copyFile(path.join(packageRoot, license), path.join(directory, license));
  }

  manifest.files = [...files];
  await writeFile(manifestPath, `${JSON.stringify(manifest, null, 2)}\n`);
}
