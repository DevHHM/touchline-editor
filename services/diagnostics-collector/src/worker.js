const REPOSITORY = "DevHHM/touchline-editor";
const MAX_BODY = 2048;
const DAY = 86_400_000;
const HOUR = 3_600_000;
const REPORT_RETENTION = 7 * DAY;
const ISSUE_CAP = 5;
const UPDATE_CAP = 20;
const INTAKE_CAP = 3000;
const CLIENT_DAILY_CAP = 200;
const CLIENT_HOURLY_CAP = 60;
const CODES = new Set(["access_denied", "unsupported_build", "registry_validation", "session_changed",
  "engine_missing", "engine_transport", "timeout", "other_error", "application_error"]);
const BUILDS = new Set(["6ADFB11C-19E69000", "6A229E8A-1EB41000"]);
const KEYS = ["schema", "product", "report_id", "occurred_at", "editor_version", "source", "category", "event", "code", "build"];
const encoder = new TextEncoder();
const PRIVACY = `<!doctype html><html lang="en"><meta charset="utf-8"><meta name="viewport" content="width=device-width"><title>Touchline diagnostic sharing</title>
<main><h1>Touchline diagnostic sharing</h1><p>Sharing is optional. The editor asks whether to send each new error report, with No selected by default. Approving one report does not authorize future reports. Only a report you approve enters the local sending queue; that approved report may be retried for up to seven days if delivery fails. You can cancel pending reports from the editor, which stops pending uploads and clears the local sharing outbox. Reports already accepted or published cannot be withdrawn by cancelling pending reports.</p>
<h2>What is sent</h2><p>Editor version, component, a predefined error category/code, an optional recognized game-build identifier, UTC occurrence time, and a random per-report identifier. No player names, save contents, local paths, memory addresses, passwords, access tokens, exception text, computer identifiers or local log files are sent.</p>
<h2>Public destination</h2><p>Equal error summaries are grouped into public issues at <a href="https://github.com/DevHHM/touchline-editor/issues">DevHHM/touchline-editor</a>. Issues contain predefined codes, version/build information, aggregate report counts and UTC days. Counts are anonymous reports, not unique affected people. Later reports update the existing managed summary at most hourly, subject to a global daily limit; no per-report comments are posted. Notes outside the marked summary are preserved.</p>
<h2>Retention and rate limiting</h2><p>The server receives the network IP to handle the request, converts it to a keyed daily hash for abuse limits, and never stores or publishes the raw IP in application data. Daily hash rate keys expire after two days. Random report-ID receipts and unattempted queue records expire after seven days. Published fingerprints, aggregate counts/timestamps and unresolved creation attempts are retained until an operator removes them so retries cannot create duplicate public issues. Public GitHub issues remain public until the repository operator removes them.</p>
<p>Cloudflare and GitHub process traffic under their own platform policies. Application request/body logging is disabled in this deployment configuration. No personal diagnostic files are uploaded by this feature. An accepted report is durably queued; rate limits or an unresolved network failure may delay or prevent creation of an issue.</p>
<h2>Contact</h2><p>For questions or a removal request, contact the maintainer through the repository linked above. Do not post tokens, save files or personal logs in a public issue.</p></main></html>`;
const json = (body, status = 200, headers = {}) => new Response(JSON.stringify(body), {
  status, headers: { "Content-Type": "application/json", "Cache-Control": "no-store", ...headers },
});
const hex = buffer => [...new Uint8Array(buffer)].map(n => n.toString(16).padStart(2, "0")).join("");
const dayKey = time => new Date(time).toISOString().slice(0, 10);

export function validateReport(input, now = Date.now()) {
  if (!input || typeof input !== "object" || Array.isArray(input) || Object.keys(input).length !== KEYS.length ||
      KEYS.some(key => !Object.hasOwn(input, key)) || input.schema !== 1 || input.product !== "touchline-editor" ||
      typeof input.report_id !== "string" || !/^[a-f0-9]{32}$/.test(input.report_id) ||
      typeof input.editor_version !== "string" || !/^\d{1,3}\.\d{1,3}\.\d{1,3}(?:\.\d{1,5})?$/.test(input.editor_version) ||
      !["client", "engine"].includes(input.source) || !CODES.has(input.code) ||
      (input.build !== null && !BUILDS.has(input.build))) return null;
  const expected = { connection: ["connection_failed"], lifecycle: ["engine_failed"], application: ["application_error"],
    command: ["connect", "connection_poll", "connection_reconnect"] };
  if (!Object.hasOwn(expected, input.category) || !expected[input.category].includes(input.event) ||
      (input.category === "application" && input.code !== "application_error")) return null;
  if (typeof input.occurred_at !== "string" ||
      !/^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(?:\.\d{1,7})?(?:Z|\+00:00)$/.test(input.occurred_at)) return null;
  const time = Date.parse(input.occurred_at);
  if (!Number.isFinite(time) || time < now - REPORT_RETENTION || time > now + 300_000 ||
      new Date(time).toISOString().slice(0, 19) !== input.occurred_at.slice(0, 19)) return null;
  // Reconstruct exactly the admitted schema. No extra/raw text survives.
  return Object.fromEntries(KEYS.map(key => [key, key === "occurred_at" ? new Date(time).toISOString() : input[key]]));
}

export async function fingerprint(report) {
  return hex(await crypto.subtle.digest("SHA-256", encoder.encode([
    report.editor_version, report.source, report.category, report.event, report.code, report.build ?? "",
  ].join("|"))));
}
export async function clientKey(ip, day, secret) {
  const key = await crypto.subtle.importKey("raw", encoder.encode(secret), { name: "HMAC", hash: "SHA-256" }, false, ["sign"]);
  return hex(await crypto.subtle.sign("HMAC", key, encoder.encode(`${day}|${ip}`)));
}
async function readBounded(request) {
  const declared = request.headers.get("content-length");
  if (declared !== null && (!/^\d+$/.test(declared) || Number(declared) > MAX_BODY)) return null;
  if (!request.body) return null;
  const reader = request.body.getReader(); let length = 0; const chunks = [];
  try {
    while (true) {
      const { done, value } = await reader.read(); if (done) break;
      length += value.byteLength;
      if (length > MAX_BODY) { await reader.cancel(); return null; }
      chunks.push(value);
    }
    const bytes = new Uint8Array(length); let offset = 0;
    for (const chunk of chunks) { bytes.set(chunk, offset); offset += chunk.length; }
    return JSON.parse(new TextDecoder("utf-8", { fatal: true }).decode(bytes));
  } catch { return null; } finally { reader.releaseLock(); }
}

export default {
  async fetch(request, env) {
    const url = new URL(request.url);
    if (request.method === "GET" && url.pathname === "/privacy") return new Response(PRIVACY, { headers: {
      "Content-Type": "text/html; charset=utf-8", "Cache-Control": "public, max-age=300",
      "Content-Security-Policy": "default-src 'none'; base-uri 'none'; frame-ancestors 'none'", "X-Content-Type-Options": "nosniff",
    } });
    if (request.method === "GET" && url.pathname === "/health") return json({ service: "touchline-diagnostics", schema: 1 });
    if (request.method !== "POST" || url.pathname !== "/v1/reports" || url.search) return json({ error: "not_found" }, 404);
    if (request.headers.has("origin")) return json({ error: "browser_requests_disabled" }, 403);
    if (request.headers.get("content-type")?.split(";", 1)[0].trim().toLowerCase() !== "application/json") return json({ error: "json_required" }, 415);
    if (typeof env.RATE_LIMIT_SECRET !== "string" || env.RATE_LIMIT_SECRET.length < 32 || !env.GITHUB_TOKEN) return json({ error: "not_configured" }, 503);
    const report = validateReport(await readBounded(request));
    if (!report) return json({ error: "invalid_report" }, 400);
    // Cloudflare supplies/overwrites this header. Never forward or persist it.
    const ip = request.headers.get("cf-connecting-ip") ?? "unknown";
    if (ip.length > 64 || !/^[0-9a-fA-F:.]+$/.test(ip) && ip !== "unknown") return json({ error: "invalid_client" }, 400);
    try {
      const client_key = await clientKey(ip, dayKey(Date.now()), env.RATE_LIMIT_SECRET);
      const object = env.DIAGNOSTICS.get(env.DIAGNOSTICS.idFromName("global-v1"));
      return await object.fetch(new Request("https://internal/ingest", { method: "POST", headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ report, client_key }) }));
    } catch { return json({ error: "temporarily_unavailable" }, 503); }
  },
};

const identityMarker = fp => `<!-- touchline-diagnostic:${fp} -->`;
const beginMarker = fp => `<!-- touchline-diagnostic-managed:${fp}:begin -->`;
const endMarker = fp => `<!-- touchline-diagnostic-managed:${fp}:end -->`;
function managedSummary(row) {
  // Only server-validated fixed enums/version/build and integer aggregates enter
  // public GitHub content. No report ID, IP/hash, exception, path or session ID.
  return `Automatically aggregated, opt-in diagnostic summary.\n\n` +
      `| Field | Value |\n|---|---|\n| Editor version | ${row.version} |\n| Component | ${row.source} |\n` +
      `| Category | ${row.category} |\n| Event | ${row.event} |\n| Error code | ${row.code} |\n` +
      `| Verified game build | ${row.build ?? "unknown"} |\n| Reports received | ${row.count} |\n` +
      `| First observed day (UTC) | ${row.first_at.slice(0, 10)} |\n| Latest observed day (UTC) | ${row.last_at.slice(0, 10)} |\n\n` +
      `This report contains no save data, player names, local paths, memory addresses, tokens or raw error messages. ` +
      `Reports are anonymous and do not establish the number of distinct affected users. ` +
      `This marked summary may be refreshed hourly; maintainer notes belong outside the markers.`;
}
export function issueContent(row) {
  return {
    title: `[Auto diagnostics] ${row.code} · ${row.version} · ${row.source}`,
    body: `${identityMarker(row.fp)}\n${beginMarker(row.fp)}\n${managedSummary(row)}\n${endMarker(row.fp)}`,
  };
}
export function replaceManagedSummary(body, row) {
  if (typeof body !== "string" || body.length > 60_000) return null;
  const identity = identityMarker(row.fp), start = beginMarker(row.fp), end = endMarker(row.fp);
  // Unknown, truncated, duplicated or manually replaced bodies are not ours to
  // overwrite. Only the content between the unique matching markers is managed.
  if ([identity, start, end].some(marker => body.split(marker).length !== 2)) return null;
  const first = body.indexOf(start), last = body.indexOf(end);
  if (last < first + start.length) return null;
  return body.slice(0, first + start.length) + `\n${managedSummary(row)}\n` + body.slice(last);
}
function verifiedIssue(body, row) {
  return body && !body.pull_request && body.number === row.issue_number &&
    body.repository_url === `https://api.github.com/repos/${REPOSITORY}` &&
    body.url === `https://api.github.com/repos/${REPOSITORY}/issues/${row.issue_number}` &&
    typeof body.body === "string";
}

export class DiagnosticsQueue {
  constructor(ctx, env, dependencies = {}) {
    this.ctx = ctx; this.storage = ctx.storage; this.sql = ctx.storage.sql; this.env = env;
    this.now = dependencies.now ?? Date.now;
    // workerd requires the global receiver for native fetch. An instance method
    // call must not pass DiagnosticsQueue as its `this` value.
    this.fetcher = dependencies.fetcher ?? ((...args) => fetch(...args));
    this.sql.exec("CREATE TABLE IF NOT EXISTS receipts (id TEXT PRIMARY KEY, fp TEXT NOT NULL, expires INTEGER NOT NULL)");
    this.sql.exec("CREATE INDEX IF NOT EXISTS receipts_expiry ON receipts(expires)");
    this.sql.exec("CREATE TABLE IF NOT EXISTS rates (bucket TEXT PRIMARY KEY, count INTEGER NOT NULL, expires INTEGER NOT NULL)");
    this.sql.exec("CREATE TABLE IF NOT EXISTS issue_days (day TEXT PRIMARY KEY, count INTEGER NOT NULL)");
    this.sql.exec("CREATE TABLE IF NOT EXISTS update_days (day TEXT PRIMARY KEY, count INTEGER NOT NULL)");
    this.sql.exec(`CREATE TABLE IF NOT EXISTS aggregates (fp TEXT PRIMARY KEY, version TEXT NOT NULL, source TEXT NOT NULL,
      category TEXT NOT NULL, event TEXT NOT NULL, code TEXT NOT NULL, build TEXT, count INTEGER NOT NULL,
      first_at TEXT NOT NULL, last_at TEXT NOT NULL, created INTEGER NOT NULL, state TEXT NOT NULL DEFAULT 'queued',
      issue_number INTEGER, next_at INTEGER NOT NULL DEFAULT 0, reconciliations INTEGER NOT NULL DEFAULT 0,
      published_count INTEGER NOT NULL DEFAULT 0, last_published_at INTEGER NOT NULL DEFAULT 0,
      next_update_at INTEGER NOT NULL DEFAULT 0, update_lease_until INTEGER NOT NULL DEFAULT 0,
      update_blocked INTEGER NOT NULL DEFAULT 0)`);
    // Idempotently upgrade a pre-aggregation SQLite namespace; never discard
    // receipts or old published/uncertain creation history during deployment.
    const columns = new Set([...this.sql.exec("PRAGMA table_info(aggregates)")].map(column => column.name));
    for (const column of ["published_count", "last_published_at", "next_update_at", "update_lease_until", "update_blocked"])
      if (!columns.has(column)) this.sql.exec(`ALTER TABLE aggregates ADD COLUMN ${column} INTEGER NOT NULL DEFAULT 0`);
    this.sql.exec("CREATE INDEX IF NOT EXISTS queue_due ON aggregates(state,next_at)");
  }
  one(query, ...args) { return [...this.sql.exec(query, ...args)][0]; }
  async fetch(request) {
    if (request.method !== "POST" || new URL(request.url).pathname !== "/ingest") return json({ error: "not_found" }, 404);
    try {
      const envelope = await request.json(); const now = this.now();
      const report = validateReport(envelope.report, now);
      if (!report || typeof envelope.client_key !== "string" || !/^[a-f0-9]{64}$/.test(envelope.client_key)) return json({ error: "invalid_report" }, 400);
      const fp = await fingerprint(report);
      const result = this.storage.transactionSync(() => this.persist(report, fp, envelope.client_key, now));
      if (result !== 202) return json({ error: result === 409 ? "report_id_conflict" : "rate_limited" }, result, result === 429 ? { "Retry-After": "3600" } : {});
      await this.schedule();
      // Explicit durable barrier: never acknowledge a report still only in RAM.
      await this.storage.sync();
      return json({ accepted: true }, 202);
    } catch { return json({ error: "temporarily_unavailable" }, 503); }
  }
  prune(now) {
    this.sql.exec("DELETE FROM receipts WHERE expires < ?", now);
    this.sql.exec("DELETE FROM rates WHERE expires < ?", now);
    this.sql.exec("DELETE FROM issue_days WHERE day < ?", dayKey(now - 2 * DAY));
    this.sql.exec("DELETE FROM update_days WHERE day < ?", dayKey(now - 2 * DAY));
    // Published fingerprints and unresolved creation attempts are retained to
    // prevent duplicate issue creation. Unattempted stale input is bounded.
    this.sql.exec("DELETE FROM aggregates WHERE state='queued' AND created < ?", now - REPORT_RETENTION);
  }
  persist(report, fp, client, now) {
    this.prune(now);
    const receipt = this.one("SELECT fp FROM receipts WHERE id=?", report.report_id);
    if (receipt && receipt.fp !== fp) return 409;
    const buckets = [[`hour:${Math.floor(now / HOUR)}:${client}`, CLIENT_HOURLY_CAP],
      [`day:${dayKey(now)}:${client}`, CLIENT_DAILY_CAP], [`global:${dayKey(now)}`, INTAKE_CAP]];
    if (buckets.some(([key, limit]) => (this.one("SELECT count FROM rates WHERE bucket=?", key)?.count ?? 0) >= limit)) return 429;
    for (const [key] of buckets) this.sql.exec("INSERT INTO rates VALUES (?,1,?) ON CONFLICT(bucket) DO UPDATE SET count=count+1", key, now + 2 * DAY);
    if (receipt) return 202;
    this.sql.exec("INSERT INTO receipts VALUES (?,?,?)", report.report_id, fp, now + REPORT_RETENTION);
    this.sql.exec(`INSERT INTO aggregates (fp,version,source,category,event,code,build,count,first_at,last_at,created)
      VALUES (?,?,?,?,?,?,?,1,?,?,?) ON CONFLICT(fp) DO UPDATE SET count=count+1,
      first_at=MIN(first_at,excluded.first_at),last_at=MAX(last_at,excluded.last_at)`, fp, report.editor_version,
    report.source, report.category, report.event, report.code, report.build, report.occurred_at, report.occurred_at, now);
    return 202;
  }
  async schedule() {
    const now = this.now();
    const queued = this.one("SELECT MIN(next_at) AS next FROM aggregates WHERE state='queued'")?.next;
    const unresolved = this.one("SELECT MIN(next_at) AS next FROM aggregates WHERE state IN ('creating','uncertain','reconciling')")?.next;
    const cap = (this.one("SELECT count FROM issue_days WHERE day=?", dayKey(now))?.count ?? 0) >= ISSUE_CAP;
    const times = [];
    if (queued !== null && queued !== undefined) times.push(cap ? (Math.floor(now / DAY) + 1) * DAY + 60_000 : Math.max(now + 60_000, queued));
    if (unresolved !== null && unresolved !== undefined) times.push(Math.max(now + 60_000, unresolved));
    const pendingUpdate = this.one(`SELECT MIN(MAX(next_update_at,update_lease_until)) AS next FROM aggregates
      WHERE state='published' AND update_blocked=0 AND count>published_count`)?.next;
    const updateCap = (this.one("SELECT count FROM update_days WHERE day=?", dayKey(now))?.count ?? 0) >= UPDATE_CAP;
    if (pendingUpdate !== null && pendingUpdate !== undefined)
      times.push(updateCap ? (Math.floor(now / DAY) + 1) * DAY + 60_000 : Math.max(now + 60_000, pendingUpdate));
    // Retention must not depend on the arrival of another report. This also
    // keeps a published-only queue alive just long enough to remove receipts.
    const expiry = this.one(`SELECT MIN(expires) AS next FROM (
      SELECT MIN(expires) AS expires FROM receipts UNION ALL SELECT MIN(expires) FROM rates
      UNION ALL SELECT MIN(created)+? FROM aggregates WHERE state='queued')`, REPORT_RETENTION)?.next;
    if (expiry !== null && expiry !== undefined) times.push(Math.max(now + 60_000, expiry + 1));
    if (times.length) {
      const due = Math.min(...times), existing = await this.storage.getAlarm();
      // Frequent incoming reports must not continually postpone an earlier
      // queue alarm and starve publication.
      if (existing === null || existing > due) await this.storage.setAlarm(due);
    }
  }
  claim() {
    const now = this.now();
    return this.storage.transactionSync(() => {
      this.prune(now);
      this.sql.exec("UPDATE aggregates SET state='uncertain' WHERE state IN ('creating','reconciling') AND next_at<=?", now);
      const uncertain = this.one("SELECT * FROM aggregates WHERE state='uncertain' AND next_at<=? ORDER BY created LIMIT 1", now);
      if (uncertain) {
        this.sql.exec("UPDATE aggregates SET state='reconciling',next_at=? WHERE fp=?", now + 10 * 60_000, uncertain.fp);
        return { mode: "reconcile", row: uncertain };
      }
      if ((this.one("SELECT count FROM issue_days WHERE day=?", dayKey(now))?.count ?? 0) < ISSUE_CAP) {
        const row = this.one("SELECT * FROM aggregates WHERE state='queued' AND next_at<=? ORDER BY created LIMIT 1", now);
        if (row) {
          this.sql.exec("INSERT INTO issue_days VALUES (?,1) ON CONFLICT(day) DO UPDATE SET count=count+1", dayKey(now));
          // Reserve the attempt before any HTTP request. Crashes/ambiguous responses
          // reconcile, never replay POST. Even rejected attempts consume the cap.
          this.sql.exec("UPDATE aggregates SET state='creating',next_at=? WHERE fp=?", now + 10 * 60_000, row.fp);
          return { mode: "create", row };
        }
      }
      if ((this.one("SELECT count FROM update_days WHERE day=?", dayKey(now))?.count ?? 0) >= UPDATE_CAP) return null;
      const update = this.one(`SELECT * FROM aggregates WHERE state='published' AND update_blocked=0 AND count>published_count
        AND next_update_at<=? AND update_lease_until<=? ORDER BY next_update_at,created LIMIT 1`, now, now);
      if (!update) return null;
      this.sql.exec("INSERT INTO update_days VALUES (?,1) ON CONFLICT(day) DO UPDATE SET count=count+1", dayKey(now));
      this.sql.exec("UPDATE aggregates SET update_lease_until=?,next_update_at=? WHERE fp=?", now + 10 * 60_000, now + HOUR, update.fp);
      return { mode: "update", row: update };
    });
  }
  async github(path, options = {}) {
    // workerd supports manual/follow only. Manual preserves the fixed origin;
    // callers reject 3xx responses instead of following them with credentials.
    return this.fetcher(`https://api.github.com${path}`, { ...options, redirect: "manual", signal: AbortSignal.timeout(8000),
      headers: { Accept: "application/vnd.github+json", Authorization: `Bearer ${this.env.GITHUB_TOKEN}`,
        "X-GitHub-Api-Version": "2022-11-28", "User-Agent": "Touchline-Diagnostics-Collector", "Content-Type": "application/json" } });
  }
  async updatePublished(row) {
    let done = false, blocked = false;
    try {
      const path = `/repos/${REPOSITORY}/issues/${row.issue_number}`;
      const response = await this.github(path);
      if (response.status === 404 || response.status === 410) blocked = true;
      else if (response.ok) {
        const original = await response.json();
        if (!verifiedIssue(original, row)) blocked = true;
        else {
          const body = replaceManagedSummary(original.body, row);
          if (body === null) blocked = true;
          else if (body === original.body) done = true;
          else {
            // GitHub does not document conditional PATCH support. A second read
            // catches intervening edits; never pretend an If-Match header gives
            // atomic compare-and-swap here. Notes outside markers remain intact.
            const check = await this.github(path);
            if (check.ok) {
              const latest = await check.json();
              if (verifiedIssue(latest, row) && latest.body === original.body) {
                const patched = await this.github(path, { method: "PATCH", body: JSON.stringify({ body }) });
                if (patched.status === 200) {
                  const updated = await patched.json();
                  done = verifiedIssue(updated, row) && updated.body === body;
                }
              }
            }
          }
        }
      }
    } catch { /* No response bodies, user notes, headers or secrets are logged. */ }
    this.sql.exec(`UPDATE aggregates SET published_count=MAX(published_count,?),last_published_at=CASE WHEN ? THEN ? ELSE last_published_at END,
      update_lease_until=0,next_update_at=?,update_blocked=? WHERE fp=?`, done ? row.count : 0, done ? 1 : 0,
      this.now(), this.now() + HOUR, blocked ? 1 : 0, row.fp);
    await this.storage.sync();
  }
  async alarm() {
    try { await this.processAlarm(); }
    catch {
      // Once accepted, a report must retain a durable wakeup even if a later
      // storage barrier or finalization fails. If storage is entirely down,
      // let Cloudflare's bounded alarm retries retry this static failure.
      try { await this.storage.setAlarm(this.now() + HOUR); await this.storage.sync(); return; }
      catch { throw new Error("diagnostics_queue_retry"); }
    }
  }
  async processAlarm() {
    if (!this.env.GITHUB_TOKEN) {
      this.storage.transactionSync(() => this.prune(this.now()));
      await this.storage.sync();
      const pending = this.one(`SELECT COUNT(*) AS count FROM aggregates WHERE state IN ('queued','creating','uncertain','reconciling')
        OR (state='published' AND update_blocked=0 AND count>published_count)`).count;
      if (pending) await this.storage.setAlarm(this.now() + HOUR);
      else await this.schedule();
      return;
    }
    const job = this.claim();
    if (!job) { await this.schedule(); return; }
    // Persist the lease's recovery alarm before any external I/O. A crash after
    // GitHub accepts a request must not strand this job until new intake.
    await this.schedule();
    await this.storage.sync();
    const { row, mode } = job;
    if (mode === "update") { await this.updatePublished(row); await this.schedule(); return; }
    let issue = null;
    try {
      if (mode === "create") {
        const response = await this.github(`/repos/${REPOSITORY}/issues`, { method: "POST", body: JSON.stringify(issueContent(row)) });
        if (response.status === 201) {
          const body = await response.json();
          if (Number.isSafeInteger(body.number) && body.number > 0) issue = body.number;
        }
      } else {
        const marker = `touchline-diagnostic:${row.fp}`;
        const response = await this.github(`/search/issues?q=${encodeURIComponent(`repo:${REPOSITORY} is:issue in:body "${marker}"`)}&per_page=10`);
        if (response.ok) {
          const data = await response.json();
          const match = Array.isArray(data.items) && data.items.find(item => item.repository_url === `https://api.github.com/repos/${REPOSITORY}` &&
            typeof item.body === "string" && item.body.includes(`<!-- ${marker} -->`) && Number.isSafeInteger(item.number) && item.number > 0);
          if (match) issue = match.number;
        }
      }
    } catch { /* Never log headers, response text, IPs or tokens. */ }
    if (issue !== null) this.sql.exec(`UPDATE aggregates SET state='published',issue_number=?,published_count=?,last_published_at=?,
      next_update_at=?,update_lease_until=0 WHERE fp=?`, issue, mode === "create" ? row.count : 0, this.now(), this.now() + HOUR, row.fp);
    else this.sql.exec("UPDATE aggregates SET state=?,next_at=?,reconciliations=reconciliations+? WHERE fp=?",
      mode === "reconcile" && row.reconciliations >= 5 ? "needs_review" : "uncertain",
      this.now() + (mode === "create" ? 10 * 60_000 : HOUR), mode === "reconcile" ? 1 : 0, row.fp);
    await this.storage.sync();
    await this.schedule();
  }
}
