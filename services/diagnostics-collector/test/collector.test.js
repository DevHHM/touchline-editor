import test from "node:test";
import assert from "node:assert/strict";
import { DatabaseSync } from "node:sqlite";
import worker, { DiagnosticsQueue, validateReport, fingerprint, clientKey, issueContent, replaceManagedSummary } from "../src/worker.js";

const NOW = Date.parse("2026-10-30T12:00:00.000Z");
const report = (id = 1, changes = {}) => ({ schema: 1, product: "touchline-editor", report_id: id.toString(16).padStart(32, "0"),
  occurred_at: new Date(NOW).toISOString(), editor_version: "0.13.3", source: "engine", category: "connection",
  event: "connection_failed", code: "registry_validation", build: "6ADFB11C-19E69000", ...changes });
const detailedReport = (id = 1, changes = {}) => report(id, { schema: 2, editor_version: "0.14.3", source: "client",
  category: "application", event: "application_error", code: "application_error", failure_stage: "handled",
  exception_type: "null_reference", build_source: "verified_session", storefront: "steam", ...changes });

function harness(fetcher = async () => new Response(JSON.stringify({ number: 10 }), { status: 201 })) {
  const db = new DatabaseSync(":memory:"); let now = NOW; let pendingAlarm = null; const alarms = [];
  const storage = {
    sql: { exec(query, ...args) { return db.prepare(query).all(...args); } },
    transactionSync(fn) { db.exec("BEGIN"); try { const result = fn(); db.exec("COMMIT"); return result; } catch (e) { db.exec("ROLLBACK"); throw e; } },
    async sync() {}, async setAlarm(at) { alarms.push(at); pendingAlarm = at; }, async getAlarm() { return pendingAlarm; },
  };
  const object = new DiagnosticsQueue({ storage }, { GITHUB_TOKEN: "server-secret-never-public" }, { now: () => now, fetcher });
  const alarm = object.alarm.bind(object);
  object.alarm = async () => { pendingAlarm = null; return alarm(); };
  return { db, object, storage, alarms, advance(ms) { now += ms; },
    rows(table = "aggregates") { return db.prepare(`SELECT * FROM ${table}`).all(); },
    ingest(value, key = "a".repeat(64)) { return object.fetch(new Request("https://internal/ingest", { method: "POST", body: JSON.stringify({ report: value, client_key: key }) })); } };
}
function githubMemory() {
  const issues = new Map(), requests = [];
  return { issues, requests, async fetcher(url, init) {
    const method = init.method ?? "GET"; requests.push({ method, url, body: init.body });
    if (method === "POST") {
      const number = issues.size + 1;
      const issue = { ...JSON.parse(init.body), number, url: `https://api.github.com/repos/DevHHM/touchline-editor/issues/${number}`,
        repository_url: "https://api.github.com/repos/DevHHM/touchline-editor" };
      issues.set(number, issue); return new Response(JSON.stringify(issue), { status: 201 });
    }
    const number = Number(url.split("/").at(-1)), issue = issues.get(number);
    if (!issue) return new Response("{}", { status: 404 });
    if (method === "PATCH") Object.assign(issue, JSON.parse(init.body));
    return new Response(JSON.stringify(issue));
  } };
}

test("strict ten-field schema accepts client UTC format and rejects raw payloads and malformed timestamps", () => {
  assert.equal(validateReport(report(), NOW).code, "registry_validation");
  assert.equal(validateReport(report(1, { occurred_at: "2026-10-30T12:00:00.1234567+00:00" }), NOW).occurred_at, "2026-10-30T12:00:00.123Z");
  for (const changes of [
    { message: "private-save-data" }, { fingerprint: "accidental-computed-property" }, { report_id: "@private" },
    { code: "arbitrary exception" }, { build: "0xPRIVATE" }, { source: "other" }, { event: "user-name" },
    { category: "__proto__" }, { occurred_at: "2026-02-30T12:00:00Z" }, { occurred_at: "2099-10-30T12:00:00Z" },
    { occurred_at: "2026-10-20T12:00:00Z" }, { editor_version: "1.2.3\n@mention" }, { schema: "1" },
  ]) assert.equal(validateReport(report(1, changes), NOW), null, JSON.stringify(changes));
  assert.equal(validateReport(null, NOW), null);
  const missing = report(); delete missing.build; assert.equal(validateReport(missing, NOW), null);
});

test("fingerprint excludes random report ID/time, HMAC keys rotate daily without storing IP", async () => {
  assert.equal(await fingerprint(report(1)), await fingerprint(report(2, { occurred_at: "2026-10-30T13:00:00Z" })));
  assert.notEqual(await fingerprint(report(1)), await fingerprint(report(2, { source: "client" })));
  const a = await clientKey("192.0.2.10", "2026-10-30", "x".repeat(32));
  const b = await clientKey("192.0.2.10", "2026-10-31", "x".repeat(32));
  assert.match(a, /^[0-9a-f]{64}$/); assert.notEqual(a, b); assert(!a.includes("192.0.2.10"));
});

test("v2 admits only fixed diagnostic enums and preserves strict legacy compatibility", async () => {
  assert.equal(validateReport(detailedReport(), NOW).exception_type, "null_reference");
  assert.equal(Object.keys(validateReport(report(), NOW)).length, 10);
  for (const key of ["failure_stage", "exception_type", "build_source", "storefront"]) {
    assert.equal(validateReport(detailedReport(1, { [key]: "PRIVATE_MESSAGE C:\\user\\file 0x12345678" }), NOW), null);
    const missing = detailedReport(); delete missing[key]; assert.equal(validateReport(missing, NOW), null);
  }
  assert.equal(validateReport(detailedReport(1, { message: "private" }), NOW), null);
  assert.equal(validateReport(detailedReport(1, { build: null }), NOW), null);
  assert.equal(validateReport(detailedReport(1, { build: null, build_source: "unknown" }), NOW).build, null);
  assert.equal(validateReport(report(1, { failure_stage: "handled" }), NOW), null);
  assert.equal(validateReport(detailedReport(1, { build: "12345678-01234000", build_source: "observed_pe" }), NOW).build, "12345678-01234000");
  assert.equal(validateReport(detailedReport(1, { build: "12345678-01234000", build_source: "verified_session" }), NOW), null);
  assert.equal(validateReport(detailedReport(1, { build: "12345678-00000000", build_source: "observed_pe" }), NOW), null);
  assert.equal(validateReport(detailedReport(1, { build: "0x7FF123456789 PRIVATE_PATH", build_source: "observed_pe" }), NOW), null);
  assert.equal(validateReport(report(1, { build: "12345678-01234000" }), NOW), null);
  assert.equal(await fingerprint(detailedReport()), "8e23f315ed96c57888ca5253c356bebd403a80067b5b146932596ac3124ec982");
  assert.notEqual(await fingerprint(detailedReport()), await fingerprint(detailedReport(2, { exception_type: "io" })));
  assert.notEqual(await fingerprint(detailedReport()), await fingerprint(detailedReport(2, { failure_stage: "unhandled_domain" })));
  assert.notEqual(await fingerprint(detailedReport()), await fingerprint(report(2, { editor_version: "0.14.3", source: "client", category: "application", event: "application_error", code: "application_error" })));
});

test("v2 persists and publishes diagnostic codes without mixing generic legacy aggregates", async () => {
  const gh = githubMemory(), h = harness(gh.fetcher);
  await h.ingest(report(1, { editor_version: "0.14.3", source: "client", category: "application", event: "application_error", code: "application_error" }));
  await h.ingest(detailedReport(2)); await h.ingest(detailedReport(3));
  assert.equal(h.rows().length, 2); assert.equal(h.rows().find(row => row.exception_type === "null_reference").count, 2);
  await h.object.alarm(); await h.object.alarm();
  const content = [...gh.issues.values()].find(issue => issue.body.includes("| Exception kind | null_reference |"));
  assert(content.body.includes("| Failure stage | handled |") && content.body.includes("| Build provenance | verified_session |") && content.body.includes("| Storefront | steam |"));
  assert(!content.body.includes(detailedReport(2).report_id));
  assert.equal(gh.requests.filter(request => request.method === "POST").length, 2);
  h.db.close();
});

test("202 follows durable sync; retry after uncertain acceptance is report-ID idempotent", async () => {
  const h = harness(); let syncs = 0;
  h.storage.sync = async () => { syncs++; if (syncs === 1) throw new Error("disk unavailable"); };
  assert.equal((await h.ingest(report())).status, 503);
  assert.equal(h.rows()[0].count, 1);
  assert.equal((await h.ingest(report())).status, 202);
  assert.equal(h.rows()[0].count, 1); assert(syncs >= 2); assert(h.alarms.length);
  assert.equal((await h.ingest(report(1, { code: "timeout" }))).status, 409);
  assert.equal(h.rows().length, 1);
  h.storage.transactionSync = () => { throw new Error("storage denied"); };
  assert.equal((await h.ingest(report(2))).status, 503);
  h.db.close();
});

test("concurrent repeated IDs aggregate exactly once and equal errors share one issue", async () => {
  let calls = 0; const h = harness(async (_url, init) => { if (init.method === "POST") calls++; return new Response('{"number":23}', { status: 201 }); });
  const results = await Promise.all(Array.from({ length: 10 }, () => h.ingest(report(1))));
  assert(results.every(r => r.status === 202)); assert.equal(h.rows()[0].count, 1);
  assert.equal((await h.ingest(report(2))).status, 202); assert.equal(h.rows()[0].count, 2);
  await h.object.alarm(); assert.equal(calls, 1); assert.equal(h.rows()[0].issue_number, 23);
  await h.ingest(report(3)); await h.object.alarm(); assert.equal(calls, 1); assert.equal(h.rows()[0].count, 3);
  h.db.close();
});

test("hourly, daily and global intake caps persist and do not accept unrecorded reports", async () => {
  const h = harness();
  for (let n = 1; n <= 60; n++) assert.equal((await h.ingest(report(n))).status, 202);
  assert.equal((await h.ingest(report(61))).status, 429); assert.equal(h.rows("receipts").length, 60);
  h.advance(3_600_000);
  assert.equal((await h.ingest(report(61))).status, 202);
  h.db.prepare("UPDATE rates SET count=200 WHERE bucket LIKE 'day:%'").run();
  assert.equal((await h.ingest(report(62))).status, 429);
  h.db.prepare("UPDATE rates SET count=3000 WHERE bucket LIKE 'global:%'").run();
  assert.equal((await h.ingest(report(63), "b".repeat(64))).status, 429);
  h.db.close();
});

test("public issue content contains only controlled metadata and never anonymous client keys", async () => {
  const sent = []; const h = harness(async (url, init) => { sent.push({ url, init }); return new Response('{"number":2}', { status: 201 }); });
  const r = report(0x12345); await h.ingest(r); await h.object.alarm();
  assert.equal(sent[0].url, "https://api.github.com/repos/DevHHM/touchline-editor/issues");
  const body = sent[0].init.body;
  for (const privateValue of [r.report_id, "a".repeat(64), "server-secret-never-public", "192.0.2.10"]) assert(!body.includes(privateValue));
  assert(body.includes("registry_validation")); assert(body.includes("touchline-diagnostic:"));
  assert.equal(sent[0].init.redirect, "manual");
  h.db.close();
});

test("global issue cap counts reserved attempts and rolls to the next UTC day", async () => {
  let creates = 0; const h = harness(async () => new Response(JSON.stringify({ number: ++creates }), { status: 201 }));
  for (let n = 1; n <= 6; n++) await h.ingest(report(n, { editor_version: `0.13.${n}` }));
  for (let n = 0; n < 8; n++) await h.object.alarm();
  assert.equal(creates, 5); assert.equal(h.rows().filter(row => row.state === "queued").length, 1);
  assert(h.alarms.at(-1) >= Date.parse("2026-10-31T00:00:00Z"));
  h.advance(86_400_000); await h.object.alarm(); assert.equal(creates, 6);
  h.db.close();
});

test("overlapping alarm handlers cannot POST the same fingerprint twice", async () => {
  let release; let creates = 0;
  const h = harness(async () => { creates++; return await new Promise(resolve => { release = () => resolve(new Response('{"number":8}', { status: 201 })); }); });
  await h.ingest(report()); const first = h.object.alarm();
  while (!release) await new Promise(resolve => setImmediate(resolve));
  await h.object.alarm(); assert.equal(creates, 1); release(); await first;
  assert.equal(h.rows()[0].state, "published"); h.db.close();
});

test("ambiguous POST and crash reconcile the marker, never automatically replay creation", async () => {
  const calls = []; let fp;
  const h = harness(async (url, init) => {
    calls.push(init.method ?? "GET");
    if (init.method === "POST") throw new Error("connection lost after possible GitHub acceptance");
    return new Response(JSON.stringify({ items: [{ number: 99, repository_url: "https://api.github.com/repos/DevHHM/touchline-editor", body: `<!-- touchline-diagnostic:${fp} -->` }] }));
  });
  await h.ingest(report()); fp = h.rows()[0].fp; await h.object.alarm();
  assert.equal(h.rows()[0].state, "uncertain"); h.advance(600_000); await h.object.alarm();
  assert.deepEqual(calls, ["POST", "GET"]); assert.equal(h.rows()[0].issue_number, 99);
  h.db.close();
  const crash = harness(async () => new Response('{"items":[]}'));
  await crash.ingest(report()); assert.equal(crash.object.claim().mode, "create");
  for (let n = 0; n < 6; n++) { crash.advance(3_600_000); await crash.object.alarm(); }
  assert.equal(crash.rows()[0].state, "needs_review");
  assert.equal(crash.rows("issue_days")[0].count, 1); crash.db.close();
});

test("edge validates body before forwarding and never forwards IP or headers", async () => {
  const forwarded = []; const env = { RATE_LIMIT_SECRET: "x".repeat(32), GITHUB_TOKEN: "server-secret", DIAGNOSTICS: {
    idFromName: name => name, get: () => ({ async fetch(request) { forwarded.push(await request.json()); return new Response('{}', { status: 202 }); } }),
  } };
  const request = body => new Request("https://collector.example/v1/reports", { method: "POST", headers: {
    "Content-Type": "application/json", "CF-Connecting-IP": "192.0.2.123", "X-Private-Header": "PRIVATE_HEADER",
  }, body: JSON.stringify(body) });
  const current = report(1, { occurred_at: new Date().toISOString() });
  assert.equal((await worker.fetch(request(current), env)).status, 202);
  const text = JSON.stringify(forwarded); assert(!text.includes("192.0.2.123")); assert(!text.includes("PRIVATE_HEADER")); assert(!text.includes("server-secret"));
  assert.equal((await worker.fetch(request({ ...current, message: "raw-secret" }), env)).status, 400);
  assert.equal(forwarded.length, 1);
  assert.equal((await worker.fetch(request(current), { ...env, RATE_LIMIT_SECRET: "" })).status, 503);
  const oversized = request({ ...current, message: "a".repeat(3000) }); assert.equal((await worker.fetch(oversized, env)).status, 400);
  const privacy = await worker.fetch(new Request("https://collector.example/privacy"), env);
  assert.equal(privacy.status, 200); assert.match(privacy.headers.get("content-type"), /text\/html/);
  const disclosure = await privacy.text(); assert(disclosure.includes("seven days") && disclosure.includes("two days"));
  assert(disclosure.includes("DevHHM/touchline-editor")); assert(!disclosure.includes("server-secret"));
});

test("new reports preserve the earlier queue alarm instead of postponing it", async () => {
  const h = harness(); await h.ingest(report()); const first = h.alarms[0];
  for (let n = 2; n <= 10; n++) { h.advance(1000); await h.ingest(report(n)); }
  assert.equal(h.alarms.length, 1); assert.equal(h.alarms[0], first); h.db.close();
});

test("retention alarms remove private receipts and rate keys without new intake", async () => {
  const h = harness(); await h.ingest(report()); await h.object.alarm();
  assert.equal(h.rows()[0].state, "published");
  h.advance(2 * 86_400_000 + 1); await h.object.alarm();
  assert.equal(h.rows("rates").length, 0); assert.equal(h.rows("receipts").length, 1);
  assert(h.alarms.at(-1) >= NOW + 7 * 86_400_000);
  h.advance(5 * 86_400_000 + 1); await h.object.alarm();
  assert.equal(h.rows("receipts").length, 0); assert.equal(h.rows()[0].state, "published"); h.db.close();

  let creates = 0; const stale = harness(async () => { creates++; return new Response('{"number":1}', { status: 201 }); });
  await stale.ingest(report()); stale.advance(7 * 86_400_000 + 1); await stale.object.alarm();
  assert.equal(creates, 0); assert.equal(stale.rows().length, 0); stale.db.close();

  const disabled = harness(); await disabled.ingest(report());
  disabled.object.env.GITHUB_TOKEN = null; disabled.advance(7 * 86_400_000 + 1); await disabled.object.alarm();
  assert.equal(disabled.rows("receipts").length, 0); assert.equal(disabled.rows("rates").length, 0);
  assert.equal(disabled.rows().length, 0); disabled.db.close();
});

test("managed summaries preserve external notes and reject missing or ambiguous markers", async () => {
  const h = harness(); await h.ingest(report()); const row = h.rows()[0];
  const original = issueContent(row).body, next = { ...row, count: 3 };
  const wrapped = `Maintainer notes before\n${original}\nMaintainer notes after\n`;
  const replaced = replaceManagedSummary(wrapped, next);
  assert(replaced.startsWith("Maintainer notes before\n")); assert(replaced.endsWith("\nMaintainer notes after\n"));
  assert(replaced.includes("| Reports received | 3 |"));
  for (const bad of ["user rewrote whole issue", original.replace(":begin -->", ":changed -->"), original + original,
    original.replace(`<!-- touchline-diagnostic:${row.fp} -->`, ""), "x".repeat(60_001)])
    assert.equal(replaceManagedSummary(bad, next), null);
  h.db.close();
});

test("later reports refresh only the existing issue summary hourly and preserve notes", async () => {
  const gh = githubMemory(), h = harness(gh.fetcher);
  await h.ingest(report()); await h.object.alarm();
  gh.issues.get(1).body = `Investigation notes\n${gh.issues.get(1).body}\nPlease keep this note.`;
  await h.ingest(report(2)); await h.object.alarm();
  assert.equal(gh.requests.filter(r => r.method === "PATCH").length, 0);
  h.advance(3_600_000 + 1); await h.object.alarm();
  assert.equal(gh.requests.filter(r => r.method === "POST").length, 1);
  assert.equal(gh.requests.filter(r => r.method === "PATCH").length, 1);
  const issue = gh.issues.get(1);
  assert(issue.body.startsWith("Investigation notes\n")); assert(issue.body.endsWith("\nPlease keep this note."));
  assert(issue.body.includes("| Reports received | 2 |")); assert.equal(h.rows()[0].published_count, 2);
  await h.ingest(report(3)); await h.object.alarm();
  assert.equal(gh.requests.filter(r => r.method === "PATCH").length, 1);
  h.db.close();
});

test("unknown bodies and wrong issue identity are never patched; concurrent edits defer updates", async () => {
  for (const mode of ["missing_markers", "wrong_repository", "changed_between_reads"]) {
    const gh = githubMemory(); let reads = 0;
    const h = harness(async (url, init) => {
      if (!init.method) {
        reads++;
        if (mode === "changed_between_reads" && reads === 2) gh.issues.get(1).body += "\nNew maintainer note";
      }
      return gh.fetcher(url, init);
    });
    await h.ingest(report()); await h.object.alarm();
    if (mode === "missing_markers") gh.issues.get(1).body = "Maintainer replaced the whole issue";
    if (mode === "wrong_repository") gh.issues.get(1).repository_url = "https://api.github.com/repos/other/repo";
    await h.ingest(report(2)); h.advance(3_600_000 + 1); await h.object.alarm();
    assert.equal(gh.requests.filter(r => r.method === "PATCH").length, 0, mode);
    assert.equal(h.rows()[0].published_count, 1);
    assert.equal(h.rows()[0].update_blocked, mode === "changed_between_reads" ? 0 : 1);
    h.db.close();
  }
});

test("lost PATCH acknowledgement is reconciled by a read, without duplicate mutation or creation", async () => {
  const gh = githubMemory(); let losePatch = true;
  const h = harness(async (url, init) => {
    const response = await gh.fetcher(url, init);
    if (init.method === "PATCH" && losePatch) { losePatch = false; throw new Error("response lost"); }
    return response;
  });
  await h.ingest(report()); await h.object.alarm(); await h.ingest(report(2));
  h.advance(3_600_000 + 1); await h.object.alarm(); assert.equal(h.rows()[0].published_count, 1);
  h.advance(3_600_000 + 1); await h.object.alarm(); assert.equal(h.rows()[0].published_count, 2);
  assert.equal(gh.requests.filter(r => r.method === "PATCH").length, 1);
  assert.equal(gh.requests.filter(r => r.method === "POST").length, 1); h.db.close();
});

test("global update cap reserves at most 20 cycles per UTC day and resets independently of creates", async () => {
  const gh = githubMemory(), h = harness(gh.fetcher);
  for (let n = 1; n <= 21; n++) await h.ingest(report(n, { editor_version: `0.14.${n}` }));
  // Existing published records from prior days, retaining production SQLite
  // scheduling/counter behavior without bypassing the separate five-create cap.
  for (const [index, row] of h.rows().entries()) {
    const number = index + 1;
    gh.issues.set(number, { ...issueContent(row), number,
      url: `https://api.github.com/repos/DevHHM/touchline-editor/issues/${number}`,
      repository_url: "https://api.github.com/repos/DevHHM/touchline-editor" });
    h.db.prepare("UPDATE aggregates SET state='published',issue_number=?,count=2,published_count=1 WHERE fp=?").run(number, row.fp);
  }
  for (let n = 0; n < 25; n++) await h.object.alarm();
  assert.equal(gh.requests.filter(r => r.method === "PATCH").length, 20);
  assert.equal(h.rows("update_days")[0].count, 20); assert.equal(h.rows("issue_days").length, 0);
  h.advance(86_400_000); await h.object.alarm();
  assert.equal(gh.requests.filter(r => r.method === "PATCH").length, 21); h.db.close();
});

test("claimed updates have recovery alarms and failed finalization schedules a bounded retry", async () => {
  const gh = githubMemory(), h = harness(gh.fetcher);
  await h.ingest(report()); await h.object.alarm(); await h.ingest(report(2)); h.advance(3_600_000 + 1);
  let patches = 0;
  h.object.fetcher = async (url, init) => {
    if (init.method === "PATCH") { patches++; assert(await h.storage.getAlarm() !== null); }
    return gh.fetcher(url, init);
  };
  let syncs = 0;
  h.storage.sync = async () => { if (++syncs === 2) throw new Error("transient durable barrier failure"); };
  await h.object.alarm(); assert.equal(patches, 1);
  assert(await h.storage.getAlarm() >= NOW + 2 * 3_600_000); h.db.close();
});

test("existing SQLite databases gain update fields without losing deduplication history", async () => {
  const h = harness(); await h.ingest(report());
  for (const column of ["published_count", "last_published_at", "next_update_at", "update_lease_until", "update_blocked", "failure_stage", "exception_type", "build_source", "storefront"])
    h.db.exec(`ALTER TABLE aggregates DROP COLUMN ${column}`);
  new DiagnosticsQueue({ storage: h.storage }, { GITHUB_TOKEN: "server-secret" }, { now: () => NOW });
  assert.equal(h.rows().length, 1); assert.equal(h.rows()[0].published_count, 0);
  assert.equal(h.rows()[0].exception_type, null);
  assert.equal((await h.ingest(report())).status, 202); assert.equal(h.rows()[0].count, 1); h.db.close();
});
