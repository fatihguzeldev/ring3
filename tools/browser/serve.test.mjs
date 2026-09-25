import assert from "node:assert/strict";
import { mkdtemp, writeFile, rm, truncate } from "node:fs/promises";
import { get } from "node:http";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import { loadConfig, startServer } from "./serve.mjs";

test("local host serves localhost and IPv4 with configured files and a session token", async (t) => {
  const dir = await mkdtemp(join(tmpdir(), "ring3-host-"));
  t.after(() => rm(dir, { recursive: true, force: true }));
  const source = join(dir, "owned.exe");
  const config = join(dir, "private.json");
  await writeFile(source, "owned");
  await writeFile(
    config,
    JSON.stringify({ files: [{ path: "C:\\program.exe", role: "executable", source }] }),
  );
  const { server, url } = await startServer(config, { port: 0 });
  t.after(
    () =>
      new Promise((resolve) => {
        server.closeAllConnections();
        server.close(resolve);
      }),
  );
  const page = await fetch(url);
  const token = /data-token="([a-f0-9]{64})"/.exec(await page.text())[1];
  const headers = { "X-Ring3-Token": token };
  for (const hostname of ["localhost", "127.0.0.1"]) {
    const localUrl = new URL(url);
    localUrl.hostname = hostname;
    assert.equal((await fetch(localUrl)).status, 200, hostname);
    assert.equal((await fetch(new URL("/assets/worker.js", localUrl))).status, 200, hostname);
    assert.equal((await fetch(new URL("/core.wasm", localUrl), { headers })).status, 200, hostname);
    assert.equal(
      (
        await fetch(new URL("/manifest", localUrl), {
          headers: { ...headers, Origin: localUrl.origin },
        })
      ).status,
      200,
      hostname,
    );
    assert.equal(
      await (await fetch(new URL("/files/0", localUrl), { headers })).text(),
      "owned",
      hostname,
    );
  }
  assert.match(page.headers.get("content-security-policy"), /frame-ancestors 'none'/);
  assert.equal((await fetch(`${url}/manifest`)).status, 403);
  assert.equal((await fetch(`${url}/files/0`)).status, 403);
  assert.equal(
    (await fetch(`${url}/manifest`, { headers: { ...headers, Origin: "https://example.org" } }))
      .status,
    200,
  );
  assert.equal(
    (await fetch(`${url}/manifest`, { headers: { ...headers, "Sec-Fetch-Site": "cross-site" } }))
      .status,
    200,
  );
  const customHost = await new Promise((resolve, reject) => {
    get(`${url}/manifest`, { headers: { ...headers, Host: "ring3.local" } }, (response) => {
      response.resume();
      resolve(response.statusCode);
    }).on("error", reject);
  });
  assert.equal(customHost, 200);
  const crossSiteNavigation = await new Promise((resolve, reject) => {
    get(
      url,
      {
        headers: {
          "Sec-Fetch-Site": "cross-site",
          "Sec-Fetch-Mode": "navigate",
          "Sec-Fetch-Dest": "document",
        },
      },
      (response) => {
        response.resume();
        resolve(response.statusCode);
      },
    ).on("error", reject);
  });
  assert.equal(crossSiteNavigation, 200);
  const manifest = await (await fetch(`${url}/manifest`, { headers })).json();
  assert.deepEqual(manifest, { files: [{ path: "C:\\program.exe", role: "executable", size: 5 }] });
  assert.equal(
    (await fetch(`${url}/files/0`, { headers })).headers.get("access-control-allow-origin"),
    null,
  );
  assert.equal(await (await fetch(`${url}/files/0`, { headers })).text(), "owned");
  for (const path of [
    "/files/01",
    "/files/1",
    "/files/-1",
    "/private.json",
    "/files/0?source=/etc/passwd",
    "/%2e%2e/%2e%2e/etc/passwd",
  ]) {
    assert.equal((await fetch(`${url}${path}`, { headers })).status, 404, path);
  }
  assert.equal((await fetch(`${url}/files/0`, { method: "POST", headers })).status, 405);
  await writeFile(source, "other");
  assert.equal((await fetch(`${url}/files/0`, { headers })).status, 409);
});

test("slow file responses retain the two-response memory limit until disconnected", async (t) => {
  const dir = await mkdtemp(join(tmpdir(), "ring3-host-limit-"));
  t.after(() => rm(dir, { recursive: true, force: true }));
  const source = join(dir, "owned.exe");
  const config = join(dir, "private.json");
  await writeFile(source, "");
  await truncate(source, 16 * 1024 * 1024);
  await writeFile(
    config,
    JSON.stringify({ files: [{ path: "C:\\program.exe", role: "executable", source }] }),
  );
  const { server, url } = await startServer(config, { port: 0 });
  t.after(
    () =>
      new Promise((resolve) => {
        server.closeAllConnections();
        server.close(resolve);
      }),
  );
  const token = /data-token="([a-f0-9]{64})"/.exec(await (await fetch(url)).text())[1];
  const headers = { "X-Ring3-Token": token };
  const clients = await Promise.all(
    [0, 1].map(
      () =>
        new Promise((resolve, reject) => {
          get(`${url}/files/0`, { headers }, (response) => {
            response.pause();
            resolve(response);
          }).on("error", reject);
        }),
    ),
  );
  t.after(() => clients.forEach((client) => client.destroy()));
  assert.equal((await fetch(`${url}/files/0`, { headers })).status, 429);
});

test("config rejects duplicate virtual paths, unknown roles and oversized files", async (t) => {
  const dir = await mkdtemp(join(tmpdir(), "ring3-config-"));
  t.after(() => rm(dir, { recursive: true, force: true }));
  const source = join(dir, "owned.exe");
  const config = join(dir, "private.json");
  await writeFile(source, "owned");
  const valid = { path: "C:\\program.exe", role: "executable", source };
  for (const files of [
    [],
    [valid, { ...valid, path: "c:\\PROGRAM.EXE", role: "data" }],
    [{ ...valid, role: "unexpected" }],
    [{ ...valid, path: "C:\\..\\program.exe" }],
    [{ ...valid, path: "relative.exe" }],
  ]) {
    await writeFile(config, JSON.stringify({ files }));
    assert.throws(() => loadConfig(config));
  }
  await truncate(source, 128 * 1024 * 1024 + 1);
  await writeFile(config, JSON.stringify({ files: [valid] }));
  assert.throws(() => loadConfig(config), /exceeds host limit/);
});
