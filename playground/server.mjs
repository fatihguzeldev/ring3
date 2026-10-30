import { createServer } from "node:http";
import { open, readFile, readdir, realpath, stat } from "node:fs/promises";
import { createReadStream } from "node:fs";
import { dirname, extname, isAbsolute, join, relative, resolve, sep } from "node:path";
import { fileURLToPath } from "node:url";
import { execFile } from "node:child_process";
import { promisify } from "node:util";
import { engineRuntime } from "./engine-runtime.mjs";

const here = dirname(fileURLToPath(import.meta.url));
const observabilityRoot = resolve(here, "../observability/typescript");
await promisify(execFile)(process.execPath, [
  resolve(here, "../observability/catalog/generate.mjs"),
  "--check",
]);
await promisify(execFile)(process.execPath, [
  join(observabilityRoot, "node_modules/typescript/bin/tsc"),
  "-p",
  join(observabilityRoot, "tsconfig.json"),
]);
const { createNodeCaptureStore } = await import("../observability/typescript/dist/output/node.js");
const { createTraceHttpHandler } = await import("../observability/typescript/dist/output/http.js");
const args = process.argv.slice(2);
function option(name, fallback) {
  const at = args.indexOf(name);
  if (at < 0) return fallback;
  if (!args[at + 1] || args[at + 1].startsWith("--")) throw new Error(`missing ${name} value`);
  return args[at + 1];
}
const port = Number(option("--port", "4173"));
if (!Number.isInteger(port) || port < 1024 || port > 65535) throw new Error("invalid port");
const traceStore = await createNodeCaptureStore({
  rootDirectory: resolve(here, "../target/observability"),
  maxCaptureBytes: Number(option("--trace-bytes", String(512 * 1024 * 1024))),
  maxTotalBytes: Number(option("--trace-total", String(4 * 1024 * 1024 * 1024))),
});
const traceRequest = createTraceHttpHandler(traceStore, {
  allowedOrigins: [`http://localhost:${port}`, `http://127.0.0.1:${port}`],
});
const wasm = resolve(
  option("--engine", join(here, "../target/wasm32-unknown-unknown/release/ring3_engine.wasm")),
);
const engineBytes = await readFile(wasm);
const runtime = engineRuntime(await WebAssembly.compile(engineBytes), {
  relaxedFpu: args.includes("--relaxed-fpu"),
  fastPreset: args.includes("--fast-preset"),
});
const mime = new Map([
  [".html", "text/html; charset=utf-8"],
  [".js", "text/javascript"],
  [".mjs", "text/javascript"],
  [".css", "text/css"],
  [".json", "application/json"],
  [".wasm", "application/wasm"],
]);
let mount = { root: "", files: [], sources: new Map(), executable: "" };
let nextSourceId = 1;
let diagnostics = [];

async function scan(root, executable) {
  if (!isAbsolute(root)) throw new Error("game root must be an absolute directory path");
  root = await realpath(root);
  if (!(await stat(root)).isDirectory()) throw new Error("game root is not a directory");
  const files = [];
  async function walk(directory) {
    const entries = (await readdir(directory, { withFileTypes: true })).sort((a, b) =>
      a.name.localeCompare(b.name),
    );
    for (const entry of entries) {
      if (entry.name.startsWith(".") || entry.isSymbolicLink()) continue;
      const source = join(directory, entry.name);
      const path = "C:\\" + relative(root, source).split(sep).join("\\");
      if (entry.isDirectory()) {
        files.push({ id: nextSourceId++, path, source, size: 0, isDirectory: true });
        await walk(source);
      } else if (entry.isFile()) {
        const info = await stat(source);
        files.push({
          id: nextSourceId++,
          path,
          source,
          size: info.size,
          role: /\.exe$/i.test(entry.name)
            ? "executable"
            : /\.dll$/i.test(entry.name)
              ? "module"
              : "file",
        });
      }
      if (files.length > 100000) throw new Error("game directory exceeds 100000 entries");
    }
  }
  await walk(root);
  const executables = files.filter((file) => file.role === "executable");
  const selected = executable
    ? executables.find((file) => file.path.toLowerCase() === executable.toLowerCase())
    : (executables.find((file) => /\\maxpayne\.exe$/i.test(file.path)) ?? executables[0]);
  if (executable && !selected) throw new Error("executable is not in the selected directory");
  return {
    root,
    files,
    sources: new Map(files.map((file) => [file.id, file])),
    executable: selected?.path ?? "",
  };
}

function game() {
  return {
    configured: Boolean(mount.executable),
    root: mount.root,
    executables: mount.files
      .filter((file) => file.role === "executable")
      .map((file) => ({ path: file.path, label: file.path })),
    selectedExecutable: mount.executable,
    engineUrl: "/engine.wasm",
    runtime,
  };
}
function json(response, value, status = 200) {
  response.writeHead(status, { "Content-Type": "application/json" });
  response.end(JSON.stringify(value));
}
async function body(request) {
  const chunks = [];
  let size = 0;
  for await (const chunk of request) {
    size += chunk.length;
    if (size > 1024 * 1024) throw new Error("request body exceeds 1 MiB");
    chunks.push(chunk);
  }
  return JSON.parse(Buffer.concat(chunks).toString("utf8"));
}
async function serve(response, source) {
  const info = await stat(source);
  if (!info.isFile()) return json(response, { error: "not a file" }, 404);
  response.writeHead(200, {
    "Content-Type": mime.get(extname(source)) ?? "application/octet-stream",
    "Content-Length": info.size,
  });
  const stream = createReadStream(source);
  stream.on("error", () => response.destroy());
  response.on("close", () => stream.destroy());
  stream.pipe(response);
}

const initial = option("--game", "");
if (initial) mount = await scan(initial, option("--executable", ""));
const server = createServer(async (request, response) => {
  response.setHeader("Cache-Control", "no-store");
  response.setHeader("Cross-Origin-Opener-Policy", "same-origin");
  response.setHeader("Cross-Origin-Embedder-Policy", "require-corp");
  response.setHeader("Cross-Origin-Resource-Policy", "same-origin");
  response.setHeader("X-Content-Type-Options", "nosniff");
  try {
    const hosts = new Set([`localhost:${port}`, `127.0.0.1:${port}`]);
    if (!hosts.has(request.headers.host)) return json(response, { error: "invalid host" }, 403);
    if (request.headers.origin && !hosts.has(new URL(request.headers.origin).host))
      return json(response, { error: "cross-origin request denied" }, 403);
    if (request.headers["sec-fetch-site"] === "cross-site")
      return json(response, { error: "cross-site request denied" }, 403);
    if (await traceRequest(request, response)) return;
    const url = new URL(request.url, `http://127.0.0.1:${port}`);
    if (url.pathname === "/api/game") {
      if (request.method === "GET") return json(response, game());
      if (request.method !== "POST") return json(response, { error: "method not allowed" }, 405);
      const config = await body(request);
      if (config.root) mount = await scan(config.root, config.executable);
      else if (config.executable) {
        const selected = mount.files.find(
          (file) =>
            file.role === "executable" &&
            file.path.toLowerCase() === config.executable.toLowerCase(),
        );
        if (!selected) throw new Error("unknown executable");
        mount = { ...mount, executable: selected.path };
      }
      return json(response, game());
    }
    if (url.pathname === "/api/diagnostics") {
      if (request.method === "GET") return json(response, diagnostics);
      if (request.method !== "POST") return json(response, { error: "method not allowed" }, 405);
      diagnostics.push({ time: new Date().toISOString(), event: await body(request) });
      if (diagnostics.length > 256) diagnostics.shift();
      return json(response, { ok: true });
    }
    if (request.method !== "GET") return json(response, { error: "method not allowed" }, 405);
    if (url.pathname === "/files/manifest")
      return json(response, {
        root: "C:\\",
        executable: mount.executable,
        files: mount.files.map(({ source, ...file }) => file),
      });
    if (url.pathname === "/files/bytes") {
      const id = Number(url.searchParams.get("id"));
      const offset = Number(url.searchParams.get("offset"));
      const length = Number(url.searchParams.get("length"));
      const file = mount.sources.get(id);
      if (
        !url.searchParams.has("id") ||
        !url.searchParams.has("offset") ||
        !url.searchParams.has("length") ||
        !Number.isSafeInteger(id) ||
        !file ||
        file.isDirectory ||
        !Number.isSafeInteger(offset) ||
        !Number.isSafeInteger(length) ||
        offset < 0 ||
        length < 0 ||
        length > 1024 * 1024 ||
        offset + length > file.size
      ) {
        return json(response, { error: "invalid file span" }, 400);
      }
      const source = await realpath(file.source);
      if (source !== file.source) return json(response, { error: "file identity changed" }, 409);
      const handle = await open(source, "r");
      try {
        if ((await handle.stat()).size !== file.size)
          return json(response, { error: "file size changed" }, 409);
        const data = Buffer.alloc(length);
        const { bytesRead } = await handle.read(data, 0, length, offset);
        if (bytesRead !== length) throw new Error("short file read");
        response.writeHead(200, {
          "Content-Type": "application/octet-stream",
          "Content-Length": length,
        });
        response.end(data);
      } finally {
        await handle.close();
      }
      return;
    }
    if (url.pathname === "/engine.wasm") {
      response.writeHead(200, {
        "Content-Type": "application/wasm",
        "Content-Length": engineBytes.byteLength,
      });
      response.end(engineBytes);
      return;
    }
    if (url.pathname.startsWith("/sdk/")) {
      const root = resolve(here, "../sdk"),
        source = resolve(root, decodeURIComponent(url.pathname.slice("/sdk/".length)));
      if (!source.startsWith(root + sep) || ![".html", ".js", ".mjs", ".css", ".wasm"].includes(extname(source)))
        return json(response, { error: "not found" }, 404);
      if ((await realpath(source)) !== source)
        return json(response, { error: "symbolic link denied" }, 403);
      return await serve(response, source);
    }
    if (url.pathname.startsWith("/observability-proof/")) {
      const root = resolve(here, "../observability/tests"),
        source = resolve(
          root,
          decodeURIComponent(url.pathname.slice("/observability-proof/".length)),
        );
      if (!source.startsWith(root + sep) || ![".html", ".js"].includes(extname(source)))
        return json(response, { error: "not found" }, 404);
      if ((await realpath(source)) !== source)
        return json(response, { error: "symbolic link denied" }, 403);
      return await serve(response, source);
    }
    if (url.pathname.startsWith("/observability/typescript/dist/")) {
      const root = join(observabilityRoot, "dist"),
        source = resolve(
          root,
          decodeURIComponent(url.pathname.slice("/observability/typescript/dist/".length)),
        );
      if (!source.startsWith(root + sep) || extname(source) !== ".js")
        return json(response, { error: "not found" }, 404);
      if ((await realpath(source)) !== source)
        return json(response, { error: "symbolic link denied" }, 403);
      return await serve(response, source);
    }
    const pathname = decodeURIComponent(url.pathname).replace(/^\/playground\//, "/");
    const source = resolve(here, "." + (pathname === "/" ? "/index.html" : pathname));
    if (!source.startsWith(here + sep) || !mime.has(extname(source)))
      return json(response, { error: "not found" }, 404);
    const actual = await realpath(source);
    if (actual !== source) return json(response, { error: "symbolic link denied" }, 403);
    await serve(response, source);
  } catch (error) {
    if (response.headersSent) response.destroy();
    else
      json(
        response,
        { error: error.code === "ENOENT" ? "file not found" : error.message },
        error.code === "ENOENT" ? 404 : 400,
      );
  }
});
server.listen(port, "127.0.0.1", () =>
  console.log(
    `temporary Ring3 playground: http://127.0.0.1:${port}\ngame: ${mount.root || "(select a directory in the page)"}`,
  ),
);
server.on("error", (error) => {
  console.error(error.message);
  process.exitCode = 1;
});
