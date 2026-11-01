import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { createRequire } from "node:module";

// Use Wrangler's installed runtime, including its exact workerd version. Native
// Worker APIs enforce constraints that Node's fetch and SQL mocks cannot catch.
const require = createRequire(import.meta.url);
const { Miniflare, convertV4MiniflareOptions } = require("miniflare");
const source = readFileSync(new URL("../src/worker.js", import.meta.url), "utf8")
  .replace("export default {", "const productionWorker = {");
const entry = `${source}
import { DurableObject } from "cloudflare:workers";
export class RuntimeQueue extends DurableObject {
  constructor(ctx, env) { super(ctx, env); this.queue = new DiagnosticsQueue(ctx, env); }
  async fetch(request) {
    if (new URL(request.url).pathname === "/run-alarm") {
      await this.queue.alarm();
      const row = this.queue.one("SELECT state,issue_number,published_count FROM aggregates");
      return Response.json({ ...row, alarm: await this.ctx.storage.getAlarm() });
    }
    return this.queue.fetch(request);
  }
  alarm() { return this.queue.alarm(); }
}
export default { fetch(request, env) {
  if (new URL(request.url).pathname === "/run-alarm")
    return env.DIAGNOSTICS.get(env.DIAGNOSTICS.idFromName("global-v1")).fetch(request);
  return productionWorker.fetch(request, env);
} };`;

test("workerd native fetch and SQLite alarm publish once without following redirects", async () => {
  const requests = [];
  let mode = "publish";
  const options = {
    modules: true, script: entry, compatibilityDate: "2026-10-30",
    durableObjects: { DIAGNOSTICS: { className: "RuntimeQueue", useSQLite: true } },
    bindings: { GITHUB_TOKEN: "synthetic-not-a-secret", RATE_LIMIT_SECRET: "synthetic-test-rate-limit-secret-00000000" },
    // ALL outbound calls terminate here. No GitHub/internet access is possible.
    outboundService: async request => {
      requests.push({ url: request.url, method: request.method, body: await request.text() });
      return mode === "publish"
        ? Response.json({ number: 42 }, { status: 201 })
        : new Response(null, { status: 302, headers: { Location: "https://untrusted.invalid/redirect" } });
    },
  };
  const mf = new Miniflare(convertV4MiniflareOptions ? convertV4MiniflareOptions(options) : options);
  try {
    const report = { schema: 2, product: "touchline-editor", report_id: "a".repeat(32),
      occurred_at: new Date().toISOString(), editor_version: "0.0.0", source: "client",
      category: "application", event: "application_error", code: "application_error", build: null,
      failure_stage: "handled", exception_type: "null_reference", build_source: "unknown", storefront: "unknown" };
    async function ingest(value) {
      return mf.dispatchFetch("https://collector.test/v1/reports", { method: "POST",
        headers: { "Content-Type": "application/json", "CF-Connecting-IP": "192.0.2.1" }, body: JSON.stringify(value) });
    }
    assert.equal((await ingest(report)).status, 202);
    assert.equal((await ingest(report)).status, 202);
    const result = await (await mf.dispatchFetch("https://collector.test/run-alarm")).json();
    assert.equal(result.state, "published");
    assert.equal(result.issue_number, 42);
    assert.equal(result.published_count, 1);
    assert.equal(requests.length, 1);
    assert.equal(requests[0].url, "https://api.github.com/repos/DevHHM/touchline-editor/issues");
    assert.equal(requests[0].method, "POST");
    assert(requests[0].body.includes("| Exception kind | null_reference |"));
    assert(requests[0].body.includes("| Failure stage | handled |"));
    assert(result.alarm > Date.now());
    mode = "redirect";
    assert.equal((await ingest({ ...report, report_id: "b".repeat(32), editor_version: "0.0.1" })).status, 202);
    await mf.dispatchFetch("https://collector.test/run-alarm");
    assert.equal(requests.length, 2, "a 302 must not trigger a credential-bearing follow-up request");
    assert(requests.every(request => request.url.startsWith("https://api.github.com/")));
  } finally { await mf.dispose(); }
});
