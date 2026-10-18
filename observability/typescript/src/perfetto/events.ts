import type { TraceArgs, TraceValue } from "../capture/types.js";
import { bytes, double, join, sint, string, uint } from "./writer.js";

export type EventType = 1 | 2 | 3 | 4;

function annotation(name: string, value: TraceValue): Uint8Array {
  if (name.length > 128 || (typeof value === "string" && value.length > 16_384))
    throw new RangeError("trace argument too large");
  const key = string(10, name);
  let encoded: Uint8Array;
  if (typeof value === "string") encoded = string(6, value);
  else if (typeof value === "boolean") encoded = uint(2, value ? 1 : 0);
  else if (typeof value === "bigint") encoded = value < 0n ? sint(4, value) : uint(3, value);
  else encoded = double(5, value);
  return bytes(4, join([key, encoded]));
}

function packet(sequence: number, timestamp: bigint, payload: Uint8Array): Uint8Array {
  return bytes(1, join([uint(8, timestamp), uint(58, 1), uint(10, sequence), payload]));
}

export function clockSnapshot(sequence: number, timestamp: bigint): Uint8Array {
  // the synthetic primary timeline needs no physical os boot clock in a browser.
  const realtime = join([uint(1, 1), uint(2, timestamp)]);
  const traceFile = join([uint(1, 11), uint(2, timestamp)]);
  return bytes(
    1,
    join([
      uint(10, sequence),
      bytes(6, join([bytes(1, realtime), bytes(1, traceFile), uint(2, 11)])),
    ]),
  );
}

export function descriptor(
  sequence: number,
  timestamp: bigint,
  track: bigint,
  name: string,
  parent?: bigint,
  counter = false,
): Uint8Array {
  if (name.length > 512) throw new RangeError("trace track name too large");
  const fields = [uint(1, track), string(2, name)];
  if (parent !== undefined) fields.push(uint(5, parent));
  if (counter) fields.push(bytes(8, new Uint8Array()));
  return packet(sequence, timestamp, bytes(60, join(fields)));
}

export function event(
  sequence: number,
  timestamp: bigint,
  track: bigint,
  type: EventType,
  context: string,
  name: string,
  args: TraceArgs = {},
): Uint8Array {
  if (name.length > 256) throw new RangeError("trace event name too large");
  const fields = [uint(9, type), uint(11, track)];
  if (type !== 2) fields.push(string(22, context), string(23, name));
  const entries = Object.entries(args);
  if (entries.length > 64) throw new RangeError("too many trace arguments");
  for (const [key, value] of entries) fields.push(annotation(key, value));
  return packet(sequence, timestamp, bytes(11, join(fields)));
}

export function counterEvent(
  sequence: number,
  timestamp: bigint,
  track: bigint,
  value: number | bigint,
): Uint8Array {
  const count = typeof value === "bigint" ? sint(30, value) : double(44, value);
  return packet(sequence, timestamp, bytes(11, join([uint(9, 4), uint(11, track), count])));
}
