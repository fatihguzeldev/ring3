import assert from 'node:assert/strict';
import test from 'node:test';
import {unsupportedInstruction} from '../runtime/session/diagnostics/instruction.js';

function fixture() {
  const memory = new WebAssembly.Memory({initial:1}), base = 4096, guest = new Map(), modules = new Map(), calls = [];
  const all = () => new Uint8Array(memory.buffer), d = () => new DataView(memory.buffer);
  all().set(Uint8Array.from({length:4796},(_,i) => i*7+3),base);
  const put = (address,value) => {for (let i = 0; i < 4; i++) guest.set(address+i,(value>>>(i*8))&255);};
  const engine = {memory,base,
    get: offset => d().getUint32(base+offset,true),
    request: value => all().set(value,base+140),
    take: length => all().slice(base+140,base+140+length),
    status(name,address) {
      calls.push([name,address]);
      if (name === 'module_handle') {
        const path = new TextDecoder().decode(all().slice(base+140,base+140+address));
        all().fill(0,base+140,base+212);
        for (const [offset,value] of [[0,0x4f4d3352],[4,0x10001],[8,72],[16,2],[20,modules.get(path) ?? 0]]) d().setUint32(base+140+offset,value,true);
        return 0;
      }
      const width = name === 'read8' ? 1 : 4; assert(['read8','read32'].includes(name));
      let value = 0,valid = true;
      for (let i = 0; i < width; i++) {if (!guest.has(address+i)) valid = false; value |= (guest.get(address+i) ?? 0) << (i*8);}
      all().fill(0,base+100,base+140);
      for (const [offset,word] of [[0,0x484d3352],[4,width === 1 ? 0x10002 : 0x10001],[8,40],[16,valid ? 0 : 1],[20,value>>>0]]) d().setUint32(base+100+offset,word,true);
      return 0;
    },
  };
  const pe = (address,size) => {put(address,0x5a4d); put(address+0x3c,0x80); put(address+0x80,0x4550); put(address+0x98,0x10b); put(address+0xd0,size);};
  return {engine,guest,modules,calls,put,pe,arena:() => all().slice(base,base+4796)};
}
test('unsupported bytes use current loaded full-path handles and restore the complete scratch loan', () => {
  const f = fixture(), pc = 0x10000120, code = [0x66,0x60,...Array(13).fill(0x90)];
  code.forEach((byte,i) => f.guest.set(pc+i,byte));
  f.modules.set('C:\\app\\loaded.dll',0x10000000); f.modules.set('C:\\app\\other.dll',0x20000000);
  f.pe(0x10000000,0x3000); f.pe(0x20000000,0x3000);
  const before = f.arena(), ram = [...f.guest];
  const assets = new Map(['unloaded.dll','loaded.dll','other.dll'].map((name,i) => [i,{path:`C:\\app\\${name}`} ]));
  const result = unsupportedInstruction(f.engine,assets,pc);
  assert.deepEqual(result.bytes,{hex:'6660'+'90'.repeat(13),requested:15,read:15});
  assert.deepEqual(result.module,{path:'C:\\app\\loaded.dll',base:0x10000000,rva:0x120,imageSize:0x3000});
  assert.equal(result.moduleScan.queried,3); assert.deepEqual(result.moduleScan.errors,[]);
  assert.deepEqual(f.arena(),before); assert.deepEqual([...f.guest],ram);
});
test('a partial readable window is explicit and does not leave a helper packet behind', () => {
  const f = fixture(); [0x66,0x60,0x90].forEach((byte,i) => f.guest.set(0x1000+i,byte));
  const before = f.arena(), result = unsupportedInstruction(f.engine,new Map(),0x1000);
  assert.equal(result.bytes.hex,'666090'); assert.equal(result.bytes.read,3); assert.match(result.bytes.error,/read failed/);
  assert.equal(result.module,null); assert.deepEqual(f.arena(),before);
});
test('module candidate queries are cold and bounded to 1024 paths', () => {
  const f = fixture(), before = f.arena();
  for (let i = 0; i < 15; i++) f.guest.set(0x1000+i,0x90);
  const assets = new Map(Array.from({length:1025},(_,i) => [i,{path:`C:\\app\\module${i}.dll`} ]));
  const result = unsupportedInstruction(f.engine,assets,0x1000);
  assert.equal(result.moduleScan.candidates,1025); assert.equal(result.moduleScan.queried,1024); assert.equal(result.moduleScan.truncated,true);
  assert.equal(f.calls.filter(([name]) => name === 'module_handle').length,1024); assert.deepEqual(f.arena(),before);
});
