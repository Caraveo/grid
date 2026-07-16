#!/usr/bin/env node
/**
 * GRID CLI — `grid`
 *
 * Primary miner entry:  grid node
 */
import { nodeCommand, printNodeHelp } from "./commands/node.js";
import { submitCommand } from "./commands/submit.js";
import { statsCommand } from "./commands/stats.js";

const VERSION = "0.1.0";

function printHelp(): void {
  console.log(`grid v${VERSION} — GRID useful mining CLI

Usage:
  grid <command> [options]

Commands:
  node      Run a miner node (join network, claim jobs, earn)
  submit    Submit a job to the coordinator
  stats     Show network / coordinator stats
  help      Show this help
  version   Show version

Examples:
  grid node
  grid node --class M --gpu "RTX 3080"
  grid submit --job echo --payload "hello" --wait
  grid stats

Run \`grid <command> --help\` for command options.
`);
}

async function main(): Promise<void> {
  const argv = process.argv.slice(2);
  const cmd = argv[0];

  if (!cmd || cmd === "help" || cmd === "-h" || cmd === "--help") {
    printHelp();
    return;
  }
  if (cmd === "version" || cmd === "-V" || cmd === "--version") {
    console.log(VERSION);
    return;
  }

  const rest = argv.slice(1);

  switch (cmd) {
    case "node":
      await nodeCommand(rest);
      break;
    case "submit":
      await submitCommand(rest);
      break;
    case "stats":
      await statsCommand(rest);
      break;
    // Convenience: `grid start` == `grid node`
    case "start":
      await nodeCommand(["start", ...rest]);
      break;
    default:
      console.error(`Unknown command: ${cmd}\n`);
      printHelp();
      process.exitCode = 1;
  }
}

main().catch((err) => {
  console.error(err);
  process.exitCode = 1;
});
