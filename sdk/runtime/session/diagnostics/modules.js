export function liveModules(engine) {
  const result = { scope: 'point-in-time loader snapshot; not historical ownership',
    status: 'unavailable', modules: [], truncated: false, errors: [] };
  let saved, available = false, reads = 0;
  const error = (failure, entry) => {
    if (result.errors.length < 8) result.errors.push({ ...(entry === undefined ? {} : { entry }), message: String(failure?.message ?? failure).slice(0, 256) });
  };
  const span = (address, width) => {
    if (!Number.isInteger(address) || address < 0 || !Number.isInteger(width) || width < 0 || address + width > 0x100000000)
      throw new Error('module snapshot address span exceeds 32 bits');
    return address;
  };
  const read = (address, width = 4) => {
    span(address, width);
    if (++reads > 131072) { result.truncated = true; throw new Error('module snapshot read limit exceeded'); }
    const status = engine.status(width === 1 ? 'read8' : 'read32', address);
    if (status !== 0) throw new Error(`module snapshot read returned ${status}`);
    if (engine.get(100) !== 0x484d3352 || engine.get(104) !== (width === 1 ? 0x10002 : 0x10001) ||
        engine.get(108) !== 40 || engine.get(112) !== 0 || engine.get(116) !== 0)
      throw new Error(`module snapshot read failed at 0x${address.toString(16)}`);
    const value = engine.get(120);
    if (width === 1 && value > 255) throw new Error('invalid module snapshot byte');
    return value;
  };
  const query = (operation, ...args) => {
    const status = engine.status(operation, ...args);
    if (status !== 0) throw new Error(`${operation} returned ${status}`);
    const bytes = engine.take(72), data = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
    if (data.getUint32(0, true) !== 0x4f4d3352 || data.getUint32(4, true) !== 0x10001 ||
        data.getUint32(8, true) !== 72 || data.getUint32(12, true) !== 0 || data.getUint32(16, true) !== 2)
      throw new Error(`invalid ${operation} record`);
    return data.getUint32(20, true);
  };
  const wide = address => {
    span(address, 8);
    const lengths = read(address), length = lengths & 0xffff, maximum = lengths >>> 16, pointer = read(address + 4);
    if (!length || (length | maximum) & 1 || length > maximum || maximum > 4096 || !pointer)
      throw new Error('invalid module snapshot UTF16 descriptor');
    span(pointer, length);
    const bytes = new Uint8Array(length), data = new DataView(bytes.buffer);
    let offset = 0;
    for (; offset + 4 <= length; offset += 4) data.setUint32(offset, read(pointer + offset), true);
    for (; offset < length; offset++) bytes[offset] = read(pointer + offset, 1);
    const text = new TextDecoder('utf-16le', { fatal: true }).decode(bytes);
    if (text.includes('\0')) throw new Error('module snapshot name contains NUL');
    return text;
  };
  try {
    const bytes = new Uint8Array(engine.memory.buffer);
    if (!Number.isInteger(engine.base) || engine.base < 0 || engine.base + 4236 > bytes.length)
      throw new Error('module snapshot arena unavailable');
    saved = bytes.slice(engine.base + 100, engine.base + 4236);
    const teb = query('module_teb');
    if (!teb) throw new Error('module snapshot has no current TEB');
    const peb = read(span(teb + 0x30, 4)), ldr = peb && read(span(peb + 0x0c, 4));
    if (!peb || !ldr) throw new Error('module snapshot has no loader list');
    const head = span(ldr + 0x0c, 8), tail = read(head + 4), seen = new Set();
    let link = read(head), previous = head;
    available = true;
    while (link !== head) {
      if (seen.has(link)) throw new Error('module snapshot loader cycle');
      if (seen.size === 1024) { result.truncated = true; throw new Error('module snapshot module limit exceeded'); }
      if (!link || link % 4) throw new Error('invalid module snapshot loader link');
      span(link, 52); seen.add(link);
      if (read(link + 4) !== previous) throw new Error('module snapshot loader back link mismatch');
      const next = read(link);
      try {
        const base = read(link + 24), imageSize = read(link + 32), path = wide(link + 36), name = wide(link + 44);
        if (!base || !imageSize) throw new Error('invalid module snapshot image extent');
        span(base, imageSize);
        const encoded = new TextEncoder().encode(name);
        if (encoded.length > 4096) throw new Error('module snapshot lookup name exceeds 4096 bytes');
        engine.request(encoded);
        if (query('module_handle', encoded.length) !== base) throw new Error('module snapshot native handle mismatch');
        if ((read(base) & 0xffff) !== 0x5a4d) throw new Error('module snapshot has no live MZ header');
        const pe = read(span(base + 0x3c, 4));
        if (pe < 64 || pe > 1048576 || read(span(base + pe, 4)) !== 0x4550 ||
            (read(span(base + pe + 24, 4)) & 0xffff) !== 0x10b || read(span(base + pe + 80, 4)) !== imageSize)
          throw new Error('module snapshot live PE32 extent mismatch');
        if (result.modules.some(module => base < module.base + module.imageSize && module.base < base + imageSize))
          throw new Error('module snapshot overlapping image extents');
        result.modules.push({ name, path, base, imageSize });
      } catch (failure) { error(failure, link); }
      previous = link; link = next;
    }
    if (tail !== previous) throw new Error('module snapshot loader tail mismatch');
  } catch (failure) { error(failure); }
  finally {
    if (saved) {
      try { new Uint8Array(engine.memory.buffer, engine.base + 100, saved.length).set(saved); }
      catch (failure) { error(failure); }
    }
  }
  result.status = !available ? 'unavailable' : result.errors.length || result.truncated ? 'partial' : 'complete';
  return result;
}
