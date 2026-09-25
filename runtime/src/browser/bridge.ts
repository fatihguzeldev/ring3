import type { Snapshot } from "./types.js";

interface Exports {
  memory: WebAssembly.Memory;
  ring3_input(length: number): number;
  ring3_command(operation: number, argument: number): number;
  ring3_output_pointer(): number;
  ring3_output_length(): number;
  ring3_frame_pointer(): number;
  ring3_frame_length(): number;
}

export class Bridge {
  readonly #exports: Exports;
  readonly #encoder: TextEncoder = new TextEncoder();
  readonly #decoder: TextDecoder = new TextDecoder("utf-8", { fatal: true });

  constructor(instance: WebAssembly.Instance) {
    const exports = instance.exports;
    if (!(exports["memory"] instanceof WebAssembly.Memory)) throw new Error("Wasm belleği eksik.");
    for (const name of ["ring3_input", "ring3_command", "ring3_output_pointer", "ring3_output_length", "ring3_frame_pointer", "ring3_frame_length"]) {
      if (typeof exports[name] !== "function") throw new Error(`Wasm bağlantısı eksik: ${name}`);
    }
    this.#exports = exports as unknown as Exports;
  }

  command(operation: number, argument = 0, input: unknown = null): Snapshot {
    const bytes = input instanceof Uint8Array ? input : this.#encoder.encode(JSON.stringify(input));
    if (bytes.length > 128 * 1024 * 1024 + 8) throw new Error("Dosya aktarım sınırı aşıldı.");
    const pointer = this.#exports.ring3_input(bytes.length);
    if (pointer === 0) throw new Error("Wasm giriş sınırı aşıldı.");
    new Uint8Array(this.#exports.memory.buffer, pointer, bytes.length).set(bytes);
    const success = this.#exports.ring3_command(operation, argument);
    const output = new Uint8Array(this.#exports.memory.buffer, this.#exports.ring3_output_pointer(), this.#exports.ring3_output_length());
    const response: unknown = JSON.parse(this.#decoder.decode(output));
    if (typeof response !== "object" || response === null) throw new Error("Geçersiz Wasm yanıtı.");
    if (!success) throw new Error(String((response as { error?: unknown }).error));
    return response as Snapshot;
  }

  frame(snapshot: Snapshot): ArrayBuffer | null {
    if (!snapshot.frame) return null;
    const { width, height } = snapshot.frame;
    const size = this.#exports.ring3_frame_length();
    if (size !== width * height * 4 || size > 16 * 1024 * 1024) throw new Error("Geçersiz görüntü boyutu.");
    return new Uint8Array(this.#exports.memory.buffer, this.#exports.ring3_frame_pointer(), size).slice().buffer;
  }
}
