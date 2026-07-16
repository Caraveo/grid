import { envOr, flag, opt } from "../lib/args.js";

export async function submitCommand(argv: string[]): Promise<void> {
  if (flag(argv, "--help") || flag(argv, "-h")) {
    console.log(`Usage: grid submit [--job echo|hash_file] [--payload <text>] [--wait]

Submit a job to the coordinator.

Examples:
  grid submit --job echo --payload "hello-grid" --wait
  grid submit --job hash_file --payload "data"
`);
    return;
  }

  const coordinator = opt(argv, "--coordinator") ?? envOr("GRID_COORDINATOR", "http://127.0.0.1:8787");
  const kind = (opt(argv, "--job") ?? "echo") as "echo" | "hash_file";
  const payload =
    opt(argv, "--payload") ?? (kind === "echo" ? "hello-grid" : "grid-payload");

  const res = await fetch(`${coordinator}/v1/jobs`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ kind, payload }),
  });
  const body = await res.json();
  if (!res.ok) {
    console.error(body);
    process.exitCode = 1;
    return;
  }
  console.log(JSON.stringify(body, null, 2));

  if (flag(argv, "--wait")) {
    const id = body.id as string;
    for (let i = 0; i < 30; i++) {
      await new Promise((r) => setTimeout(r, 1000));
      const j = await (await fetch(`${coordinator}/v1/jobs/${id}`)).json();
      console.log(`status=${j.status}`);
      if (j.status === "verified" || j.status === "rejected" || j.status === "failed") {
        console.log(JSON.stringify(j, null, 2));
        break;
      }
    }
  }
}
