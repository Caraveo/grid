const COORD = process.env.GRID_COORDINATOR ?? "http://127.0.0.1:8787";
const res = await fetch(`${COORD}/v1/stats`);
console.log(await res.text());
