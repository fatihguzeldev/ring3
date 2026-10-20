export function unsupportedInstruction(engine, assets, pc) {
  const result = {pc,bytes:{hex:'',requested:15,read:0},module:null,
    moduleScan:{candidates:0,queried:0,limit:1024,truncated:false,errors:[]},
    semantics:'mapped byte window at the stopped PC; module extent comes from its live PE32 header'};
  const saved = new Uint8Array(engine.memory.buffer).slice(engine.base+100,engine.base+4236);
  function read(address,width) {
    if (!Number.isInteger(address) || address < 0 || address+width > 0x100000000)
      throw new Error('diagnostic address span exceeds 32 bits');
    const status = engine.status(width === 1 ? 'read8' : 'read32',address);
    if (status !== 0) throw new Error(`diagnostic read returned ${status}`);
    const version = width === 1 ? 2 : 1;
    if (engine.get(100) !== 0x484d3352 || engine.get(104) !== 0x10000+version || engine.get(108) !== 40 ||
        engine.get(112) !== 0 || engine.get(116) !== 0)
      throw new Error(`diagnostic memory read failed at 0x${address.toString(16)}`);
    return engine.get(120);
  }
  try {
    if (!Number.isInteger(pc) || pc < 0 || pc > 0xffffffff) throw new Error('invalid stopped instruction address');
    const bytes = [];
    try {for (let i = 0; i < 15; i++) bytes.push(read(pc+i,1));}
    catch (error) {result.bytes.error = error.message;}
    result.bytes.read = bytes.length;
    result.bytes.hex = bytes.map(value => value.toString(16).padStart(2,'0')).join('');
    const paths = [...new Set(Array.from(assets?.values?.() ?? [])
      .filter(file => typeof file.path === 'string' && (file.role === 'module' || file.role === 'executable' || /\.(exe|dll|ocx|drv)$/i.test(file.path)))
      .map(file => file.path))];
    result.moduleScan.candidates = paths.length; result.moduleScan.truncated = paths.length > 1024;
    const matches = [];
    for (const path of paths.slice(0,1024)) {
      result.moduleScan.queried++;
      try {
        const encoded = new TextEncoder().encode(path);
        if (encoded.length > 4096) throw new Error('module path exceeds the diagnostic transfer limit');
        engine.request(encoded);
        const status = engine.status('module_handle',encoded.length);
        if (status !== 0) throw new Error(`module_handle returned ${status}`);
        const raw = engine.take(72), d = new DataView(raw.buffer,raw.byteOffset,raw.byteLength);
        if (d.getUint32(0,true) !== 0x4f4d3352 || d.getUint32(4,true) !== 0x10001 ||
            d.getUint32(8,true) !== 72 || d.getUint32(12,true) !== 0 || d.getUint32(16,true) !== 2)
          throw new Error('invalid module handle record');
        const base = d.getUint32(20,true);
        if (!base || pc < base) continue;
        if ((read(base,4)&0xffff) !== 0x5a4d) continue;
        const pe = read(base+0x3c,4);
        if (pe < 64 || pe > 1024*1024 || read(base+pe,4) !== 0x4550 ||
            (read(base+pe+24,4)&0xffff) !== 0x10b) continue;
        const imageSize = read(base+pe+80,4);
        if (!imageSize || base+imageSize > 0x100000000 || pc-base >= imageSize) continue;
        if (matches.length < 8) matches.push({path,base,rva:pc-base,imageSize});
        else result.moduleScan.matchesTruncated = true;
      } catch (error) {
        if (result.moduleScan.errors.length < 8) result.moduleScan.errors.push({path,message:error.message});
      }
    }
    if (matches.length === 1 && !result.moduleScan.matchesTruncated) result.module = matches[0];
    else if (matches.length) {result.moduleMatches = matches; result.moduleScan.ambiguous = true;}
  } catch (error) {result.error = error.message;}
  finally {new Uint8Array(engine.memory.buffer,engine.base+100,saved.length).set(saved);}
  return result;
}
