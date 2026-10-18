import assert from "node:assert/strict";
import { execFile } from "node:child_process";
import { access, readFile } from "node:fs/promises";
import { createServer } from "node:http";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { promisify } from "node:util";
import { test } from "node:test";
import { categories, createCapture } from "../typescript/dist/index.js";
import { createHttpTraceOutput } from "../typescript/dist/output/client.js";
import { createNodeCaptureStore } from "../typescript/dist/output/node.js";
import { createTraceHttpHandler } from "../typescript/dist/output/http.js";

const repository = resolve(dirname(fileURLToPath(import.meta.url)), "../..");
const processor =
  process.env.RING3_TRACE_PROCESSOR ??
  join(repository, "target/observability-tools/trace_processor_shell");
const engineTrace = process.env.RING3_ENGINE_TRACE;
const execute = promisify(execFile);

test("real Perfetto parses a persisted cross-producer trace and SQL recovers scopes, counters and completeness", async (t) => {
  await access(processor);
  const store = await createNodeCaptureStore({
    rootDirectory: join(repository, "target/observability"),
  });
  let handler;
  const server = createServer(async (request, response) => {
    if (!(await handler(request, response))) {
      response.writeHead(404);
      response.end();
    }
  });
  await new Promise((resolveListening) => server.listen(0, "127.0.0.1", resolveListening));
  t.after(() => new Promise((resolveClosed) => server.close(resolveClosed)));
  const origin = `http://127.0.0.1:${server.address().port}`;
  handler = createTraceHttpHandler(store, { allowedOrigins: [origin] });
  const output = await createHttpTraceOutput({
    baseUrl: `${origin}/api/observability/captures`,
    metadata: { fixture: "authored interoperability", engineTrace: engineTrace !== undefined },
    fetch: (url, options = {}) =>
      fetch(url, { ...options, headers: { ...options.headers, origin } }),
  });
  const capture = createCapture({ level: "detailed", producerId: 101, output });
  if (engineTrace !== undefined) assert.equal(capture.ingest(await readFile(engineTrace)), true);
  const loader = capture.observer(categories.engineLoader);
  await loader.scopeAsync("load.fixture", { input: "authored bytes" }, async (operation) => {
    operation.scope("link.fixture", { modules: 2 }, () =>
      operation.event("module.ready", { bytes: 4096 }),
    );
    await Promise.resolve();
  });
  loader.counter("file.bytes", 4096);
  const result = await capture.finish("complete");
  assert.equal(result.complete, true);
  assert.equal(output.completion.complete, true);
  const trace = join(repository, "target/observability", output.id, "trace.pftrace");
  const query = async (sql) =>
    (await execute(processor, ["query", trace, sql], { timeout: 20_000, maxBuffer: 1024 * 1024 }))
      .stdout;
  const scopes = await query(
    "SELECT name, dur >= 0 AS closed FROM slice WHERE name IN ('load.fixture','link.fixture','module.ready','capture.finished') ORDER BY name",
  );
  assert.match(scopes, /"load\.fixture",1/);
  assert.match(scopes, /"link\.fixture",1/);
  assert.match(scopes, /"module\.ready",1/);
  assert.match(scopes, /"capture\.finished",1/);
  const counters = await query(
    "SELECT counter_track.name, counter.value FROM counter JOIN counter_track ON counter.track_id = counter_track.id WHERE counter_track.name = 'engine.loader:file.bytes'",
  );
  assert.match(counters, /"engine\.loader:file\.bytes",4096/);
  const completeness = await query(
    "SELECT EXTRACT_ARG(arg_set_id, 'debug.complete') AS complete FROM slice WHERE name = 'capture.finished'",
  );
  assert.match(completeness, /\n1\s*$/);
  const errors = await query(
    "SELECT name, value FROM stats WHERE severity IN ('error','data_loss') AND value > 0",
  );
  assert.equal(errors.trim(), '"name","value"');
  if (engineTrace !== undefined) {
    const engine = await query("SELECT name FROM slice WHERE name = 'compiled'");
    assert.match(engine, /"compiled"/);
  }
  console.log(`persisted trace: ${trace}`);
});
