const MAX_CODE_BYTES = 32 * 1024 * 1024;

export class CodeCache {
  constructor(engine, {budget = 50000, instructions = 2048, blocks = 128, capacity = 1024, chain = true} = {}) {
    if (typeof chain !== 'boolean') throw new Error('invalid core chain option');
    for (const [name, value, maximum] of [['budget', budget, 0xffffffff], ['instructions', instructions, 16384], ['blocks', blocks, 1024], ['capacity', capacity, 1024]]) {
      if (!Number.isInteger(value) || value < 1 || value > maximum) throw new Error(`invalid core ${name}`);
    }
    this.engine = engine;
    this.api = engine.api;
    this.memory = engine.memory;
    if (typeof SharedArrayBuffer !== 'undefined' && this.memory.buffer instanceof SharedArrayBuffer)
      throw new Error('core execution requires exclusively owned memory');
    this.native = Object.fromEntries(['core_prepare', 'core_code_ptr', 'core_code_len', 'core_context_ptr',
      'core_begin', 'core_end', 'core_import', 'core_export', 'core_read', 'core_write', 'core_numeric',
      ...(chain ? ['core_dispatch_code_ptr','core_dispatch_code_len','core_link','core_unlink','core_chain_next'] : [])].map(name => {
      if (typeof this.api[name] !== 'function') throw new Error(`missing core export ${name}`);
      return [name, this.api[name]];
    }));
    this.budget = budget; this.instructions = instructions; this.blocks = blocks; this.capacity = capacity;
    this.units = new Map(); this.context = 0; this.open = false; this.dirty = false; this.dirtyWork = 0; this.lastStop = 1;
    this.runs = 0; this.regions = 0; this.retired = 0; this.work = 0; this.admissions = 0; this.hits = 0; this.evictions = 0;
    this.evictionReasons = {count:0,bytes:0};
    this.generatedModuleBytes = 0; this.retainedModuleBytes = 0;
    this.chain = chain;
    if (chain) this.initializeDispatch();
  }
  encoded(pointerName, lengthName) {
    const pointer = this.native[pointerName]() >>> 0, length = this.native[lengthName]() >>> 0;
    if (!pointer || length < 8 || length > 16 * 1024 * 1024 || pointer + length > this.memory.buffer.byteLength)
      throw new Error('invalid core module descriptor');
    return new Uint8Array(this.memory.buffer).slice(pointer, pointer + length);
  }
  initializeDispatch() {
    this.table = new WebAssembly.Table({element:'anyfunc',initial:4097,maximum:4097});
    this.freeSlots = Array.from({length:4096},(_,index) => 4096-index);
    const module = new WebAssembly.Module(this.encoded('core_dispatch_code_ptr','core_dispatch_code_len'));
    const imports = WebAssembly.Module.imports(module);
    const expected = new Map([['memory','memory'],['functions','table'],['next','function']]);
    if (imports.length !== expected.size || imports.some(item => item.module !== 'ring3' || expected.get(item.name) !== item.kind) ||
        new Set(imports.map(item => item.name)).size !== expected.size) throw new Error('invalid core dispatch imports');
    const exports = new WebAssembly.Instance(module,{ring3:{memory:this.memory,functions:this.table,next:this.native.core_chain_next}}).exports;
    if (typeof exports.run !== 'function' || exports.run.length !== 3 || !(exports.regions instanceof WebAssembly.Global) || exports.regions.value !== 0)
      throw new Error('invalid core dispatch exports');
    this.dispatch = exports;
  }
  evict(reason) {
    const [id,unit] = this.units.entries().next().value;
    if (this.chain) {
      const status = this.native.core_unlink(unit.slot,Number(id&0xffffffffn),Number(id>>32n));
      if (status !== 0) throw new Error(`core_unlink returned ${status}`);
      this.table.set(unit.slot,null); this.freeSlots.push(unit.slot);
    }
    this.units.delete(id); this.retainedModuleBytes -= unit.bytes; this.evictions++; this.evictionReasons[reason]++;
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
    const encoded = this.encoded('core_code_ptr','core_code_len'), length = encoded.length;
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
    while (this.units.size >= this.capacity || this.retainedModuleBytes + length > MAX_CODE_BYTES)
      this.evict(this.retainedModuleBytes + length > MAX_CODE_BYTES ? 'bytes' : 'count');
    if (this.chain) {
      const slot = this.freeSlots.pop();
      if (!slot || this.table.get(slot) !== null) throw new Error('invalid free core table slot');
      this.table.set(slot,run);
      const status = this.native.core_link(slot);
      if (status !== 0) {this.table.set(slot,null); this.freeSlots.push(slot); throw new Error(`core_link returned ${status}`);}
      unit.slot = slot;
    }
    this.units.set(id, unit); this.admissions++; this.generatedModuleBytes += length; this.retainedModuleBytes += length;
    return unit;
  }
  execute(selected) {
    if (this.open) throw new Error('core execution is already active');
    this.pristine();
    if (!selected || this.units.get(selected.id) !== selected) throw new Error('core region is not retained by this cache');
    if (this.dirtyWork > 0xffffffff - this.budget) this.publish();
    let unit = selected, status = this.native.core_begin();
    if (status === 4) { unit = this.entry(this.pc); status = this.native.core_begin(); }
    if (status !== 0) throw Object.assign(new Error(`core_begin returned ${status}`), {operation: 'core_begin', status});
    this.open = true;
    const before = this.word(40);
    let stop, failure;
    let regions;
    try {
      if (this.chain) {
        if (this.table.get(unit.slot) !== unit.run) throw new Error('core table binding changed');
        stop = this.dispatch.run(this.context,this.budget,unit.slot) >>> 0;
        regions = this.dispatch.regions.value;
      } else {stop = unit.run(this.context, this.budget) >>> 0; regions = 1;}
    }
    catch (error) { failure = error; throw error; }
    finally {
      const ended = this.native.core_end(); this.open = false;
      if (ended !== 0 && !failure) throw new Error(`core_end returned ${ended}`);
    }
    const retired = (this.word(40) - before) >>> 0, remaining = this.word(44), work = this.budget - remaining;
    if (remaining > this.budget || retired > work || stop < 1 || stop > 10 || stop === 8 || !Number.isInteger(regions) || regions < 1 || regions > work+1)
      throw new Error(`invalid core stop ${stop} at 0x${this.pc.toString(16)}`);
    const id = new DataView(this.memory.buffer).getBigUint64(this.context+120,true);
    if (!this.units.has(id)) throw new Error('core dispatch selected an unretained region');
    this.runs++; this.regions += regions; this.retired += retired; this.work += work; this.dirtyWork += work; this.lastStop = stop; this.dirty = true;
    if (stop !== 1) this.publish();
    if (stop === 4) throw Object.assign(new Error(`unsupported instruction at 0x${this.pc.toString(16)}`), {pc:this.pc,reason:stop});
    return {pc: this.pc, coreReason: stop, reason: stop === 1 ? null : this.engine.get(72),
      detail: stop === 1 ? null : this.engine.get(80), retired, work, id, regions};
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
    return {backend: 'core', chainEnabled:this.chain, runs: this.runs, regions:this.regions, retired: this.retired, work: this.work, admissions: this.admissions,
      engineCacheHits: this.hits, evictions: this.evictions, generatedModuleBytes: this.generatedModuleBytes,
      retainedModuleBytes: this.retainedModuleBytes, installedUnits: this.units.size, unitCapacity: this.capacity,
      byteCapacity:MAX_CODE_BYTES, evictionReasons:{...this.evictionReasons},
      wasmMemoryBytes: this.memory.buffer.byteLength};
  }
}
