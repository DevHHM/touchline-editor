# Touchline diagnostic collector

This optional service turns individually approved **error codes and version metadata** into aggregated public issues in `DevHHM/touchline-editor`. The editor asks about each new error report with **No selected by default**. Approval applies only to that report, never to future errors. Only approved reports enter its outbox and may be retried for up to seven days. Cancelling pending reports stops uploads and clears that outbox; it does not remove reports already accepted by the server or published on GitHub.

The deployed service is [touchline-diagnostics-collector](https://touchline-diagnostics-collector.touchline-diagnostics-collector.workers.dev/health), with [public privacy information](https://touchline-diagnostics-collector.touchline-diagnostics-collector.workers.dev/privacy). Publishing this source does not configure or deploy a separate user's collector; server secrets are not included.

## Data contract

`POST /v1/reports`, `Content-Type: application/json`, maximum 2 KiB:

```json
{
  "schema": 1,
  "product": "touchline-editor",
  "report_id": "0123456789abcdef0123456789abcdef",
  "occurred_at": "2026-10-30T12:00:00.000Z",
  "editor_version": "0.14.0",
  "source": "engine",
  "category": "connection",
  "event": "connection_failed",
  "code": "registry_validation",
  "build": "6ADFB11C-19E69000"
}
```

Schema 1 accepts exactly these ten properties. UTC `+00:00` and up to seven fractional digits are accepted and normalized. Unknown fields, free text, unknown codes/builds, timestamps older than seven days or more than five minutes in the future are rejected. `build` can be `null`. See `validateReport` for the fixed enum and category/event rules.

The deployed collector also accepts schema 2, with exactly four extra properties: `failure_stage`, `exception_type`, `build_source`, and `storefront`. All use fixed enums; exception messages, stacks and paths are excluded. `build_source: "verified_session"` is a recognized connected profile; `"error_text"` is a recognized code in a diagnostic; `"observed_pe"` is an executable PE timestamp/image-size fingerprint (`8HEX-8HEX`) observed during connection, and **does not mean the build is supported**. An unknown or ambiguous identity uses `build: null` and `build_source: "unknown"`. Storefront is `unknown` unless explicitly available; it is never inferred from personal paths.

The dual-schema collector was deployed on 2026-11-01 UTC and verified through read-only deployment/binding checks and GET requests to `/health` and `/privacy`. Health advertises `accepted_schemas: [1, 2]`; the privacy page describes schema-2 metadata and the complete payload preview. The existing Durable Object namespace, secret bindings, compatibility date and logging configuration were preserved. After verification, `distribution/diagnostics-collector.json` was updated to `"report_schema": 2` for the 0.14.5 package. Older configurations without that property still default to schema 1.

Every new error requires explicit approval of its complete outgoing payload; approving one report never approves later reports. Changing schema invalidates old approved outbox entries and requires a fresh per-report preview and approval. The payload queued for transmission is the exact one reviewed by the user, including its report ID. There is no pre-consent network discovery and no automatic schema downgrade/repost. The rollout did not submit a diagnostic report or create a test GitHub issue.

The response is **202 only after the transactional receipt/aggregate commit, alarm scheduling and durable storage synchronization**. A lost response can be retried with the same report ID without incrementing the aggregate. Reusing an ID for another fingerprint returns 409. Accepted reports may be aggregated or retained for operator review; 202 does not promise that a public issue has already been created.

No exception text, game data, player names, computer paths, tokens, session IDs or raw IPs are included in public issues. Report IDs remain private for seven days. The source IP supplied by Cloudflare is HMACed with a server-only secret and UTC day **before** it reaches durable storage; rate keys expire after two days. Raw request headers/bodies are never application-logged. Platform observability is disabled in the example configuration; the account operator should also review any account-level request logging policy.

## Durability, abuse controls and duplicates

- One SQLite-backed Durable Object serializes transactional counters and queue claims.
- Intake is limited to 60/hour and 200/day per daily client HMAC, plus 3,000/day globally. Anonymous reports can be forged; counts are not counts of unique users. NAT users share the rate budget.
- Schema-1 reports with equal version/source/category/event/code/build share one fingerprint and issue. Schema 2 also includes failure stage, exception kind, build provenance and storefront in that fingerprint. Further reports update the existing issue's marked summary at most once per hour per fingerprint. At most **20 update cycles per UTC day** are reserved globally before reading/patching GitHub; failed cycles also consume the budget. No per-report comments or additional issue-creation requests are used for repeats.
- An update first verifies the fixed repository, issue number and API URL, and requires one matching identity marker plus one managed begin/end marker pair. It preserves all content outside that block and updates only `body`, leaving title, state, labels and assignees alone. Missing/duplicated markers or a replaced/unknown body set `update_blocked=1` for operator review. The operator can deliberately stop summary updates by removing the managed markers. Existing records from the earlier collector migrate without losing receipts; old issues without managed markers are not rewritten automatically.
- GitHub does not document conditional `PATCH` support for issues. The collector rereads the body immediately before an update and defers if it changed, but cannot provide an atomic compare-and-swap guarantee for an edit racing the final request. Keep maintainer commentary in issue comments when actively editing during automated updates, or remove the managed markers to pause updates. Lost PATCH acknowledgements are safe to retry: the next cycle rereads current content, and skips mutation when the desired summary is already present.
- At most **five GitHub create attempts per UTC day** are reserved before any network call. The repository destination is fixed in source. The cap also applies to failed/ambiguous attempts.
- Durable Object alarms run at least once. A create attempt and its recovery wakeup are persisted before POST. If a response is lost, the Worker crashes, or GitHub rejects/fails the request, **the POST is never blindly replayed**. The queue searches GitHub for the deterministic marker after ten minutes and then hourly, up to six times. A match completes the job; otherwise it becomes `needs_review` for the operator. This favors avoiding duplicate public issues over guaranteed delivery after ambiguous failures.
- Published fingerprints and unresolved attempts remain as deduplication records. Unattempted queue entries expire after seven days, bounding anonymous backlog. Receipt IDs expire after seven days, matching the client outbox lifetime.

For a `needs_review` record, inspect the repository for the `touchline-diagnostic:<fingerprint>` marker first. If an issue exists, record its number and mark the aggregate `published`. Only after confirming that no issue was created should an operator explicitly requeue the record. For an `update_blocked=1` record, inspect the issue identity and maintainer intent before restoring managed markers and clearing that flag. Do not clear all durable storage to resolve authentication problems: doing so removes idempotency history.

## Deployment preparation

1. Use a Cloudflare account with Workers and SQLite-backed Durable Objects. Run `npm ci` to install the pinned Wrangler version. `wrangler.jsonc` contains the namespace binding and initial migration; no D1 database is needed.
2. Create a fine-grained GitHub token restricted to `DevHHM/touchline-editor` with issue write access. Keep it on the server:
   - `npx wrangler secret put GITHUB_TOKEN`
   - `npx wrangler secret put RATE_LIMIT_SECRET` (at least 32 random characters, distinct from the GitHub token).
3. Review the public disclosure text and deployment configuration, then deploy separately with Wrangler. No secret belongs in the editor ZIP or this repository.
4. Configure the editor distributor's `diagnostics-collector.json` with the real HTTPS `/v1/reports` endpoint, the same deployed origin's `/privacy` disclosure URL, and `destination: "public"`. A configured endpoint never authorizes an upload by itself; each new report still requires the user's approval.
5. Run `npm test` with all outbound requests mocked, then verify the deployed version, existing bindings and GET `/health` and `/privacy` before enabling schema 2 in a distributor configuration. These checks do not submit reports or create public test issues. Consider Cloudflare WAF/rate rules as an additional perimeter for a public anonymous intake.

The collector intentionally has no unauthenticated administration route, raw report download endpoint, token query parameter, arbitrary GitHub destination or redirect following.

## Verification

After `npm ci`, run `npm test` with Node 22.13 or newer. The SQL tests use the built-in experimental `node:sqlite` and cover strict schema/privacy, durable acceptance, ID/fingerprint deduplication, overlapping alarms, retention without new intake, create/update limits, ambiguous network/crash recovery, managed-summary note preservation and migration. The runtime regression uses Miniflare/workerd from the pinned Wrangler dependency with real SQLite Durable Object storage and the runtime's native fetch; every outbound request is mocked. No test creates a real GitHub issue or contacts the production collector. `npx wrangler deploy --dry-run` validates the deployment bundle without publishing it. The separate 2026-11-01 rollout verified the active version, existing namespace and secret binding names, dual-schema health response and updated privacy disclosure. No production diagnostic submission was used for that verification.

The implementation follows the primary API documentation for [SQLite Durable Object storage and transactions](https://developers.cloudflare.com/durable-objects/api/sqlite-storage-api/), [at-least-once alarms](https://developers.cloudflare.com/durable-objects/api/alarms/), [GitHub issues](https://docs.github.com/en/rest/issues/issues) and [GitHub conditional-request limitations](https://docs.github.com/en/rest/using-the-rest-api/best-practices-for-using-the-rest-api#use-conditional-requests).
