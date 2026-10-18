export class TracePacketStream {
  readonly #limit: number;
  #header: number[] = [];
  #length = 0;
  #packet: Uint8Array | undefined;
  #offset = 0;
  #failed = false;

  constructor(maxPacketBytes: number) {
    this.#limit = maxPacketBytes;
  }

  get pendingBytes(): number {
    return this.#packet?.byteLength ?? this.#header.length;
  }

  get incomplete(): boolean {
    return this.#failed || this.#header.length > 0 || this.#packet !== undefined;
  }

  push(input: Uint8Array, emit: (packet: Uint8Array) => boolean): boolean {
    if (this.#failed) return false;
    let accepted = true;
    try {
      for (let index = 0; index < input.length;) {
        if (this.#packet === undefined) {
          const value = input[index++]!;
          if (this.#header.length === 0) {
            if (value !== 10)
              throw new Error("trace stream must contain repeated TracePacket field 1");
            this.#header.push(value);
            continue;
          }
          if (this.#header.length >= 6) throw new Error("invalid trace packet length");
          this.#length += (value & 127) * 2 ** ((this.#header.length - 1) * 7);
          this.#header.push(value);
          if (this.#length + this.#header.length > this.#limit)
            throw new Error("trace packet exceeds capture byte budget");
          if ((value & 128) !== 0) continue;
          this.#packet = new Uint8Array(this.#length + this.#header.length);
          this.#packet.set(this.#header);
          this.#offset = this.#header.length;
          this.#header = [];
          this.#length = 0;
        }
        const count = Math.min(input.length - index, this.#packet.length - this.#offset);
        this.#packet.set(input.subarray(index, index + count), this.#offset);
        this.#offset += count;
        index += count;
        if (this.#offset === this.#packet.length) {
          const packet = this.#packet;
          this.#packet = undefined;
          this.#offset = 0;
          accepted = emit(packet) && accepted;
        }
      }
    } catch (error) {
      this.#failed = true;
      this.#header = [];
      this.#packet = undefined;
      throw error;
    }
    return accepted;
  }
}
