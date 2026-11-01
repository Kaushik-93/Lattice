#!/usr/bin/env node
// End-to-end query latency over the data socket against a running stack, on the
// "Benchmark · 50M orders" project (`docker compose exec gateway gateway seed-bench 50000000`).
//
// Each aggregation runs N times uncached (Redis flushed before every run, so DuckDB does the
// work) and N times cached. Times are client-side: batch sent → Arrow frame received and
// decoded, through Docker's port forwarding. Writes perf-results/ws.json.
//
//   node scripts/bench-ws.mjs [-n 7]
import { execFileSync } from "node:child_process";
import { mkdirSync, writeFileSync } from "node:fs";
import { createRequire } from "node:module";
import path from "node:path";

const repo = path.resolve(import.meta.dirname, "..");
const { tableFromIPC } = createRequire(path.join(repo, "apps/web/package.json"))("apache-arrow");

const GATEWAY = process.env.GATEWAY_URL ?? "http://localhost:8080";
const WEB = process.env.WEB_ORIGIN ?? "http://localhost:3000";
const N = Number(process.argv[process.argv.indexOf("-n") + 1] || 7);

const CASES = {
  sum_amount_by_region: { dimensions: [{ column: "region" }], measures: [{ column: "amount", agg: "sum" }] },
  sum_amount_by_month: {
    dimensions: [{ column: "order_date", bucket: "month" }],
    measures: [{ column: "amount", agg: "sum" }, { agg: "count" }],
  },
  top10_categories_filtered: {
    dimensions: [{ column: "category" }],
    measures: [{ column: "amount", agg: "sum" }],
    filters: [{ column: "order_date", op: "gte", value: "2025-01-01" }],
    sort: [{ by: "sum_amount", dir: "desc" }],
    limit: 10,
  },
  sum_avg_by_store_and_region: {
    dimensions: [{ column: "store_id" }, { column: "region" }],
    measures: [{ column: "amount", agg: "sum" }, { column: "amount", agg: "avg" }],
  },
  approx_distinct_customers_by_region: {
    dimensions: [{ column: "region" }],
    measures: [{ column: "customer_id", agg: "approx_count_distinct" }],
  },
  exact_distinct_customers_by_region: {
    dimensions: [{ column: "region" }],
    measures: [{ column: "customer_id", agg: "count_distinct" }],
  },
};

async function json(url, init = {}) {
  const res = await fetch(`${GATEWAY}${url}`, { ...init, headers: { origin: WEB, ...init.headers } });
  if (!res.ok) throw new Error(`${url}: HTTP ${res.status}`);
  return { body: await res.json(), res };
}

const login = await json("/api/auth/login", {
  method: "POST",
  headers: { "content-type": "application/json" },
  body: JSON.stringify({ email: "demo@lattice.dev", password: "lattice-demo" }),
});
const cookie = login.res.headers.get("set-cookie").split(";")[0];
const auth = { headers: { cookie } };
const project = (await json("/api/projects", auth)).body.projects.find((p) => p.name.startsWith("Benchmark"));
if (!project) throw new Error("No Benchmark project: run `docker compose exec gateway gateway seed-bench 50000000`");
const source = (await json(`/api/projects/${project.id}/sources`, auth)).body.sources[0];
const detail = (await json(`/api/projects/${project.id}/sources/${source.id}`, auth)).body;
const orders = detail.tables.find((t) => t.name === "orders");

const ws = new WebSocket(`${GATEWAY.replace(/^http/, "ws")}/ws`, { headers: { cookie, origin: WEB } });
ws.binaryType = "arraybuffer";
await new Promise((resolve, reject) => {
  ws.onopen = resolve;
  ws.onerror = () => reject(new Error("websocket failed"));
});

let nextBatch = 1;
function run(query) {
  const batchId = nextBatch++;
  return new Promise((resolve, reject) => {
    const sent = performance.now();
    let result;
    ws.onmessage = ({ data }) => {
      if (typeof data !== "string") {
        const view = new DataView(data);
        const len = view.getUint32(0, true);
        const header = JSON.parse(new TextDecoder().decode(new Uint8Array(data, 4, len)));
        const table = tableFromIPC(new Uint8Array(data, 4 + len));
        result = { header, rows: table.numRows, clientMs: performance.now() - sent };
        return;
      }
      const msg = JSON.parse(data);
      if (msg.type === "query_error") reject(new Error(msg.message));
      if (msg.type === "batch_done" && msg.batch_id === batchId) resolve(result);
    };
    ws.send(JSON.stringify({ type: "query_batch", batch_id: batchId, project_id: project.id, queries: [{ id: "q", query: { table_id: orders.id, ...query } }] }));
  });
}

const median = (xs) => [...xs].sort((a, b) => a - b)[Math.floor(xs.length / 2)];
const flush = () => execFileSync("docker", ["compose", "exec", "-T", "redis", "redis-cli", "FLUSHDB"], { cwd: repo });

const results = {};
console.log(`${orders.row_count.toLocaleString("en-US")} rows · median of ${N} · ms (client end-to-end / inside gateway)`);
for (const [name, query] of Object.entries(CASES)) {
  await run(query); // warm-up (DuckDB buffer pool)
  const cold = [];
  const coldGateway = [];
  for (let i = 0; i < N; i++) {
    flush();
    const r = await run(query);
    cold.push(r.clientMs);
    coldGateway.push(r.header.timings.total_ms);
  }
  const hot = [];
  for (let i = 0; i < N; i++) hot.push((await run(query)).clientMs);
  results[name] = {
    uncached_client_ms: +median(cold).toFixed(1),
    uncached_gateway_ms: +median(coldGateway).toFixed(1),
    cached_client_ms: +median(hot).toFixed(2),
  };
  const r = results[name];
  console.log(
    `${name.padEnd(38)} uncached ${String(r.uncached_client_ms).padStart(7)} / ${String(r.uncached_gateway_ms).padStart(7)}   cached ${r.cached_client_ms}`,
  );
}
ws.close();
mkdirSync(path.join(repo, "perf-results"), { recursive: true });
writeFileSync(path.join(repo, "perf-results/ws.json"), JSON.stringify({ rows: orders.row_count, results }, null, 2) + "\n");
