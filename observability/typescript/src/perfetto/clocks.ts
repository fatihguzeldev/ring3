interface Field {
  readonly number: number;
  readonly wire: number;
  readonly value: bigint | Uint8Array;
}

function fields(input: Uint8Array): Field[] {
  let offset = 0;
  function varint(): bigint {
    let result = 0n;
    for (let index = 0; index < 10; index++) {
      const byte = input[offset++];
      if (byte === undefined || (index === 9 && byte > 1))
        throw new Error("invalid protobuf varint");
      result |= BigInt(byte & 127) << BigInt(index * 7);
      if ((byte & 128) === 0) return result;
    }
    throw new Error("invalid protobuf varint");
  }
  const result: Field[] = [];
  while (offset < input.length) {
    const tag = varint();
    if (tag > 0xffff_ffffn || tag < 8n) throw new Error("invalid protobuf tag");
    const wire = Number(tag & 7n);
    const number = Number(tag >> 3n);
    let value: bigint | Uint8Array;
    if (wire === 0) value = varint();
    else if (wire === 1 || wire === 5) {
      const size = wire === 1 ? 8 : 4;
      if (offset + size > input.length) throw new Error("truncated protobuf fixed value");
      value = input.subarray(offset, offset + size);
      offset += size;
    } else if (wire === 2) {
      const size = Number(varint());
      if (size > input.length - offset) throw new Error("truncated protobuf bytes");
      value = input.subarray(offset, offset + size);
      offset += size;
    } else throw new Error("unsupported protobuf wire type");
    result.push({ number, wire, value });
  }
  return result;
}

function scalar(entries: readonly Field[], number: number): bigint | undefined {
  const matching = entries.filter((entry) => entry.number === number);
  if (matching.length !== 1 || matching[0]?.wire !== 0) return undefined;
  const value = matching[0].value;
  return typeof value === "bigint" ? value : undefined;
}

export function isIdentityClockSnapshot(packet: Uint8Array): boolean {
  const outer = fields(packet);
  const payload = outer[0];
  if (outer.length !== 1 || payload?.number !== 1 || !(payload.value instanceof Uint8Array)) {
    throw new Error("expected one standard TracePacket");
  }
  const entries = fields(payload.value);
  const snapshot = entries.find((entry) => entry.number === 6);
  if (snapshot === undefined) return false;
  const incompatible = (): never => {
    throw new Error("incompatible imported trace clock snapshot");
  };
  if (
    entries.some((entry) => entry.number !== 6 && entry.number !== 10) ||
    entries.filter((entry) => entry.number === 6).length !== 1 ||
    !(snapshot.value instanceof Uint8Array)
  )
    return incompatible();
  const body = fields(snapshot.value);
  if (scalar(body, 2) !== 11n || body.some((entry) => entry.number !== 1 && entry.number !== 2))
    return incompatible();
  const readings = body.filter((entry) => entry.number === 1);
  if (readings.length !== 2) return incompatible();
  const clocks = new Map<bigint, bigint>();
  for (const reading of readings) {
    if (!(reading.value instanceof Uint8Array)) return incompatible();
    const clock = fields(reading.value);
    const id = scalar(clock, 1);
    const timestamp = scalar(clock, 2);
    if (
      (id !== 1n && id !== 11n) ||
      timestamp === undefined ||
      clocks.has(id) ||
      clock.some((entry) => ![1, 2, 3, 4].includes(entry.number)) ||
      (clock.some((entry) => entry.number === 3) && scalar(clock, 3) !== 0n) ||
      (clock.some((entry) => entry.number === 4) && scalar(clock, 4) !== 1n)
    )
      return incompatible();
    clocks.set(id, timestamp);
  }
  if (clocks.get(1n) !== clocks.get(11n)) return incompatible();
  return true;
}
