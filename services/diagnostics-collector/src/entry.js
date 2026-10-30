import { DurableObject } from "cloudflare:workers";
import worker, { DiagnosticsQueue as Queue } from "./worker.js";

// Keep the Cloudflare platform wrapper separate so the production queue/SQL
// and privacy contract can be tested directly with Node's built-in SQLite.
export class DiagnosticsQueue extends DurableObject {
  constructor(ctx, env) { super(ctx, env); this.queue = new Queue(ctx, env); }
  fetch(request) { return this.queue.fetch(request); }
  alarm() { return this.queue.alarm(); }
}
export default worker;
