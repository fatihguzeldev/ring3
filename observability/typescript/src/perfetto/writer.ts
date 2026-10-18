const textEncoder = new TextEncoder();

export function join(parts: readonly Uint8Array[]): Uint8Array {
  const result = new Uint8Array(parts.reduce((size, part) => size + part.byteLength, 0));
  let offset = 0;
  for (const part of parts) {
    result.set(part, offset);
    offset += part.byteLength;
  }
  return result;
}

function varint(value: bigint): Uint8Array {
  if (value < 0n || value > 0xffff_ffff_ffff_ffffn)
    throw new RangeError("protobuf uint64 out of range");
  const bytes: number[] = [];
  do {
    const next = Number(value & 127n);
    value >>= 7n;
    bytes.push(value === 0n ? next : next | 128);
  } while (value !== 0n);
  return Uint8Array.from(bytes);
}

export function uint(field: number, value: bigint | number): Uint8Array {
  return join([varint(BigInt(field * 8)), varint(BigInt(value))]);
}

export function sint(field: number, value: bigint): Uint8Array {
  if (value < -0x8000_0000_0000_0000n || value > 0x7fff_ffff_ffff_ffffn)
    throw new RangeError("protobuf int64 out of range");
  return uint(field, BigInt.asUintN(64, value));
}

export function bytes(field: number, value: Uint8Array): Uint8Array {
  return join([varint(BigInt(field * 8 + 2)), varint(BigInt(value.byteLength)), value]);
}

export function string(field: number, value: string): Uint8Array {
  return bytes(field, textEncoder.encode(value));
}

export function double(field: number, value: number): Uint8Array {
  if (!Number.isFinite(value)) throw new RangeError("trace numbers must be finite");
  const data = new Uint8Array(8);
  new DataView(data.buffer).setFloat64(0, value, true);
  return join([varint(BigInt(field * 8 + 1)), data]);
}
