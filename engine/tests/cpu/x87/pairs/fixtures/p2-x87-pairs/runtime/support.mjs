import assert from 'node:assert/strict';
import {createArena, ARENA_SIZE} from '../../../../../../support/wasm/arena.mjs';
import {consumeCpuEvent} from '../../../../../../support/wasm/events.mjs';

export const PC = 0x1000, FP = 4236, RAM = 0x4000;
const key = [0x58503837, 0x50414952];
const pair = value => [Number(value & 0xffffffffn), Number(value >> 32n)];
export const envelope = (status, completed, reported = 0) =>
  BigInt(status >>> 0) | BigInt(completed) << 32n | BigInt(reported) << 34n;
export function packet(length, flags = 0x202) {
  const bytes = Buffer.alloc(40);
  bytes.write('R3MH'); bytes.writeUInt32LE(0x10007, 4); bytes.writeUInt32LE(40, 8);
  bytes.writeUInt32LE(flags >>> 0, 20); bytes.writeUInt32LE(length, 36);
  return bytes;
}
export function ext(value) {
  const bytes = Buffer.alloc(10);
  const negative = value < 0;
  value = Math.abs(value);
  if (value !== 0) {
    const power = Math.floor(Math.log2(value));
    bytes.writeBigUInt64LE(BigInt(value) << BigInt(63 - power));
    bytes.writeUInt16LE(0x3fff + power | (negative ? 0x8000 : 0), 8);
  }
  return bytes;
}

export class Fixture {
  constructor(module, program, {resident = true, scalar = false} = {}) {
    const instance = new WebAssembly.Instance(module, {ring3_observability: {now_ns: () => 0n}});
    this.api = Object.fromEntries(Object.entries(instance.exports)
      .filter(([name]) => name.startsWith('ring3_abi_v1_'))
      .map(([name, value]) => [name.slice(13), value]));
    this.memory = instance.exports.memory;
    this.program = program; this.resident = resident; this.scalar = scalar; this.calls = [];
    this.ok('open', 8, ...key); this.base = this.api.arena_ptr() >>> 0;
    this.ok('map', PC, 1, 7); this.ok('map', RAM, 1, 3);
    this.data = Buffer.alloc(4096, 0x5a);
    this.data.writeFloatLE(3, 0); this.data.writeFloatLE(5, 4);
    this.put(140, this.data); this.ok('upload', RAM, this.data.length);
    this.code = Buffer.alloc(4096); this.code.set(program.code);
    this.put(140, Buffer.from(program.code)); this.ok('upload', PC, program.code.length);
    const specs = Buffer.alloc(8); specs.writeUInt32LE(PC); specs.writeUInt32LE(program.code.length, 4);
    this.put(140, specs); this.ok(resident ? 'compile_resident' : 'compile', 1);
    const descriptor = this.arena();
    this.id = resident ? descriptor.readBigUInt64LE(148) : 0n;
    const pointer = resident ? descriptor.readUInt32LE(156) : this.api.module_ptr() >>> 0;
    const length = resident ? descriptor.readUInt32LE(160) : this.api.module_len() >>> 0;
    this.bytes = Buffer.from(new Uint8Array(this.memory.buffer, pointer, length));
    const generated = new WebAssembly.Module(this.bytes);
    this.imports = WebAssembly.Module.imports(generated);
    const bindings = {...this.api};
    for (const name of ['x87_resident_op', 'x87_op']) {
      bindings[name] = (...args) => {
        this.calls.push({name, args: args.map(value => value >>> 0)});
        return this.api[name](...args);
      };
    }
    bindings.x87_resident_pair = (...args) => {
      const row = {name: 'x87_resident_pair', args: args.map(value => value >>> 0)};
      this.calls.push(row);
      let result;
      if (this.synthetic) {
        const reply = this.synthetic(args, this);
        if (reply.packet) this.put(100, reply.packet);
        result = reply.envelope;
      } else if (this.scalar) {
        // use the real pre-pair scalar helper, then the unchanged second instruction body.
        const step = this.program.steps[0];
        const status = this.api.x87_resident_op(...args.slice(0, 4), step.word,
          args[5], args[7], args[4], step.opcode, step.control);
        result = envelope(status, status === 0 && this.arena().readUInt32LE(116) === 0 ? 1 : 0);
      } else {
        result = this.api.x87_resident_pair(...args.slice(0, 8), this.forceOne ? 1 : args[8]);
      }
      if (this.cancelAfterFirst) this.put(96, Buffer.from([1, 0, 0, 0]));
      row.result = result.toString();
      return result;
    };
    this.run = new WebAssembly.Instance(generated, {env: {memory: this.memory}, ring3: bindings}).exports.run;
    if (resident) {
      this.table = new WebAssembly.Table({element: 'anyfunc', initial: 1, maximum: 1});
      this.table.set(0, this.run);
      this.ok('acknowledge_resident_installation', ...key, ...pair(this.id), 0);
    }
  }
  ok(name, ...args) { assert.equal(this.api[name](...args), 0, name); }
  put(offset, bytes) { new Uint8Array(this.memory.buffer).set(bytes, this.base + offset); }
  arena() { return Buffer.from(new Uint8Array(this.memory.buffer, this.base, ARENA_SIZE)); }
  seed(row = {}) {
    const bytes = createArena();
    bytes.writeUInt32LE(0x10003, 4); bytes.writeUInt32LE(row.shadow ?? 0, 12);
    [0x11223344, 0x22334455, 0x1100, RAM, RAM + 0x800, 0x55667788, 0x66778899,
      row.secondAddress ?? RAM + 4].forEach((value, index) => bytes.writeUInt32LE(value >>> 0, 16 + index * 4));
    bytes.writeUInt32LE(row.pc ?? PC, 48); bytes.writeUInt32LE(row.flags ?? 0x202, 52);
    bytes.writeUInt32LE(row.cancel ?? 0, 96);
    bytes.writeUInt16LE(row.control ?? 0x37f, FP + 16);
    bytes.writeUInt16LE(row.status ?? 0, FP + 18);
    bytes.writeUInt16LE(0xfffc, FP + 20); bytes.set(ext(1), FP + 40);
    if (row.nullDs) bytes.writeUInt16LE(0, 4546);
    if (row.zeroSecondStack) {
      bytes.writeUInt16LE(0xfff4, FP + 20); bytes.set(ext(0), FP + 50);
    }
    if (row.csLimit !== undefined) {
      bytes.writeUInt16LE(2, 4528);
      [0xf33fffff, 0xf2b00000 | row.csLimit, 0xf33fffff, 0xf33fffff, 0xf33fffff, 0]
        .forEach((value, index) => bytes.writeUInt32LE(value >>> 0, 4524 + [28, 32, 36, 40, 44, 56][index]));
    }
    this.put(0, bytes);
    this.calls.length = 0;
    return bytes;
  }
  execute(budget, {receipt = this.resident, consume = true} = {}) {
    let token;
    if (receipt) {
      this.ok('begin_execution_receipt', ...key, budget, ...pair(this.id), 0);
      token = this.arena().readBigUInt64LE(156);
      assert.ok(token > 0n);
    }
    const status = this.run(this.base, this.base + 56, budget, this.base + 96);
    const stopped = this.arena();
    let returned;
    if (receipt && status === 0 && stopped.readUInt32LE(72) !== 7) {
      this.ok('finish_execution_receipt', ...key, ...pair(token));
      const bytes = this.arena().subarray(140, 204);
      assert.deepEqual(bytes.subarray(0, 16), Buffer.from('52334552010001004000000000000000', 'hex'));
      assert.deepEqual(bytes.subarray(52), Buffer.alloc(12));
      returned = {epoch: bytes.readBigUInt64LE(16), owner: bytes.readBigUInt64LE(24),
        slot: bytes.readUInt32LE(32), retired: bytes.readUInt32LE(36), child: bytes.readUInt32LE(40),
        reason: bytes.readUInt32LE(44), kind: bytes.readUInt32LE(48)};
      assert.equal(returned.owner, this.id); assert.equal(returned.slot, 0);
      assert.equal(returned.retired, stopped.readUInt32LE(76));
      assert.equal(returned.child, returned.retired); assert.equal(returned.reason, stopped.readUInt32LE(72));
      assert.notEqual(this.api.finish_execution_receipt(...key, ...pair(token)), 0, 'receipt cannot replay');
    }
    if (status === 0 && consume) consumeCpuEvent(this, ARENA_SIZE);
    const cr2Status = this.api.read_cr2();
    const cr2 = cr2Status === 0 ? this.arena().readUInt32LE(140) : null;
    return {status, arena: stopped, receipt: returned, token, cr2, calls: this.calls.slice()};
  }
  checkMemory() {
    for (const [address, expected] of [[PC, this.code], [RAM, this.data]]) {
      const actual = Buffer.alloc(expected.length);
      const view = new DataView(this.memory.buffer);
      for (let offset = 0; offset < actual.length; offset += 4) {
        this.ok('read32', address + offset);
        actual.writeUInt32LE(view.getUint32(this.base + 120, true), offset);
      }
      assert.deepEqual(actual, expected, 'read/register pairs do not mutate any mapped guest byte');
    }
  }
  close() { this.ok('close'); }
}

export const defined = arena => Buffer.concat([arena.subarray(0, 140), arena.subarray(4236)]);
