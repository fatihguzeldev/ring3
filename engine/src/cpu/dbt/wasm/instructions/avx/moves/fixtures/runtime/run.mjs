import assert from 'node:assert/strict';
let input = '';
for await (const chunk of process.stdin) input += chunk;
const cases = JSON.parse(input);
const STATE = 256, LOWER = STATE + 64, UPPER = STATE + 256, GENERAL = STATE, MXCSR = STATE + 32;
function requirement(c) {
  if (c.tag === 'load' || c.tag === 'store') return [c.width, c.aligned ? c.width : 1];
  if (c.tag === 'load_scalar' || c.tag === 'store_scalar') return [c.element, 1];
  if (c.tag === 'load_half' || c.tag === 'store_half' || c.tag === 'store_qword') return [8, 1];
  if (c.tag.startsWith('duplicate_') && c.source === 'memory') return [c.tag === 'duplicate_double_low' && c.width === 16 ? 8 : c.width, 1];
  if (c.tag === 'load_qword' && c.source === 'memory') return [8, 1];
  if ((c.tag === 'load_dword' && c.source === 'memory') || (c.tag === 'store_dword' && c.destination === 'memory') || c.tag === 'load_control' || c.tag === 'store_control') return [4, 1];
  return [0, 1];
}
for (const [index, c] of cases.entries()) {
  const label = `${index}:${c.tag}:${c.width ?? c.element ?? ''}:${c.address}`;
  const memory = new WebAssembly.Memory({initial: 1, maximum: 1});
  const bytes = new Uint8Array(memory.buffer);
  for (let i = 0; i < bytes.length; i++) bytes[i] = (i * 37 + (i >>> 6) * 53 + 19) & 255;
  const initialView = new DataView(memory.buffer);
  initialView.setUint32(MXCSR, c.invalidState ? 0x10000 : 0xe5af, true);
  // moving signaling NaNs, signed zero and arbitrary payloads is raw bit copying.
  initialView.setUint32(LOWER + 16, 0x7f800001, true);
  initialView.setUint32(LOWER + 20, 0x80000000, true);
  initialView.setUint32(LOWER + 24, 0xffffffff, true);
  if (c.tag === 'load_control' && c.address <= 65532) initialView.setUint32(c.address, c.control, true);
  const before = bytes.slice();
  const expected = before.slice();
  const view = new DataView(expected.buffer);
  const vector = register => Uint8Array.from([...before.slice(LOWER + register * 16, LOWER + (register + 1) * 16), ...before.slice(UPPER + register * 16, UPPER + (register + 1) * 16)]);
  const publish = value => {
    const result = new Uint8Array(32); result.set(value);
    expected.set(result.subarray(0,16), LOWER + c.destination * 16);
    expected.set(result.subarray(16), UPPER + c.destination * 16);
  };
  const [length, alignment] = requirement(c);
  assert.equal(c.memoryBytes, length, `${label}: architectural operand width`);
  assert.equal(c.alignment, alignment, `${label}: architectural alignment`);
  let status = c.invalidState ? 4 : length && c.address % alignment ? 2 : length && c.address > 65536 - length ? 3 : 0;
  if (!status && c.tag === 'load_control' && view.getUint32(c.address, true) > 0xffff) status = 1;
  if (!status) {
    const operand = before.subarray(c.address, c.address + length);
    switch (c.tag) {
      case 'copy': publish(vector(c.source).subarray(0,c.width)); break;
      case 'load': case 'load_scalar': publish(operand); break;
      case 'store': expected.set(vector(c.source).subarray(0,c.width),c.address); break;
      case 'store_scalar': expected.set(vector(c.source).subarray(0,c.element),c.address); break;
      case 'scalar': {
        const value = vector(c.merge).slice(0,16); value.set(vector(c.source).subarray(0,c.element)); publish(value); break;
      }
      case 'load_half': {
        const value = vector(c.merge).slice(0,16); value.set(operand,c.half); publish(value); break;
      }
      case 'store_half': expected.set(vector(c.source).subarray(c.half,c.half+8),c.address); break;
      case 'high_to_low': publish(Uint8Array.from([...vector(c.source).slice(8,16),...vector(c.merge).slice(8,16)])); break;
      case 'low_to_high': publish(Uint8Array.from([...vector(c.merge).slice(0,8),...vector(c.source).slice(0,8)])); break;
      case 'duplicate_single_low': case 'duplicate_single_high': case 'duplicate_double_low': {
        const source = c.source === 'memory' ? operand : vector(c.source);
        const value = new Uint8Array(c.width);
        for (let half=0; half<c.width; half+=16) {
          if (c.tag === 'duplicate_double_low') {
            value.set(source.subarray(half,half+8),half); value.set(source.subarray(half,half+8),half+8);
          } else {
            const first = c.tag === 'duplicate_single_high' ? 4 : 0;
            for (const [destination, offset] of [[0,first],[4,first],[8,first+8],[12,first+8]]) value.set(source.subarray(half+offset,half+offset+4),half+destination);
          }
        }
        publish(value); break;
      }
      case 'load_dword': {
        const value = c.source === 'memory' ? operand : before.subarray(GENERAL+c.source*4,GENERAL+c.source*4+4);
        publish(value); break;
      }
      case 'store_dword': {
        const value = vector(c.source).subarray(0,4);
        expected.set(value,c.destination === 'memory' ? c.address : GENERAL+c.destination*4); break;
      }
      case 'load_qword': publish(c.source === 'memory' ? operand : vector(c.source).subarray(0,8)); break;
      case 'store_qword': expected.set(vector(c.source).subarray(0,8),c.address); break;
      case 'mask': {
        const value = vector(c.source); let mask = 0;
        for (let lane=0; lane<c.width/c.element; lane++) if (value[(lane+1)*c.element-1]&128) mask |= 1<<lane;
        view.setUint32(GENERAL+c.destination*4,mask,true); break;
      }
      case 'load_control': view.setUint32(MXCSR,view.getUint32(c.address,true),true); break;
      case 'store_control': view.setUint32(c.address,view.getUint32(MXCSR,true),true); break;
      case 'zero_upper': expected.fill(0,UPPER,UPPER+128); break;
      case 'zero_all': expected.fill(0,UPPER,UPPER+128); expected.fill(0,LOWER,LOWER+128); break;
      default: throw new Error(`unhandled movement case ${label}`);
    }
  }
  const module = new WebAssembly.Module(Buffer.from(c.module,'hex'));
  assert.deepEqual(WebAssembly.Module.imports(module),[{module:'env',name:'memory',kind:'memory'}],label);
  assert.deepEqual(WebAssembly.Module.exports(module),[{name:'run',kind:'function'}],label);
  const instance = new WebAssembly.Instance(module,{env:{memory}});
  assert.equal(instance.exports.run(STATE,c.address),status,`${label}: result`);
  assert.deepEqual(bytes,expected,`${label}: all64KiB preserve unrelated bytes, complete destination and fault neutrality`);
}
console.log(`${cases.length} actual movement modules passed`);
