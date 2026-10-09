// Fails if the Shim bundle uses any syntax newer than ES5 (Chromium M47).
import { readFileSync } from "node:fs";
import { parse } from "acorn";

const file = "dist/tizen-sim-shims.js";
try {
  parse(readFileSync(file, "utf8"), { ecmaVersion: 5 });
  console.log(`${file} is ES5`);
} catch (error) {
  console.error(`${file} is not ES5: ${error.message}`);
  process.exit(1);
}
