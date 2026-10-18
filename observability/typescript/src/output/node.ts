import {
  CAPTURE_ID_PATTERN,
  CaptureOutputError,
  requireCaptureId,
  requireSequence,
} from "./types.js";
import type { CaptureManifest, CaptureStore, PersistentTraceOutput } from "./types.js";

export interface NodeCaptureStoreOptions {
  readonly rootDirectory: string;
  readonly maxCaptureBytes?: number;
  readonly maxTotalBytes?: number;
  readonly maxChunkBytes?: number;
  readonly maxCaptures?: number;
}

interface ActiveCapture {
  manifest: CaptureManifest;
  finalized: boolean;
  lastDigest?: string;
  lastLength?: number;
}

export async function createNodeTraceOutput(
  store: CaptureStore,
  metadata: Readonly<Record<string, unknown>>,
): Promise<PersistentTraceOutput> {
  const { id } = await store.begin(metadata);
  let sequence = 0;
  let completion: CaptureManifest | undefined;
  let closed = false;
  return {
    id,
    get completion() {
      return completion;
    },
    async write(bytes) {
      if (closed) throw new CaptureOutputError("trace output is closed", 409);
      for (let offset = 0; offset < bytes.length; offset += store.maxChunkBytes) {
        await store.append(id, sequence, bytes.subarray(offset, offset + store.maxChunkBytes));
        sequence += 1;
      }
    },
    async flush() {
      await store.flush(id, sequence);
    },
    async close() {
      if (completion !== undefined) return;
      if (closed) throw new CaptureOutputError("trace output is closed", 409);
      completion = await store.finish(id, sequence);
      closed = true;
    },
    async abort(reason) {
      if (completion !== undefined) return;
      await store.abort(id, reason);
      closed = true;
    },
  };
}

export async function createNodeCaptureStore(
  options: NodeCaptureStoreOptions,
): Promise<CaptureStore> {
  const [filesystem, path, crypto, constants] = await Promise.all([
    import("node:fs/promises"),
    import("node:path"),
    import("node:crypto"),
    import("node:fs"),
  ]);
  const fs = filesystem.default;
  const maxCaptureBytes = options.maxCaptureBytes ?? 128 * 1024 * 1024;
  const maxTotalBytes = options.maxTotalBytes ?? 1024 * 1024 * 1024;
  const maxChunkBytes = options.maxChunkBytes ?? 1024 * 1024;
  const maxCaptures = options.maxCaptures ?? 128;
  for (const limit of [maxCaptureBytes, maxTotalBytes, maxChunkBytes, maxCaptures]) {
    if (!Number.isSafeInteger(limit) || limit <= 0)
      throw new CaptureOutputError("invalid output limit");
  }
  await fs.mkdir(options.rootDirectory, { recursive: true });
  const root = await fs.realpath(options.rootDirectory);
  const active = new Map<string, ActiveCapture>();
  const requests = new Map<string, { id: string; metadata: string }>();
  let totalBytes = 0;
  let captureCount = 0;
  let pending = Promise.resolve();

  function ordered<T>(operation: () => Promise<T>): Promise<T> {
    const result = pending.then(operation);
    pending = result.then(
      () => undefined,
      () => undefined,
    );
    return result;
  }

  async function directory(id: string): Promise<string> {
    requireCaptureId(id);
    const result = path.join(root, id);
    const info = await fs.lstat(result);
    if (!info.isDirectory() || info.isSymbolicLink() || (await fs.realpath(result)) !== result) {
      throw new CaptureOutputError("invalid capture directory", 409);
    }
    return result;
  }

  async function save(manifest: CaptureManifest): Promise<void> {
    const parent = await directory(manifest.id);
    const temporary = path.join(parent, `${crypto.randomUUID()}.tmp`);
    try {
      await fs.writeFile(temporary, JSON.stringify(manifest, null, 2), { flag: "wx", mode: 0o600 });
      await fs.rename(temporary, path.join(parent, "capture.json"));
    } finally {
      await fs.rm(temporary, { force: true });
    }
  }

  async function read(id: string): Promise<CaptureManifest> {
    const parent = await directory(id);
    const handle = await fs.open(
      path.join(parent, "capture.json"),
      constants.constants.O_RDONLY | constants.constants.O_NOFOLLOW,
    );
    try {
      const manifest = JSON.parse(await handle.readFile("utf8")) as CaptureManifest;
      if (manifest.id !== id || manifest.version !== 1)
        throw new CaptureOutputError("invalid capture manifest", 409);
      return manifest;
    } finally {
      await handle.close();
    }
  }

  async function sync(capture: ActiveCapture): Promise<void> {
    const parent = await directory(capture.manifest.id);
    const name = capture.finalized ? "trace.pftrace" : "trace.pftrace.partial";
    const handle = await fs.open(
      path.join(parent, name),
      constants.constants.O_RDONLY | constants.constants.O_NOFOLLOW,
    );
    try {
      await handle.sync();
    } finally {
      await handle.close();
    }
    await save(capture.manifest);
  }

  function current(id: string): ActiveCapture {
    requireCaptureId(id);
    const capture = active.get(id);
    if (!capture) throw new CaptureOutputError("capture is not active in this server", 404);
    return capture;
  }

  function checkSequence(capture: ActiveCapture, sequence: number): void {
    requireSequence(sequence);
    if (sequence !== capture.manifest.nextSequence)
      throw new CaptureOutputError("unexpected capture sequence", 409);
  }

  for (const entry of await fs.readdir(root, { withFileTypes: true })) {
    if (!CAPTURE_ID_PATTERN.test(entry.name)) continue;
    const parent = await directory(entry.name);
    captureCount += 1;
    for (const name of ["trace.pftrace", "trace.pftrace.partial"]) {
      try {
        const info = await fs.lstat(path.join(parent, name));
        if (!info.isFile() || info.isSymbolicLink())
          throw new CaptureOutputError("invalid trace file", 409);
        totalBytes += info.size;
      } catch (error) {
        if ((error as NodeJS.ErrnoException).code !== "ENOENT") throw error;
      }
    }
  }

  return {
    maxChunkBytes,
    begin: (metadata, requestId) =>
      ordered(async () => {
        let encoded: string;
        try {
          encoded = JSON.stringify(metadata);
        } catch {
          throw new CaptureOutputError("invalid metadata");
        }
        if (
          !metadata ||
          Array.isArray(metadata) ||
          typeof metadata !== "object" ||
          typeof encoded !== "string" ||
          new TextEncoder().encode(encoded).length > 64 * 1024
        ) {
          throw new CaptureOutputError("metadata exceeds output limit");
        }
        const snapshot = JSON.parse(encoded) as unknown;
        if (!snapshot || typeof snapshot !== "object" || Array.isArray(snapshot))
          throw new CaptureOutputError("invalid metadata object");
        if (requestId !== undefined && !/^[a-zA-Z0-9-]{1,128}$/.test(requestId))
          throw new CaptureOutputError("invalid request identifier");
        const previous = requestId === undefined ? undefined : requests.get(requestId);
        if (previous) {
          if (previous.metadata !== encoded)
            throw new CaptureOutputError("request metadata mismatch", 409);
          return { id: previous.id };
        }
        if (captureCount >= maxCaptures)
          throw new CaptureOutputError("capture count limit reached", 507);
        const id = crypto.randomUUID();
        const parent = path.join(root, id);
        await fs.mkdir(parent, { mode: 0o700 });
        const timestamp = new Date().toISOString();
        const manifest: CaptureManifest = {
          version: 1,
          id,
          createdAt: timestamp,
          updatedAt: timestamp,
          complete: false,
          status: "recording",
          bytes: 0,
          nextSequence: 0,
          metadata: snapshot as Record<string, unknown>,
        };
        try {
          await fs.writeFile(path.join(parent, "trace.pftrace.partial"), new Uint8Array(), {
            flag: "wx",
            mode: 0o600,
          });
          await save(manifest);
        } catch (error) {
          await fs.rm(parent, { recursive: true, force: true });
          throw error;
        }
        active.set(id, { manifest, finalized: false });
        captureCount += 1;
        if (requestId !== undefined) requests.set(requestId, { id, metadata: encoded });
        return { id };
      }),
    append: (id, sequence, bytes) =>
      ordered(async () => {
        const capture = current(id);
        if (capture.manifest.status !== "recording" || capture.finalized)
          throw new CaptureOutputError("capture is closed", 409);
        requireSequence(sequence);
        if (!(bytes instanceof Uint8Array) || bytes.length === 0 || bytes.length > maxChunkBytes)
          throw new CaptureOutputError("chunk exceeds output limit", 413);
        const digest = crypto.createHash("sha256").update(bytes).digest("hex");
        if (sequence === capture.manifest.nextSequence - 1) {
          if (capture.lastDigest !== digest || capture.lastLength !== bytes.length)
            throw new CaptureOutputError("retry payload mismatch", 409);
          return { nextSequence: capture.manifest.nextSequence };
        }
        checkSequence(capture, sequence);
        if (capture.manifest.bytes + bytes.length > maxCaptureBytes)
          throw new CaptureOutputError("capture byte limit reached", 507);
        if (totalBytes + bytes.length > maxTotalBytes)
          throw new CaptureOutputError("total byte limit reached", 507);
        const parent = await directory(id);
        const handle = await fs.open(
          path.join(parent, "trace.pftrace.partial"),
          constants.constants.O_WRONLY |
            constants.constants.O_APPEND |
            constants.constants.O_NOFOLLOW,
        );
        try {
          let offset = 0;
          while (offset < bytes.length) {
            const result = await handle.write(bytes, offset, bytes.length - offset);
            if (result.bytesWritten === 0)
              throw new CaptureOutputError("trace output made no progress", 500);
            offset += result.bytesWritten;
          }
          await handle.close();
        } catch (error) {
          const info = await fs
            .lstat(path.join(parent, "trace.pftrace.partial"))
            .catch(() => undefined);
          const size = info?.isFile() ? info.size : capture.manifest.bytes + bytes.length;
          totalBytes += Math.max(0, size - capture.manifest.bytes);
          capture.manifest = {
            ...capture.manifest,
            bytes: size,
            status: "aborted",
            reason: "trace write failed",
            updatedAt: new Date().toISOString(),
          };
          await save(capture.manifest).catch(() => undefined);
          await handle.close().catch(() => undefined);
          throw error;
        }
        totalBytes += bytes.length;
        capture.lastDigest = digest;
        capture.lastLength = bytes.length;
        capture.manifest = {
          ...capture.manifest,
          bytes: capture.manifest.bytes + bytes.length,
          nextSequence: sequence + 1,
          updatedAt: new Date().toISOString(),
        };
        return { nextSequence: sequence + 1 };
      }),
    flush: (id, sequence) =>
      ordered(async () => {
        const capture = current(id);
        checkSequence(capture, sequence);
        await sync(capture);
      }),
    finish: (id, sequence) =>
      ordered(async () => {
        const capture = current(id);
        checkSequence(capture, sequence);
        if (capture.manifest.complete) return capture.manifest;
        if (capture.manifest.status !== "recording")
          throw new CaptureOutputError("capture is closed", 409);
        await sync(capture);
        const parent = await directory(id);
        if (!capture.finalized) {
          await fs.rename(
            path.join(parent, "trace.pftrace.partial"),
            path.join(parent, "trace.pftrace"),
          );
          capture.finalized = true;
        }
        const completed: CaptureManifest = {
          ...capture.manifest,
          complete: true,
          status: "complete",
          updatedAt: new Date().toISOString(),
        };
        await save(completed);
        capture.manifest = completed;
        return completed;
      }),
    abort: (id, reason = "capture aborted") =>
      ordered(async () => {
        const capture = current(id);
        if (capture.manifest.complete) throw new CaptureOutputError("capture is closed", 409);
        capture.manifest = {
          ...capture.manifest,
          status: "aborted",
          reason: reason.slice(0, 1024),
          updatedAt: new Date().toISOString(),
        };
        await sync(capture);
        return capture.manifest;
      }),
    get: (id) => ordered(async () => read(id)),
  };
}
