/**
 * Prefer the `grid` CLI:
 *
 *   grid node
 *   grid node --class M --gpu "RTX 3080"
 *
 * This script delegates to the same entry for `npm run dev` in apps/node.
 */
import { spawnSync } from "node:child_process";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { existsSync } from "node:fs";

const __dirname = dirname(fileURLToPath(import.meta.url));
const cliRoot = join(__dirname, "..", "..", "cli");
const mainTs = join(cliRoot, "src", "main.ts");
const tsxCli = join(cliRoot, "node_modules", "tsx", "dist", "cli.mjs");
const localTsx = join(__dirname, "..", "node_modules", "tsx", "dist", "cli.mjs");
const tsx = existsSync(tsxCli) ? tsxCli : localTsx;

const result = spawnSync(process.execPath, [tsx, mainTs, "node", ...process.argv.slice(2)], {
  stdio: "inherit",
  env: process.env,
});
process.exit(result.status ?? 1);
