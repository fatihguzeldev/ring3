import assert from "node:assert/strict";
import { test } from "node:test";
import { Engine } from "../../engine.js";
import { Files } from "../../files.js";
import { Session } from "../../session.js";
import { CompletedWindowsCalls } from "../diagnostics.js";

for (const mode of ["off", "filtered", "enabled"]) {
  test(`${mode} worker observation controls return diagnostics allocation`, async (t) => {
    const reached = new Error("file setup reached");
    const engine = {
      api: {},
      get: () => 0,
      ok() {},
      request() {},
      take: () => new Uint8Array(64),
    };
    t.mock.method(Engine, "create", async () => engine);
    t.mock.method(Files.prototype, "configure", () => {
      throw reached;
    });
    const observer = {
      enabled: mode === "enabled",
      event() {},
      scopeAsync: (name, args, operation) => operation(),
    };
    const session = new Session(() => {});
    t.after(() => session.scheduler.close());
    await assert.rejects(
      session.start({
        manifest: { files: [], executable: "C:\\game.exe" },
        observation:
          mode === "off"
            ? undefined
            : { observer, bind() {}, capture: { observer: () => observer } },
      }),
      (error) => error === reached,
    );
    if (mode === "enabled") assert.ok(session.callDiagnostics instanceof CompletedWindowsCalls);
    else assert.equal(session.callDiagnostics, null);
  });
}
