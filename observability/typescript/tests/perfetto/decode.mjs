export function fields(bytes) {
  let offset = 0;
  function varint() {
    let result = 0n;
    let shift = 0n;
    for (let index = 0; index < 10; index++) {
      if (offset >= bytes.length) throw Error("truncated protobuf");
      const byte = bytes[offset++];
      result |= BigInt(byte & 127) << shift;
      if (!(byte & 128)) return result;
      shift += 7n;
    }
    throw Error("invalid protobuf varint");
  }
  const result = [];
  while (offset < bytes.length) {
    const tag = Number(varint());
    const field = tag >>> 3;
    const wire = tag & 7;
    let value;
    if (wire === 0) value = varint();
    else if (wire === 1) {
      if (offset + 8 > bytes.length) throw Error("truncated fixed64");
      value = new DataView(bytes.buffer, bytes.byteOffset + offset, 8).getFloat64(0, true);
      offset += 8;
    } else if (wire === 2) {
      const length = Number(varint());
      if (offset + length > bytes.length) throw Error("truncated bytes");
      value = bytes.subarray(offset, offset + length);
      offset += length;
    } else throw Error(`unsupported wire type ${wire}`);
    result.push({ field, wire, value });
  }
  return result;
}

export function get(fields, field) {
  return fields.find((entry) => entry.field === field)?.value;
}
export function text(value) {
  return new TextDecoder().decode(value);
}

export function packets(chunks) {
  return chunks.flatMap((chunk) =>
    fields(chunk).map((entry) => {
      if (entry.field !== 1) throw Error("not a standard Trace packet stream");
      const packet = fields(entry.value);
      const event = get(packet, 11);
      const descriptor = get(packet, 60);
      return {
        packet,
        event: event && fields(event),
        descriptor: descriptor && fields(descriptor),
      };
    }),
  );
}

export function memoryOutput() {
  const chunks = [];
  return {
    chunks,
    async write(bytes) {
      chunks.push(bytes.slice());
    },
    async flush() {},
    async close() {},
  };
}
