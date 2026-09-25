import { createServer } from "node:http";
import { randomBytes, timingSafeEqual } from "node:crypto";
import { readFile, open } from "node:fs/promises";
import { readFileSync, realpathSync, statSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const root = fileURLToPath(new URL("../../", import.meta.url));
const maxFile = 128 * 1024 * 1024;
const staticFiles = new Map([
  ["/style.css", ["runtime/browser/style.css", "text/css"]],
  ["/core.wasm", ["target/wasm32-unknown-unknown/release/ring3_browser.wasm", "application/wasm"]],
  ...["app", "worker", "bridge", "types"].map((name) => [
    `/assets/${name}.js`,
    [`runtime/dist/browser/${name}.js`, "text/javascript"],
  ]),
]);

export function loadConfig(path) {
  const config = JSON.parse(readFileSync(path, "utf8"));
  if (!Array.isArray(config.files) || !config.files.length || config.files.length > 100_000)
    throw new Error("invalid file count");
  const names = new Set();
  const files = config.files.map((file) => {
    if (
      typeof file.path !== "string" ||
      !/^[A-Za-z]:\\/.test(file.path) ||
      file.path.length > 32767 ||
      !/^[\x20-\x7e]+$/.test(file.path) ||
      file.path
        .slice(3)
        .split("\\")
        .some((part) => !part || part === "." || part === ".." || /[<>:"|?*/]/.test(part)) ||
      names.has(file.path.toLowerCase()) ||
      !["executable", "module", "deferred", "data"].includes(file.role) ||
      typeof file.source !== "string"
    )
      throw new Error("invalid file declaration");
    names.add(file.path.toLowerCase());
    const source = realpathSync(resolve(dirname(path), file.source));
    const stat = statSync(source, { bigint: true });
    if (!stat.isFile() || stat.size > BigInt(maxFile))
      throw new Error(`file exceeds host limit: ${file.path}`);
    return { path: file.path, role: file.role, size: Number(stat.size), source, stat };
  });
  if (files.filter((file) => file.role === "executable").length !== 1)
    throw new Error("exactly one executable is required");
  return files;
}

function sameFile(expected, current) {
  return (
    current.isFile() &&
    ["dev", "ino", "size", "mtimeNs", "ctimeNs"].every((key) => expected[key] === current[key])
  );
}

export async function startServer(configPath, { port = 8000 } = {}) {
  const files = loadConfig(resolve(configPath));
  const token = randomBytes(32).toString("hex");
  const tokenBytes = Buffer.from(token);
  const html = (await readFile(resolve(root, "runtime/browser/index.html"), "utf8")).replace(
    "RING3_SESSION_TOKEN",
    token,
  );
  for (const [path] of staticFiles.values()) statSync(resolve(root, path));
  let activeReads = 0;
  const server = createServer((request, response) => {
    const send = (status, body = "", type = "text/plain; charset=utf-8") => {
      response.writeHead(status, {
        "Content-Type": type,
        "Content-Length": Buffer.byteLength(body),
        "Cache-Control": "no-store",
        "X-Content-Type-Options": "nosniff",
        "Cross-Origin-Resource-Policy": "same-origin",
        "Content-Security-Policy":
          "default-src 'self'; script-src 'self' 'wasm-unsafe-eval'; worker-src 'self'; connect-src 'self'; style-src 'self'; frame-ancestors 'none'",
      });
      response.end(body);
    };
    void (async () => {
      if (request.method !== "GET") {
        send(405);
        return;
      }
      const url = new URL(request.url, "http://localhost");
      if (url.search) {
        send(404);
        return;
      }
      if (url.pathname === "/") {
        send(200, html, "text/html; charset=utf-8");
        return;
      }
      const privateRoute =
        url.pathname === "/manifest" ||
        url.pathname.startsWith("/files/") ||
        url.pathname === "/core.wasm";
      if (privateRoute) {
        const offered = Buffer.from(String(request.headers["x-ring3-token"] ?? ""));
        if (offered.length !== tokenBytes.length || !timingSafeEqual(offered, tokenBytes)) {
          send(403);
          return;
        }
      }
      if (url.pathname === "/manifest") {
        send(
          200,
          JSON.stringify({ files: files.map(({ path, size, role }) => ({ path, size, role })) }),
          "application/json",
        );
        return;
      }
      const match = /^\/files\/(0|[1-9][0-9]*)$/.exec(url.pathname);
      if (match) {
        const file = files[Number(match[1])];
        if (!file) {
          send(404);
          return;
        }
        if (activeReads >= 2) {
          send(429);
          return;
        }
        activeReads++;
        let reading = true;
        let responseDone = false;
        let released = false;
        const release = () => {
          if (!released && !reading && responseDone) {
            released = true;
            activeReads--;
          }
        };
        for (const event of ["finish", "close"])
          response.once(event, () => {
            responseDone = true;
            release();
          });
        try {
          const handle = await open(file.source, "r");
          try {
            if (!sameFile(file.stat, await handle.stat({ bigint: true }))) {
              send(409, "source file changed; restart the local host");
              return;
            }
            const bytes = await handle.readFile();
            if (
              !sameFile(file.stat, await handle.stat({ bigint: true })) ||
              bytes.length !== file.size
            ) {
              send(409, "source file changed");
              return;
            }
            send(200, bytes, "application/octet-stream");
          } finally {
            await handle.close();
          }
        } finally {
          reading = false;
          release();
        }
        return;
      }
      const asset = staticFiles.get(url.pathname);
      if (asset) {
        send(200, await readFile(resolve(root, asset[0])), asset[1]);
        return;
      }
      send(404);
    })().catch(() => {
      if (!response.headersSent) send(500, "local file could not be read");
      else response.destroy();
    });
  });
  await new Promise((resolve, reject) => {
    server.once("error", reject);
    server.listen(port, "127.0.0.1", resolve);
  });
  return { server, url: `http://localhost:${server.address().port}` };
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  const args = process.argv.slice(2);
  if (args.length !== 2 || args[0] !== "--config")
    throw new Error("usage: pnpm browser --config <private-local-config.json>");
  const { server, url } = await startServer(args[1]);
  console.log(`Ring3 local player: ${url}`);
  for (const signal of ["SIGINT", "SIGTERM"])
    process.on(signal, () => server.close(() => process.exit(0)));
}
