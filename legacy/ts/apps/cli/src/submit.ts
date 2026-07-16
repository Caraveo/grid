const COORD = process.env.GRID_COORDINATOR ?? "http://127.0.0.1:8787";

const kind = (process.argv.includes("--job")
  ? process.argv[process.argv.indexOf("--job") + 1]
  : "echo") as "echo" | "hash_file";

const payloadIdx = process.argv.indexOf("--payload");
const payload =
  payloadIdx >= 0 ? process.argv[payloadIdx + 1] : kind === "echo" ? "hello-grid" : "grid-payload";

const res = await fetch(`${COORD}/v1/jobs`, {
  method: "POST",
  headers: { "content-type": "application/json" },
  body: JSON.stringify({ kind, payload }),
});
const body = await res.json();
console.log(JSON.stringify(body, null, 2));

if (process.argv.includes("--wait")) {
  const id = body.id as string;
  for (let i = 0; i < 30; i++) {
    await new Promise((r) => setTimeout(r, 1000));
    const j = await (await fetch(`${COORD}/v1/jobs/${id}`)).json();
    console.log(`status=${j.status}`);
    if (j.status === "verified" || j.status === "rejected" || j.status === "failed") {
      console.log(JSON.stringify(j, null, 2));
      break;
    }
  }
}
