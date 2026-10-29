import { record, view, utf16, utf8, pair } from "./engine.js";
import { inspectImports } from "./pe.js";
import { SYSTEM_MODULES } from "./configuration.js";
import { sourceChunks } from "./files/source.js";
import { FileReadMeasurements } from "./files/measurement.js";
export class Files {
  constructor(engine, manifest, bytesUrl) {
    this.engine = engine;
    this.manifest = manifest;
    this.bytesUrl = bytesUrl;
    this.assets = new Map(
      manifest.files.filter((f) => !f.isDirectory).map((f) => [BigInt(f.id), f]),
    );
    this.fetched = 0;
    this.measurements = engine.fileMeasurementsEnabled ? new FileReadMeasurements() : null;
  }
  snapshot() { return this.measurements?.snapshot(); }
  configure() {
    const h = this.engine,
      bytes = record("R3FC", 80),
      data = view(bytes);
    [
      Math.max(8192, this.manifest.files.length + 4096),
      2048,
      128,
      1024,
      128,
      16 * 1024 * 1024,
      4096,
      256,
      256,
      1024,
      256,
      32 * 1024 * 1024,
      0x40000000,
      65536,
    ].forEach((n, i) => data.setUint32(16 + i * 4, n, true));
    h.request(bytes);
    h.ok("configure_files", bytes.length);
    this.epoch = view(h.take(64)).getBigUint64(16, true);
  }
  mount(file) {
    const h = this.engine,
      path = utf16(file.path),
      long = path.length / 2 > 260;
    const bytes = record(long ? "R3FB" : "R3FM", long ? 64 : 64 + path.length),
      data = view(bytes);
    data.setBigUint64(16, file.isDirectory ? 0n : BigInt(file.id), true);
    data.setBigUint64(24, BigInt(file.size ?? 0), true);
    data.setUint32(32, file.isDirectory ? 0x10 : 0x80, true);
    data.setUint32(36, path.length / 2, true);
    if (!long) {
      bytes.set(path, 64);
      h.request(bytes);
      h.ok("mount_file", bytes.length);
      return;
    }
    h.request(bytes);
    h.ok("begin_file_mount", ...pair(this.epoch), 64);
    const token = view(h.take(64)).getBigUint64(24, true);
    for (let offset = 0; offset < path.length; offset += 4096) {
      const chunk = path.subarray(offset, offset + 4096);
      h.request(chunk);
      h.ok("append_file_input", ...pair(this.epoch), ...pair(token), offset / 2, chunk.length / 2);
    }
    h.ok("finish_file_mount", ...pair(this.epoch), ...pair(token));
  }
  environment(directory) {
    const paths = [directory, "C:\\Windows", "C:\\Windows\\System32", "C:\\Temp", directory],
      encoded = paths.map(utf16);
    const bytes = record("R3FE", 32 + paths.length * 4 + encoded.reduce((a, b) => a + b.length, 0)),
      data = view(bytes);
    data.setUint32(16, paths.length, true);
    data.setUint32(
      20,
      encoded.reduce((a, b) => a + b.length / 2, 0),
      true,
    );
    let offset = 32 + paths.length * 4;
    encoded.forEach((path, i) => {
      data.setUint32(32 + i * 4, path.length / 2, true);
      bytes.set(path, offset);
      offset += path.length;
    });
    this.engine.request(bytes);
    this.engine.ok("set_files_environment", ...pair(this.epoch), bytes.length);
  }
  async stageExecutable(path) {
    const h = this.engine,
      file = this.manifest.files.find((f) => f.path.toLowerCase() === path.toLowerCase());
    if (!file || file.isDirectory) throw new Error(`selected executable is absent: ${path}`);
    const imports = await inspectImports(file.size, (offset, length) =>
      h.bytes(this.bytesUrl, file.id, offset, length),
    );
    const available = new Set([
      "ntdll.dll",
      "shfolder.dll",
      ...SYSTEM_MODULES.map(([name, , dll]) => dll ?? `${name}.dll`),
    ]);
    for (const asset of this.assets.values())
      available.add(asset.path.split(/[\\/]/).at(-1).toLowerCase());
    const missing = imports.filter((name) => !available.has(name.toLowerCase()));
    if (missing.length)
      throw new Error(
        `PE import preflight: unavailable module${missing.length === 1 ? "" : "s"} ${missing.join(", ")}. This playground cannot start this executable with the current engine and local files.`,
      );
    h.ok("begin_module_input", file.size, h.name(file.path));
    for await (const source of sourceChunks(h, this.bytesUrl, file, 0n, file.size)) {
      this.measurements?.record(file, BigInt(source.offset), source.bytes.length, "module");
      for (let at = 0; at < source.bytes.length; at += 4096) {
        const bytes = source.bytes.subarray(at, at + 4096);
        h.request(bytes);
        h.ok("append_module_input", source.offset + at, bytes.length);
        this.fetched += bytes.length;
      }
    }
    h.ok("commit_module_input");
  }
  async supplyPending() {
    const h = this.engine;
    h.ok("pending_file_request");
    const request = h.take(80),
      data = view(request);
    if (data.getBigUint64(16, true) === 0n) return false;
    const asset = data.getBigUint64(48, true),
      offset = data.getBigUint64(56, true),
      length = data.getUint32(64, true);
    const file = this.assets.get(asset);
    if (!file) throw new Error(`engine requested unknown mounted asset${asset}`);
    for await (const source of sourceChunks(h, this.bytesUrl, file, offset, length)) {
      this.measurements?.record(file, offset + BigInt(source.offset), source.bytes.length, "file");
      for (let at = 0; at < source.bytes.length; at += 4016) {
        const bytes = source.bytes.subarray(at, at + 4016);
        const input = new Uint8Array(80 + bytes.length);
        input.set(request);
        input.set(bytes, 80);
        h.request(input);
        h.ok("supply_file_bytes", source.offset + at, bytes.length);
        this.fetched += bytes.length;
      }
    }
    return true;
  }
  async sourceProgress() {
    const h = this.engine;
    h.ok("pending_module_source");
    const data = view(h.take(96));
    if (!data.getUint32(12, true)) return false;
    const epoch = data.getBigUint64(16, true),
      token = data.getBigUint64(24, true);
    if (data.getUint32(88, true)) await this.supplyPending();
    h.ok("resume_module_source", ...pair(epoch), ...pair(token));
    return true;
  }
}
export function launch(engine, executable, commandLine, directory) {
  const environment = [
    ["SystemRoot", "C:\\Windows"],
    ["WINDIR", "C:\\Windows"],
    ["TEMP", "C:\\Temp"],
    ["TMP", "C:\\Temp"],
    ["PATH", `${directory};C:\\Windows\\System32;C:\\Windows`],
    ["USERNAME", "ring3"],
  ];
  const strings = [executable, commandLine || `"${executable}"`, directory].map(utf8),
    pairs = environment.map((pair) => pair.map(utf8));
  const total =
      56 +
      strings.reduce((n, b) => n + b.length, 0) +
      pairs.reduce((n, p) => n + 8 + p[0].length + p[1].length, 0),
    bytes = new Uint8Array(total),
    data = view(bytes);
  bytes.set(utf8("R3PL"));
  data.setUint32(4, 1, true);
  data.setUint32(8, total, true);
  data.setUint32(12, 1, true);
  data.setBigUint64(16, (BigInt(Date.now()) + 11644473600000n) * 10000n, true);
  data.setUint32(24, 1024 * 1024, true);
  data.setUint32(28, 65536, true);
  strings.forEach((s, i) => data.setUint32(32 + i * 4, s.length, true));
  data.setUint32(44, pairs.length, true);
  let at = 56;
  for (const s of strings) {
    bytes.set(s, at);
    at += s.length;
  }
  for (const [name, value] of pairs) {
    data.setUint32(at, name.length, true);
    data.setUint32(at + 4, value.length, true);
    at += 8;
    bytes.set(name, at);
    at += name.length;
    bytes.set(value, at);
    at += value.length;
  }
  engine.ok("begin_process_launch_input", bytes.length);
  for (let at = 0; at < bytes.length; at += 4096) {
    const chunk = bytes.subarray(at, at + 4096);
    engine.request(chunk);
    engine.ok("append_process_launch_input", at, chunk.length);
  }
  engine.ok("commit_process_launch_input");
}
