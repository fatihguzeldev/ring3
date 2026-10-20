export const SOURCE_READ_BYTES = 1024 * 1024;

export async function* sourceChunks(engine, url, file, offset, length) {
  const start = BigInt(offset);
  if (
    !Number.isSafeInteger(file.size) ||
    file.size < 0 ||
    !Number.isSafeInteger(length) ||
    length < 0 ||
    start < 0n ||
    start + BigInt(length) > BigInt(file.size)
  )
    throw new RangeError("file source span is outside the mounted asset");
  for (let at = 0; at < length; at += SOURCE_READ_BYTES) {
    engine.signal?.throwIfAborted();
    const size = Math.min(SOURCE_READ_BYTES, length - at);
    const bytes = await engine.bytes(url, BigInt(file.id), start + BigInt(at), size);
    engine.signal?.throwIfAborted();
    if (!(bytes instanceof Uint8Array) || bytes.length !== size)
      throw new Error("file source returned a short or invalid read");
    yield { offset: at, bytes };
  }
}
