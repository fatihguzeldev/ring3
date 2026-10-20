export class CodeCache {
  constructor(engine, {budget = 50000, instructions = 2048, blocks = 128, capacity = 256} = {}) {
    for (const [name, value, maximum] of [['budget', budget, 0xffffffff], ['instructions', instructions, 16384], ['blocks', blocks, 1024], ['capacity', capacity, 256]]) {
      if (!Number.isInteger(value) || value < 1 || value > maximum) throw new Error(`invalid core ${name}`);
    }
    this.engine = engine;
    this.api = engine.api;
    this.memory = engine.memory;
    if (typeof SharedArrayBuffer !== 'undefined' && this.memory.buffer instanceof SharedArrayBuffer)
      throw new Error('core execution requires exclusively owned memory');
    this.native = Object.fromEntries(['core_prepare', 'core_code_ptr', 'core_code_len', 'core_context_ptr',
      'core_begin', 'core_end', 'core_import', 'core_export', 'core_read', 'core_write', 'core_numeric'].map(name => {
      if (typeof this.api[name] !== 'function') throw new Error(`missing core export ${name}`);
      return [name, this.api[name]];
    }));
    this.budget = budget; this.instructions = instructions; this.blocks = blocks; this.capacity = capacity;
    this.units = new Map(); this.context = 0; this.open = false; this.dirty = false; this.dirtyWork = 0; this.lastStop = 1;
    this.runs = 0; this.retired = 0; this.work = 0; this.admissions = 0; this.hits = 0; this.evictions = 0;
    this.generatedModuleBytes = 0; this.retainedModuleBytes = 0;
  }
  pristine() {
    if (this.engine.api !== this.api || this.engine.memory !== this.memory ||
        Object.entries(this.native).some(([name, value]) => this.api[name] !== value))
      throw new Error('core native bindings changed');
  }
  word(offset) { return new DataView(this.memory.buffer).getUint32(this.context + offset, true); }
  get pc() { return this.context ? this.word(32) : this.engine.get(48); }
  entry(pc = this.pc) {
    if (this.open) throw new Error('cannot select code during execution');
    this.pristine();
    this.engine.ok('core_prepare', pc, this.instructions, this.blocks);
    const context = this.native.core_context_ptr() >>> 0;
    if (!context || (this.context && context !== this.context)) throw new Error('core context allocation changed');
    this.context = context;
    const id = new DataView(this.memory.buffer).getBigUint64(context + 120, true);
    if (id === 0n) throw new Error('core code has no identity');
    if (this.units.has(id)) {
      const unit = this.units.get(id); this.units.delete(id); this.units.set(id, unit); this.hits++;
      return unit;
    }
    const pointer = this.native.core_code_ptr() >>> 0, length = this.native.core_code_len() >>> 0;
    if (!pointer || length < 8 || length > 16 * 1024 * 1024 || pointer + length > this.memory.buffer.byteLength)
      throw new Error('invalid core module descriptor');
    const encoded = new Uint8Array(this.memory.buffer).slice(pointer, pointer + length);
    const module = new WebAssembly.Module(encoded), bindings = {memory: this.memory};
    const available = {read: this.native.core_read, write: this.native.core_write, numeric: this.native.core_numeric};
    const found = new Set();
    for (const item of WebAssembly.Module.imports(module)) {
      if (item.module !== 'ring3' || found.has(item.name)) throw new Error(`invalid core import ${item.module}.${item.name}`);
      found.add(item.name);
      if (item.name === 'memory' && item.kind === 'memory') continue;
      if (item.kind !== 'function' || !Object.hasOwn(available, item.name)) throw new Error(`invalid core import ${item.name}`);
      bindings[item.name] = available[item.name];
    }
    if (!['memory', 'read', 'write'].every(name => found.has(name))) throw new Error('incomplete core imports');
    const run = new WebAssembly.Instance(module, {ring3: bindings}).exports.run;
    if (typeof run !== 'function' || run.length !== 2) throw new Error('invalid core run export');
    const unit = {id, pc, run, bytes: length};
    this.units.set(id, unit); this.admissions++; this.generatedModuleBytes += length; this.retainedModuleBytes += length;
    while (this.units.size > this.capacity || this.retainedModuleBytes > 32 * 1024 * 1024) {
      const [oldId, old] = this.units.entries().next().value;
      this.units.delete(oldId); this.retainedModuleBytes -= old.bytes; this.evictions++;
    }
    return unit;
  }
  execute(selected) {
    if (this.open) throw new Error('core execution is already active');
    this.pristine();
    if (this.dirtyWork > 0xffffffff - this.budget) this.publish();
    let unit = selected, status = this.native.core_begin();
    if (status === 4) { unit = this.entry(this.pc); status = this.native.core_begin(); }
    if (status !== 0) throw Object.assign(new Error(`core_begin returned ${status}`), {operation: 'core_begin', status});
    this.open = true;
    const before = this.word(40);
    let stop, failure;
    try { stop = unit.run(this.context, this.budget) >>> 0; }
    catch (error) { failure = error; throw error; }
    finally {
      const ended = this.native.core_end(); this.open = false;
      if (ended !== 0 && !failure) throw new Error(`core_end returned ${ended}`);
    }
    const retired = (this.word(40) - before) >>> 0, remaining = this.word(44), work = this.budget - remaining;
    if (remaining > this.budget || retired > work || stop < 1 || stop > 10 || stop === 8)
      throw new Error(`invalid core stop ${stop} at 0x${this.pc.toString(16)}`);
    this.runs++; this.retired += retired; this.work += work; this.dirtyWork += work; this.lastStop = stop; this.dirty = true;
    if (stop !== 1) this.publish();
    if (stop === 4) throw Object.assign(new Error(`unsupported instruction at 0x${this.pc.toString(16)}`), {pc:this.pc,reason:stop});
    return {pc: this.pc, coreReason: stop, reason: stop === 1 ? null : this.engine.get(72),
      detail: stop === 1 ? null : this.engine.get(80), retired, work, id: unit.id};
  }
  publish() {
    if (this.open) throw new Error('cannot publish an active core');
    if (!this.context || !this.dirty) return;
    this.engine.ok('core_export', this.lastStop, this.dirtyWork); this.dirty = false; this.dirtyWork = 0;
  }
  import() {
    if (this.open) throw new Error('cannot import an active core');
    if (!this.context) return;
    if (this.dirty) throw new Error('unpublished core state before native import');
    this.engine.ok('core_import');
  }
  sampleUsage() { return this.stats(); }
  stats() {
    return {backend: 'core', runs: this.runs, retired: this.retired, work: this.work, admissions: this.admissions,
      engineCacheHits: this.hits, evictions: this.evictions, generatedModuleBytes: this.generatedModuleBytes,
      retainedModuleBytes: this.retainedModuleBytes, installedUnits: this.units.size, unitCapacity: this.capacity,
      wasmMemoryBytes: this.memory.buffer.byteLength};
  }
}
