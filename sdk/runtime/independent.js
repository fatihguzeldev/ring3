import {KEY} from './engine.js';

const STATE_BYTES = 160;
const MAX_CODE_BYTES = 128 * 1024 * 1024;
const REQUIRED = ['cpu_abi', 'cpu_context_ptr', 'cpu_begin', 'cpu_prepare', 'cpu_unit_slot',
  'cpu_code_ptr', 'cpu_code_len', 'cpu_dispatch_code_ptr', 'cpu_dispatch_code_len',
  'cpu_runtime_read', 'cpu_runtime_write', 'cpu_instruction', 'cpu_publish', 'cpu_end'];
const STATUS = Object.freeze({needCode: 1, budget: 2, memoryFault: 3, unsupported: 4,
  gate: 5, invalidated: 6, instruction: 7, infrastructure: 8, cpuFault: 9, systemEntry: 10});

export class IndependentCodeCache {
  constructor(engine, {budget = 50000, instructions = 2048} = {}) {
    for (const [name, value, maximum] of [['budget', budget, 0xffffffff], ['instructions', instructions, 4096]])
      if (!Number.isInteger(value) || value < 1 || value > maximum)
        throw new TypeError(`invalid independent CPU ${name}`);
    this.engine = engine;
    this.api = engine.api;
    this.memory = engine.memory;
    if (!(this.memory instanceof WebAssembly.Memory) ||
        (typeof SharedArrayBuffer !== 'undefined' && this.memory.buffer instanceof SharedArrayBuffer))
      throw new TypeError('independent CPU requires exclusively owned Wasm memory');
    for (const name of REQUIRED)
      if (typeof this.api[name] !== 'function') throw new Error(`missing independent CPU export ${name}`);
    if (this.api.cpu_abi() !== 1) throw new Error('unsupported independent CPU ABI');
    this.budget = budget;
    this.instructions = instructions;
    this.context = 0;
    this.imported = false;
    this.open = false;
    this.closed = false;
    this.failure = null;
    this.terminalSelection = null;
    this.units = new Map();
    this.table = new WebAssembly.Table({element: 'anyfunc', initial: 1});
    this.runs = 0;
    this.retired = 0;
    this.work = 0;
    this.generatedModuleBytes = 0;
    this.retainedModuleBytes = 0;
    this.evictions = 0;
    this.instructionCalls = 0;
    const bytes = this.bytes(this.api.cpu_dispatch_code_ptr(), this.api.cpu_dispatch_code_len());
    const module = new WebAssembly.Module(bytes);
    for (const item of WebAssembly.Module.imports(module))
      if (!(item.module === 'env' &&
          (item.name === 'memory' && item.kind === 'memory' || item.name === 'functions' && item.kind === 'table')))
        throw new Error(`invalid independent CPU dispatcher import ${item.module}.${item.name}`);
    const instance = new WebAssembly.Instance(module, {env: {memory: this.memory, functions: this.table}});
    if (typeof instance.exports.run !== 'function' || instance.exports.run.length !== 1)
      throw new Error('invalid independent CPU dispatcher entry');
    this.dispatch = instance.exports.run;
  }
  ready(checkSignal = true) {
    if (this.failure) throw this.failure;
    if (this.closed) throw new Error('independent CPU is closed');
    if (checkSignal && this.engine.signal?.aborted) {
      if (this.imported && !this.open) this.publish();
      throw this.fail(this.engine.signal.reason ?? new Error('independent CPU execution aborted'));
    }
  }
  bytes(pointer, length) {
    pointer >>>= 0;
    length >>>= 0;
    if (!pointer || !length || length > MAX_CODE_BYTES || pointer + length > this.memory.buffer.byteLength)
      throw new Error('invalid independent CPU code allocation');
    return new Uint8Array(this.memory.buffer, pointer, length);
  }
  state() {
    if (!this.context || this.context % 8 || this.context + STATE_BYTES > this.memory.buffer.byteLength)
      throw new Error('invalid independent CPU state allocation');
    const state = new DataView(this.memory.buffer, this.context, STATE_BYTES);
    if (state.getUint32(0, true) !== 1 || state.getUint32(4, true) !== STATE_BYTES)
      throw new Error('invalid independent CPU state format');
    return state;
  }
  fail(error) {
    this.failure ??= error;
    if (this.imported) {
      try {
        const status = this.api.cpu_end(...KEY);
        if (status !== 0) this.failure.cleanupFailure = `cpu_end returned ${status}`;
      } catch (cleanup) {this.failure.cleanupFailure = cleanup.message;}
    }
    this.imported = false;
    this.open = false;
    return this.failure;
  }
  get pc() {
    return this.imported ? this.state().getUint32(40, true) : this.engine.get(48);
  }
  drainVictims() {
    if (typeof this.api.cpu_cache_victim !== 'function') return;
    let slot = this.api.cpu_cache_victim() >>> 0;
    if (!slot) return;
    const seen = new Set();
    do {
      if (seen.has(slot)) throw new Error('duplicate independent CPU cache victim');
      seen.add(slot);
      const unit = this.units.get(slot);
      if (unit) {
        this.table.set(slot, null);
        this.units.delete(slot);
        this.retainedModuleBytes -= unit.bytes;
        this.evictions++;
      }
      slot = this.api.cpu_cache_victim() >>> 0;
    } while (slot);
  }
  import() {
    this.ready();
    if (this.imported) return;
    try {
      const status = this.api.cpu_begin(...KEY);
      if (status !== 0) throw new Error(`cpu_begin returned ${status}`);
      this.imported = true;
      this.context = this.api.cpu_context_ptr() >>> 0;
      this.state().setUint32(48, this.budget, true);
      this.drainVictims();
    } catch (error) {throw this.fail(error);}
  }
  select() {
    this.ready();
    this.import();
    try {
      this.drainVictims();
      const pc = this.pc, status = this.api.cpu_prepare(...KEY, pc, this.instructions);
      this.drainVictims();
      this.terminalSelection = null;
      if (status === 20) {
        const stop = this.state().getUint32(52, true);
        if (![STATUS.memoryFault, STATUS.unsupported, STATUS.cpuFault].includes(stop))
          throw new Error(`invalid independent CPU prepare stop ${stop}`);
        return this.terminalSelection = Object.freeze({terminal: true, pc});
      }
      if (status !== 0) throw new Error(`cpu_prepare returned ${status} at 0x${pc.toString(16)}`);
      const slot = this.api.cpu_unit_slot() >>> 0;
      if (!slot) throw new Error('invalid independent CPU unit slot');
      const retained = this.units.get(slot);
      if (retained) return retained;
      const bytes = this.bytes(this.api.cpu_code_ptr(), this.api.cpu_code_len());
      if (this.retainedModuleBytes + bytes.length > MAX_CODE_BYTES)
        throw new Error('independent CPU resident code budget exceeded');
      const module = new WebAssembly.Module(bytes);
      for (const item of WebAssembly.Module.imports(module))
        if (!(item.module === 'env' && item.name === 'memory' && item.kind === 'memory') &&
            !(item.module === 'cpu' && ['read', 'write'].includes(item.name) && item.kind === 'function'))
          throw new Error(`invalid independent CPU unit import ${item.module}.${item.name}`);
      const instance = new WebAssembly.Instance(module, {env: {memory: this.memory},
        cpu: {read: this.api.cpu_runtime_read, write: this.api.cpu_runtime_write}});
      if (typeof instance.exports.run !== 'function' || instance.exports.run.length !== 1)
        throw new Error('invalid independent CPU unit entry');
      if (slot >= this.table.length) this.table.grow(slot + 1 - this.table.length);
      this.table.set(slot, instance.exports.run);
      const unit = {slot, pc, bytes: bytes.length};
      this.units.set(slot, unit);
      this.generatedModuleBytes += bytes.length;
      this.retainedModuleBytes += bytes.length;
      return unit;
    } catch (error) {throw this.fail(error);}
  }
  execute(unit) {
    this.ready();
    try {this.drainVictims();} catch (error) {throw this.fail(error);}
    const terminal = unit != null && unit === this.terminalSelection;
    if (this.open || !this.imported || !unit || !terminal && this.units.get(unit.slot) !== unit)
      throw new Error('independent CPU unit is not selected by this owner');
    let state = this.state();
    if (!state.getUint32(48, true)) state.setUint32(48, this.budget, true);
    const before = state.getUint32(56, true), remaining = state.getUint32(48, true);
    let stop = terminal ? state.getUint32(52, true) : undefined;
    this.open = true;
    try {
      if (!terminal) do {
        stop = this.dispatch(this.context) >>> 0;
        if (stop !== STATUS.instruction) break;
        const instructionRemaining = this.state().getUint32(48, true);
        const status = this.api.cpu_instruction(...KEY);
        this.instructionCalls++;
        if (status !== 0) throw new Error(`cpu_instruction returned ${status}`);
        state = this.state();
        stop = state.getUint32(52, true);
        if (stop === 0 && state.getUint32(48, true) >= instructionRemaining)
          throw new Error('independent CPU instruction made no bounded progress');
      } while (stop === 0);
      state = this.state();
      if (!Number.isInteger(stop) || stop < 1 || stop > STATUS.systemEntry || stop === STATUS.instruction ||
          stop === STATUS.infrastructure || state.getUint32(52, true) !== stop)
        throw new Error(`invalid independent CPU stop ${stop}`);
      const work = remaining - state.getUint32(48, true), retired = (state.getUint32(56, true) - before) >>> 0;
      if (work < 0 || retired > work) throw new Error('invalid independent CPU progress');
      if (stop === STATUS.needCode && !work && this.pc === unit.pc)
        throw new Error('independent CPU dispatcher did not enter its selected unit');
      this.runs++;
      this.work += work;
      this.retired += retired;
      this.open = false;
      const pc = this.pc;
      if (stop !== STATUS.needCode) this.publish();
      if (stop === STATUS.unsupported)
        throw Object.assign(new Error(`unsupported independent CPU instruction at 0x${pc.toString(16)}`), {pc, reason: 4});
      return {pc, coreReason: stop === STATUS.gate ? 5 : stop,
        reason: stop === STATUS.needCode ? null : this.engine.get(72),
        detail: stop === STATUS.needCode ? null : this.engine.get(80), retired, work,
        kind: terminal ? 'boundary' : 'compiled', id: terminal ? null : unit.slot};
    } catch (error) {throw this.fail(error);}
    finally {this.open = false;}
  }
  publish() {
    this.ready(false);
    if (this.open) throw new Error('cannot publish active independent CPU execution');
    if (!this.imported) return;
    try {
      const status = this.api.cpu_publish(...KEY);
      if (status !== 0) throw new Error(`cpu_publish returned ${status}`);
      this.imported = false;
    } catch (error) {throw this.fail(error);}
  }
  sampleUsage() {return this.stats();}
  coldDiagnostics() {return null;}
  stats() {
    return {backend: 'independent', fpuMode: 'strict', runs: this.runs, retired: this.retired, work: this.work,
      installedUnits: this.units.size, generatedModuleBytes: this.generatedModuleBytes,
      retainedModuleBytes: this.retainedModuleBytes, evictions: this.evictions,
      instructionCalls: this.instructionCalls};
  }
  close() {
    if (this.closed) return;
    if (this.open) throw new Error('cannot close active independent CPU execution');
    try {
      if (this.imported && !this.failure) this.publish();
    } finally {
      for (const slot of this.units.keys()) this.table.set(slot, null);
      this.units.clear();
      this.retainedModuleBytes = 0;
      this.closed = true;
    }
  }
}
