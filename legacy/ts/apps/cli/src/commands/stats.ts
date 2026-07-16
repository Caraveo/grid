import { envOr, flag, opt } from "../lib/args.js";

export async function statsCommand(argv: string[]): Promise<void> {
  if (flag(argv, "--help") || flag(argv, "-h")) {
    console.log(`Usage: grid stats [--coordinator <url>]

Show coordinator jobs and nodes.
`);
    return;
  }
  const coordinator = opt(argv, "--coordinator") ?? envOr("GRID_COORDINATOR", "http://127.0.0.1:8787");
  const res = await fetch(`${coordinator}/v1/stats`);
  console.log(await res.text());
}
