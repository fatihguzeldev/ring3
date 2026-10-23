const MAX_CODE_BYTES = 128 * 1024 * 1024;
const LINK_LOCALS = [...Array.from({length:12},(_,index)=>index+2),
  ...Array.from({length:6},(_,index)=>index+20),99];
const LINK_GLOBALS = ['link.mode','link.valid','link.context',...LINK_LOCALS.map(local=>`link.local${local}`),'trace.arena'];
const SUPPORT_FUNCTIONS = [
  ['support.flags',8],['support.materialize_numeric',1],['support.resolve_status',5],
  ['support.translate_page',3],['support.acquire_group',9],['support.store8',7],
  ['support.store16',7],['support.store32',7],['support.commit_pool_cold',42],['support.commit_native',5],['support.commit_raw',4],
  ['support.record_trace',6],
];
const SUPPORT_IMPORTS = [...SUPPORT_FUNCTIONS.map(([name])=>name),'support.abi'];
const NATIVE_COUNTERS = ['compiled','admittedEncodedBytes','countEvictions','byteEvictions',
  'sourceInvalidations','retainedUnits','retainedEncodedBytes','unitCapacity'];
const COLD_COUNTERS = ['sourceResets','requestedResets','occupiedLiveReplacements','nativeRetired',
  'hotRequests','decodeFallbackRequests','admissionFallbackRequests','unsupportedOperationRequests',
  'rowsVisits1','rowsVisits2To63','rowsVisits64Plus','maximumVisits'];
const DIAGNOSTIC_PC_CAPACITY = 8192;
const DIAGNOSTIC_LAST_CAPACITY = 32;
const DIAGNOSTIC_SLOW_CAPACITY = 8;
const repeatOrder = (a,b) => b.admissions-a.admissions || b.bytes-a.bytes || a.pc-b.pc;
const slowOrder = (a,b) => b.phaseTotalNs-a.phaseTotalNs || a.sequence-b.sequence;
const compilationPc = (pc,prepareHistoryPartial = false) => ({
  pc,prepares:0,hits:0,prepareNs:0,prepareFailures:0,admissions:0,bytes:0,
  identityChanges:0,repeatedIdentityAdmissions:0,lastIdentity:null,firstSequence:0,lastSequence:0,
  sourceWitnesses:0,lastSourceVersion:null,sameSourceVersionAdmissions:0,unknownSourceAdmissions:0,
  prepareHistoryPartial,
});

export class CodeCache {
  #bindings;
  #contextView;
  #linkGlobals;
  #support;

  constructor(engine, {budget = 50000, instructions = 2048, blocks = 128, capacity = 4096, chain = true} = {}) {
    if (typeof chain !== 'boolean') throw new Error('invalid core chain option');
    for (const [name, value, maximum] of [['budget', budget, 0xffffffff], ['instructions', instructions, 16384], ['blocks', blocks, 1024], ['capacity', capacity, 4096]]) {
      if (!Number.isInteger(value) || value < 1 || value > maximum) throw new Error(`invalid core ${name}`);
    }
    this.engine = engine;
    this.api = engine.api;
    this.memory = engine.memory;
    if (typeof SharedArrayBuffer !== 'undefined' && this.memory.buffer instanceof SharedArrayBuffer)
      throw new Error('core execution requires exclusively owned memory');
    const linkExports = ['core_chain_select','core_chain_cold','core_trace_pointer'];
    const linked = linkExports.map(name=>typeof this.api[name] === 'function');
    if (linked.some(Boolean) && !linked.every(Boolean)) throw new Error('incomplete core link exports');
    this.linkedCore = linked.every(Boolean);
    this.native = Object.fromEntries(['core_prepare', 'core_code_ptr', 'core_code_len', 'core_context_ptr',
      'core_begin', 'core_end', 'core_import', 'core_export', 'core_read', 'core_write', 'core_numeric',
      ...(chain || this.linkedCore ? ['core_dispatch_code_ptr','core_dispatch_code_len'] : []),
      ...(chain ? ['core_link','core_unlink','core_chain_next',
        'core_link_owners_ptr','core_cache_victim','core_cache_status_ptr'] : []),
      ...(this.linkedCore ? linkExports : [])].map(name => {
      if (typeof this.api[name] !== 'function') throw new Error(`missing core export ${name}`);
      return [name, this.api[name]];
    }));
    if (typeof this.api.core_cold_run === 'function') this.native.core_cold_run = this.api.core_cold_run;
    if (typeof this.api.core_lookup === 'function') this.native.core_lookup = this.api.core_lookup;
    if (typeof this.api.core_cold_status_ptr === 'function') this.native.core_cold_status_ptr = this.api.core_cold_status_ptr;
    for (const name of ['core_trace_ready','core_trace_root','core_trace_finish'])
      if (typeof this.api[name] === 'function') this.native[name] = this.api[name];
    const traceExports = ['core_trace_ready','core_trace_root','core_trace_finish'].map(name=>typeof this.native[name] === 'function');
    if (traceExports.some(Boolean) && !traceExports.every(Boolean)) throw new Error('incomplete core trace exports');
    this.#bindings = Object.entries(this.native);
    this.budget = budget; this.instructions = instructions; this.blocks = blocks; this.capacity = capacity;
    this.units = new Map(); this.context = 0; this.open = false; this.dirty = false; this.dirtyWork = 0; this.lastStop = 1;
    this.runs = 0; this.regions = 0; this.retired = 0; this.work = 0; this.admissions = 0; this.hits = 0; this.evictions = 0;
    this.coldRuns = 0; this.coldRetired = 0; this.coldWork = 0; this.jitPc = null;
    this.transient = null;
    this.transientAdmissions = 0; this.transientRuns = 0; this.transientModuleBytes = 0;
    this.chainedColdRuns = 0; this.chainedColdWork = 0; this.chainedColdRetired = 0;
    this.evictionReasons = {count:0,bytes:0};
    this.prunedNative = 0; this.prunedNativeBytes = 0; this.nativeCache = undefined;
    this.generatedModuleBytes = 0; this.retainedModuleBytes = 0;
    this.compilation = engine.cacheMeasurementsEnabled ? {
      prepare:{count:0,failures:0,ns:0,maxNs:0,lastNs:0},
      module:{count:0,failures:0,ns:0,maxNs:0,lastNs:0},
      instance:{count:0,failures:0,ns:0,maxNs:0,lastNs:0},
      admissionCount:0,admittedBytes:0,prepareHits:0,
      trackedRepeatPcAdmissions:0,samePcNewIdentityAdmissions:0,samePcSameIdentityAdmissions:0,
      sourceWitnessedAdmissions:0,sourceWitnessPendingAdmissions:0,sourceWitnessRemovedAdmissions:0,
      samePcNewIdentitySameSourceVersionAdmissions:0,samePcNewIdentityUnknownSourceAdmissions:0,
      droppedPcSourceWitnesses:0,
      reclaimedPrepareOnlyPcs:0,reclaimedPrepareOnlyPrepares:0,reclaimedPrepareOnlyHits:0,
      droppedPcPrepares:0,droppedPcAdmissions:0,pcs:new Map(),prepareOnly:new Set(),topRepeat:[],last:[],slow:[],
    } : null;
    this.chain = chain;
    if (chain || this.linkedCore) this.initializeDispatch();
  }
  measureCompilation(phase,operation,pc) {
    const diagnostic = this.compilation, metric = diagnostic[phase];
    let row;
    if (phase === 'prepare') {
      pc >>>= 0;
      row = diagnostic.pcs.get(pc);
      if (!row && diagnostic.pcs.size < DIAGNOSTIC_PC_CAPACITY) {
        row = compilationPc(pc);
        diagnostic.pcs.set(pc,row);
        diagnostic.prepareOnly.add(pc);
      }
      if (row) row.prepares++;
      else diagnostic.droppedPcPrepares++;
    }
    metric.count++;
    const start = performance.now();
    try {return operation();}
    catch (error) {metric.failures++; if (row) row.prepareFailures++; throw error;}
    finally {
      const elapsed = Math.max(0,Math.round((performance.now()-start)*1e6));
      metric.ns += elapsed; metric.maxNs = Math.max(metric.maxNs,elapsed); metric.lastNs = elapsed;
      if (row) row.prepareNs += elapsed;
    }
  }
  recordAdmission(pc,id,bytes) {
    const diagnostic = this.compilation, identity = id.toString();
    const sequence = ++diagnostic.admissionCount;
    diagnostic.admittedBytes += bytes;
    pc >>>= 0;
    let row = diagnostic.pcs.get(pc);
    if (!row && diagnostic.pcs.size === DIAGNOSTIC_PC_CAPACITY && diagnostic.prepareOnly.size) {
      const reclaimedPc = diagnostic.prepareOnly.values().next().value, reclaimed = diagnostic.pcs.get(reclaimedPc);
      diagnostic.prepareOnly.delete(reclaimedPc); diagnostic.pcs.delete(reclaimedPc);
      diagnostic.reclaimedPrepareOnlyPcs++;
      diagnostic.reclaimedPrepareOnlyPrepares += reclaimed.prepares;
      diagnostic.reclaimedPrepareOnlyHits += reclaimed.hits;
    }
    if (!row && diagnostic.pcs.size < DIAGNOSTIC_PC_CAPACITY) {
      row = compilationPc(pc,true);
      row.prepares = 1; row.prepareNs = diagnostic.prepare.lastNs;
      diagnostic.pcs.set(pc,row);
    }
    const previousIdentity = row?.lastIdentity ?? null, previousSourceVersion = row?.lastSourceVersion ?? null;
    if (row) {
      diagnostic.prepareOnly.delete(pc);
      if (row.admissions) {
        diagnostic.trackedRepeatPcAdmissions++;
        if (identity !== row.lastIdentity) {row.identityChanges++; diagnostic.samePcNewIdentityAdmissions++;}
        else {row.repeatedIdentityAdmissions++; diagnostic.samePcSameIdentityAdmissions++;}
      }
      row.admissions++; row.bytes += bytes; row.lastIdentity = identity; row.lastSourceVersion = null;
      row.firstSequence ||= sequence; row.lastSequence = sequence;
      if (row.admissions > 1) {
        if (!diagnostic.topRepeat.includes(row)) diagnostic.topRepeat.push(row);
        diagnostic.topRepeat.sort(repeatOrder); diagnostic.topRepeat.length = Math.min(diagnostic.topRepeat.length,16);
      }
    } else diagnostic.droppedPcAdmissions++;
    const record = {sequence,pc,identity,bytes,prepareNs:diagnostic.prepare.lastNs,
      moduleNs:diagnostic.module.lastNs,instanceNs:diagnostic.instance.lastNs,
      previousIdentity,previousSourceVersion,sourceVersion:null,sourceWitness:'pending',sourceComparison:null};
    diagnostic.sourceWitnessPendingAdmissions++;
    record.phaseTotalNs = record.prepareNs+record.moduleNs+record.instanceNs;
    diagnostic.last.push(record);
    if (diagnostic.last.length > DIAGNOSTIC_LAST_CAPACITY) diagnostic.last.shift();
    diagnostic.slow.push(record); diagnostic.slow.sort(slowOrder);
    diagnostic.slow.length = Math.min(diagnostic.slow.length,DIAGNOSTIC_SLOW_CAPACITY);
    return record;
  }
  recordSourceWitness(unit) {
    const record = unit.compilation;
    if (record.sourceWitness !== 'pending') return;
    const buffer = this.memory.buffer;
    if (this.#contextView?.buffer !== buffer) this.#contextView = new DataView(buffer);
    const version = this.#contextView.getBigUint64(this.context+96,true).toString();
    const diagnostic = this.compilation, row = diagnostic.pcs.get(record.pc);
    record.sourceVersion = version; record.sourceWitness = 'first-begin';
    diagnostic.sourceWitnessPendingAdmissions--; diagnostic.sourceWitnessedAdmissions++;
    if (!row) {
      diagnostic.droppedPcSourceWitnesses++; record.sourceComparison = 'untracked-pc';
      return;
    }
    row.sourceWitnesses++;
    if (row.lastIdentity === record.identity) row.lastSourceVersion = version;
    if (record.previousIdentity === null) record.sourceComparison = 'first-admission';
    else if (record.previousIdentity === record.identity) record.sourceComparison = 'same-identity';
    else if (record.previousSourceVersion === version) {
      record.sourceComparison = 'same-version';
      diagnostic.samePcNewIdentitySameSourceVersionAdmissions++; row.sameSourceVersionAdmissions++;
    } else {
      record.sourceComparison = 'unknown-source';
      diagnostic.samePcNewIdentityUnknownSourceAdmissions++; row.unknownSourceAdmissions++;
    }
  }
  compilationSnapshot() {
    const diagnostic = this.compilation;
    const snapshot = {
      semantics:'elapsed wall time; same-PC identity changes alone have no source-change witness',
      sourceWitnessSemantics:'first successful core_begin before guest execution; equal validation versions apply only within the same unchanged guest address space, not identical formed regions or emitted Wasm',
      sourceWitnessPendingSemantics:'pending or removed without a first-begin witness may include units executed only through native chaining',
      pcRetentionSemantics:'admitted PC rows take priority over prepare-only rows; prepare/hit histories may be partial after reclamation; droppedPcPrepares counts prepares without a row at collection time',
      prepareHitSemantics:'returned identity already instantiated in the SDK cache',
      pcCapacity:DIAGNOSTIC_PC_CAPACITY,lastCapacity:DIAGNOSTIC_LAST_CAPACITY,slowCapacity:DIAGNOSTIC_SLOW_CAPACITY,
      trackedPcCount:diagnostic.pcs.size,droppedPcPrepares:diagnostic.droppedPcPrepares,droppedPcAdmissions:diagnostic.droppedPcAdmissions,
      trackedAdmissionPcCount:diagnostic.pcs.size-diagnostic.prepareOnly.size,prepareOnlyPcCount:diagnostic.prepareOnly.size,
      reclaimedPrepareOnlyPcs:diagnostic.reclaimedPrepareOnlyPcs,reclaimedPrepareOnlyPrepares:diagnostic.reclaimedPrepareOnlyPrepares,
      reclaimedPrepareOnlyHits:diagnostic.reclaimedPrepareOnlyHits,
      admissionCount:diagnostic.admissionCount,admittedBytes:diagnostic.admittedBytes,prepareHits:diagnostic.prepareHits,
      trackedRepeatPcAdmissions:diagnostic.trackedRepeatPcAdmissions,
      samePcNewIdentityAdmissions:diagnostic.samePcNewIdentityAdmissions,samePcSameIdentityAdmissions:diagnostic.samePcSameIdentityAdmissions,
      sourceWitnessedAdmissions:diagnostic.sourceWitnessedAdmissions,
      sourceWitnessPendingAdmissions:diagnostic.sourceWitnessPendingAdmissions,sourceWitnessRemovedAdmissions:diagnostic.sourceWitnessRemovedAdmissions,
      samePcNewIdentitySameSourceVersionAdmissions:diagnostic.samePcNewIdentitySameSourceVersionAdmissions,
      samePcNewIdentityUnknownSourceAdmissions:diagnostic.samePcNewIdentityUnknownSourceAdmissions,
      samePcNewIdentityUnwitnessedAdmissions:diagnostic.samePcNewIdentityAdmissions-diagnostic.samePcNewIdentitySameSourceVersionAdmissions-diagnostic.samePcNewIdentityUnknownSourceAdmissions,
      droppedPcSourceWitnesses:diagnostic.droppedPcSourceWitnesses,
      topRepeatPcs:diagnostic.topRepeat.map(row=>({...row})),
      lastAdmissions:diagnostic.last.map(row=>({...row})),slowestAdmissions:diagnostic.slow.map(row=>({...row})),
    };
    for (const phase of ['prepare','module','instance']) {
      const metric = diagnostic[phase];
      snapshot[`${phase}Count`] = metric.count; snapshot[`${phase}Failures`] = metric.failures;
      snapshot[`${phase}Ns`] = metric.ns; snapshot[`${phase}MaxNs`] = metric.maxNs;
    }
    return snapshot;
  }
  encoded(pointerName, lengthName) {
    const pointer = this.native[pointerName]() >>> 0, length = this.native[lengthName]() >>> 0;
    if (!pointer || length < 8 || length > 16 * 1024 * 1024 || pointer + length > this.memory.buffer.byteLength)
      throw new Error('invalid core module descriptor');
    return new Uint8Array(this.memory.buffer).slice(pointer, pointer + length);
  }
  initializeDispatch() {
    this.table = new WebAssembly.Table({element:'anyfunc',initial:4097,maximum:4097});
    if (this.linkedCore) this.publishers = new WebAssembly.Table({element:'anyfunc',initial:4097,maximum:4097});
    this.slots = new Map();
    this.freeSlots = Array.from({length:4096},(_,index) => 4096-index);
    const module = new WebAssembly.Module(this.encoded('core_dispatch_code_ptr','core_dispatch_code_len'));
    const imports = WebAssembly.Module.imports(module);
    const expected = new Map(this.linkedCore
      ? [['memory','memory'],['functions','table'],['publishers','table'],['probe','function'],['cold','function']]
      : [['memory','memory'],['functions','table'],['next','function']]);
    if (imports.length !== expected.size || imports.some(item => item.module !== 'ring3' || expected.get(item.name) !== item.kind) ||
        new Set(imports.map(item => item.name)).size !== expected.size) throw new Error('invalid core dispatch imports');
    const bindings = this.linkedCore
      ? {memory:this.memory,functions:this.table,publishers:this.publishers,probe:this.native.core_chain_select,cold:this.native.core_chain_cold}
      : {memory:this.memory,functions:this.table,next:this.native.core_chain_next};
    const exports = new WebAssembly.Instance(module,{ring3:bindings}).exports;
    if (typeof exports.run !== 'function' || exports.run.length !== 3 || !(exports.regions instanceof WebAssembly.Global) || exports.regions.value !== 0)
      throw new Error('invalid core dispatch exports');
    const cold = ['coldRuns','coldWork','coldRetired'].map(name => exports[name]);
    if (cold.some(value => value !== undefined) &&
        !cold.every(value => value instanceof WebAssembly.Global && value.value === 0))
      throw new Error('invalid core dispatch cold counters');
    if (this.linkedCore) {
      if (!(exports.linkAbi instanceof WebAssembly.Global) || exports.linkAbi.value !== 3 ||
          !cold.every(value=>value !== undefined)) throw new Error('invalid core link ABI');
      const globals = {};
      for (const name of LINK_GLOBALS) {
        const value = exports[name];
        if (!(value instanceof WebAssembly.Global) || (value.value !== 0 && value.value !== 0n))
          throw new Error(`invalid core link global ${name}`);
        globals[name] = value;
      }
      this.#linkGlobals = Object.freeze(globals);
    }
    const supportNames = Object.keys(exports).filter(name=>name.startsWith('support.'));
    if (exports.supportAbi === undefined) {
      if (supportNames.length) throw new Error('core support exports lack an ABI');
    } else {
      if (!(exports.supportAbi instanceof WebAssembly.Global) || exports.supportAbi.value !== 5 ||
          supportNames.length !== SUPPORT_FUNCTIONS.length ||
          SUPPORT_FUNCTIONS.some(([name,arity])=>typeof exports[name] !== 'function' || exports[name].length !== arity))
        throw new Error('invalid core support ABI');
      const support = {'support.abi':exports.supportAbi};
      for (const [name] of SUPPORT_FUNCTIONS) support[name] = exports[name];
      this.#support = Object.freeze(support);
    }
    this.chainedColdEnabled = cold.every(value => value !== undefined);
    this.dispatch = exports;
  }
  nativeSnapshot(name,bytes) {
    const pointer = this.native[name]() >>> 0;
    if (!pointer || pointer % 8 || pointer + bytes > this.memory.buffer.byteLength)
      throw new Error(`invalid ${name} descriptor`);
    return new DataView(this.memory.buffer,pointer,bytes);
  }
  pruneNative() {
    const owners = this.nativeSnapshot('core_link_owners_ptr',4097*8), detached = [];
    if (owners.getBigUint64(0,true) !== 0n) throw new Error('reserved native core slot has an owner');
    for (const unit of this.units.values()) {
      const owner = owners.getBigUint64(unit.slot*8,true);
      if (owner === 0n) detached.push(unit);
      else if (owner !== unit.id) throw new Error('native core slot owner mismatch');
    }
    for (const unit of detached) this.remove(unit,'native');
  }
  remove(unit,reason) {
    const {id} = unit;
    if (this.chain) {
      const status = this.native.core_unlink(unit.slot,Number(id&0xffffffffn),Number(id>>32n));
      if (status !== 0) throw new Error(`core_unlink returned ${status}`);
      this.table.set(unit.slot,null); if(this.linkedCore)this.publishers.set(unit.slot,null); this.slots.delete(unit.slot); this.freeSlots.push(unit.slot);
    }
    this.units.delete(id); this.retainedModuleBytes -= unit.bytes;
    if (this.compilation && unit.compilation.sourceWitness === 'pending') {
      unit.compilation.sourceWitness = 'removed';
      this.compilation.sourceWitnessPendingAdmissions--; this.compilation.sourceWitnessRemovedAdmissions++;
    }
    if (reason === 'native') {this.prunedNative++; this.prunedNativeBytes += unit.bytes;}
    else {this.evictions++; this.evictionReasons[reason]++;}
  }
  evict(reason) {
    let unit;
    if (this.chain) {
      const slot = this.native.core_cache_victim() >>> 0;
      unit = this.slots.get(slot);
      if (!slot || slot > 4096 || !unit || this.units.get(unit.id) !== unit)
        throw new Error('invalid native core cache victim');
    } else unit = this.units.values().next().value;
    this.remove(unit,reason);
  }
  pristine() {
    if (this.engine.api !== this.api || this.engine.memory !== this.memory)
      throw new Error('core native bindings changed');
    for (let index = 0; index < this.#bindings.length; index++) {
      const binding = this.#bindings[index], name = binding[0], value = binding[1];
      if (this.api[name] !== value || this.native[name] !== value)
        throw new Error('core native bindings changed');
    }
  }
  word(offset) {
    const buffer = this.memory.buffer;
    if (this.#contextView?.buffer !== buffer) this.#contextView = new DataView(buffer);
    return this.#contextView.getUint32(this.context + offset, true);
  }
  get pc() { return this.context ? this.word(32) : this.engine.get(48); }
  select(pc = this.pc) {
    if (this.open) throw new Error('cannot select code during execution');
    this.pristine();
    if (this.native.core_cold_run) {
      if (this.native.core_lookup) {
        const lookup = () => {
          const status = this.native.core_lookup(pc) >>> 0;
          if (status !== 0 && status !== 4)
            throw Object.assign(new Error(`core_lookup returned ${status}`),{operation:'core_lookup',status});
          return status;
        };
        const status = this.compilation ? this.measureCompilation('prepare',lookup,pc) : lookup();
        if (status === 0) return this.#selectedEntry(pc,0);
      }
      return null;
    }
    return this.entry(pc);
  }
  retainContext(pointer) {
    if (!pointer || pointer % 8 || pointer + 128 > this.memory.buffer.byteLength)
      throw new Error('invalid core context allocation');
    if (this.context && pointer !== this.context) throw new Error('core context allocation changed');
    this.context = pointer;
  }
  entry(pc = this.pc) {
    if (this.open) throw new Error('cannot select code during execution');
    this.pristine();
    const prepare = () => {
      const status = this.native.core_prepare(pc,this.instructions,this.blocks) >>> 0;
      if (status !== 0 && status !== 18)
        throw Object.assign(new Error(`core_prepare returned ${status}`),{operation:'core_prepare',status});
      return status;
    };
    const prepared = this.compilation ? this.measureCompilation('prepare',prepare,pc) : prepare();
    return this.#selectedEntry(pc,prepared);
  }
  #selectedEntry(pc,prepared) {
    const context = this.native.core_context_ptr() >>> 0;
    if (prepared === 18 && (!context || context % 8 || context+128 > this.memory.buffer.byteLength))
      throw Object.assign(new Error('core_prepare returned 18'),{operation:'core_prepare',status:18});
    this.retainContext(context);
    const view = new DataView(this.memory.buffer), id = view.getBigUint64(context + 120, true);
    const transient = prepared === 18 && view.getUint32(context+112,true) === 2 && id !== 0n;
    if (prepared === 18 && !transient)
      throw Object.assign(new Error('core_prepare returned 18'),{operation:'core_prepare',status:18});
    if (prepared === 0 && view.getUint32(context+112,true) !== 0)
      throw new Error('invalid prepared core lease');
    this.transient = null;
    if (id === 0n) throw new Error('core code has no identity');
    if (!transient && this.units.has(id)) {
      const unit = this.units.get(id);
      if (!this.chain) {this.units.delete(id); this.units.set(id, unit);}
      this.hits++;
      if (this.compilation) {
        this.compilation.prepareHits++;
        const row = this.compilation.pcs.get(pc>>>0); if (row) row.hits++;
      }
      return unit;
    }
    if (this.chain) this.pruneNative();
    const encoded = this.encoded('core_code_ptr','core_code_len'), length = encoded.length;
    const module = this.compilation
      ? this.measureCompilation('module',()=>new WebAssembly.Module(encoded))
      : new WebAssembly.Module(encoded);
    const bindings = {memory: this.memory};
    const available = {read: this.native.core_read, write: this.native.core_write, numeric: this.native.core_numeric};
    const found = new Set();
    for (const item of WebAssembly.Module.imports(module)) {
      if (item.module !== 'ring3' || found.has(item.name)) throw new Error(`invalid core import ${item.module}.${item.name}`);
      found.add(item.name);
      if (item.name === 'memory' && item.kind === 'memory') continue;
      if (item.kind === 'global' && this.#linkGlobals && Object.hasOwn(this.#linkGlobals,item.name)) {
        bindings[item.name] = this.#linkGlobals[item.name]; continue;
      }
      if (item.name.startsWith('support.')) {
        if (!this.#support || !Object.hasOwn(this.#support,item.name) ||
            item.kind !== (item.name === 'support.abi' ? 'global' : 'function'))
          throw new Error(`invalid core support import ${item.name}`);
        bindings[item.name] = this.#support[item.name]; continue;
      }
      if (item.kind !== 'function' || !Object.hasOwn(available, item.name)) throw new Error(`invalid core import ${item.name}`);
      bindings[item.name] = available[item.name];
    }
    if (!['memory', 'read', 'write'].every(name => found.has(name)) ||
        (this.linkedCore && !LINK_GLOBALS.every(name=>found.has(name)))) throw new Error('incomplete core imports');
    if (this.#support && !SUPPORT_IMPORTS.every(name=>found.has(name)))
      throw new Error('incomplete core support imports');
    const instance = this.compilation
      ? this.measureCompilation('instance',()=>new WebAssembly.Instance(module,{ring3:bindings}))
      : new WebAssembly.Instance(module,{ring3:bindings});
    const run = instance.exports.run;
    if (typeof run !== 'function' || run.length !== 2) throw new Error('invalid core run export');
    const link = this.linkedCore ? instance.exports.link : run;
    const publish = this.linkedCore ? instance.exports.publish : null;
    if (this.linkedCore && (typeof link !== 'function' || link.length !== 2 ||
        typeof publish !== 'function' || publish.length !== 1)) throw new Error('invalid core link exports');
    const unit = {id, pc, run, link, publish, bytes: length};
    if (transient) {
      unit.transient = true;
      this.transient = unit;
      this.transientAdmissions++; this.transientModuleBytes += length; this.generatedModuleBytes += length;
      return unit;
    }
    while (this.units.size >= this.capacity || this.retainedModuleBytes + length > MAX_CODE_BYTES)
      this.evict(this.retainedModuleBytes + length > MAX_CODE_BYTES ? 'bytes' : 'count');
    if (this.chain) {
      const slot = this.freeSlots.pop();
      if (!slot || this.table.get(slot) !== null || (this.linkedCore && this.publishers.get(slot) !== null))
        throw new Error('invalid free core table slot');
      this.table.set(slot,link);
      if(this.linkedCore)this.publishers.set(slot,publish);
      const status = this.native.core_link(slot);
      if (status !== 0) {this.table.set(slot,null); if(this.linkedCore)this.publishers.set(slot,null); this.freeSlots.push(slot); throw new Error(`core_link returned ${status}`);}
      unit.slot = slot; this.slots.set(slot,unit);
    }
    this.units.set(id, unit); this.admissions++; this.generatedModuleBytes += length; this.retainedModuleBytes += length;
    if (this.compilation) unit.compilation = this.recordAdmission(pc,id,length);
    return unit;
  }
  execute(selected) {
    if (this.open) throw new Error('core execution is already active');
    this.pristine();
    if (selected === null && this.native.core_cold_run) return this.executeCold();
    if (!selected || (selected.transient ? this.transient !== selected : this.units.get(selected.id) !== selected))
      throw new Error('core region is not retained by this cache');
    if (this.dirtyWork > 0xffffffff - this.budget) this.publish();
    if (this.linkedCore) {
      const pointer = this.native.core_trace_pointer() >>> 0;
      if (!pointer || pointer % 8 || pointer + 32 + 129 * 24 > this.memory.buffer.byteLength)
        throw new Error('invalid core trace allocation');
      this.#linkGlobals['trace.arena'].value = pointer;
    }
    let unit = selected, status = this.native.core_begin();
    if (status === 4) {
      unit = this.select(this.pc);
      if (unit === null) return this.executeCold();
      status = this.native.core_begin();
    }
    if (status !== 0) throw Object.assign(new Error(`core_begin returned ${status}`), {operation: 'core_begin', status});
    this.open = true;
    const before = this.word(40), startPc = this.pc;
    let stop, failure;
    let regions;
    let chainedColdRuns = 0, chainedColdWork = 0, chainedColdRetired = 0;
    try {
      if (this.compilation && !unit.transient) this.recordSourceWitness(unit);
      if (this.chain && !unit.transient) {
        if (this.table.get(unit.slot) !== (this.linkedCore ? unit.link : unit.run) ||
            (this.linkedCore && this.publishers.get(unit.slot) !== unit.publish)) throw new Error('core table binding changed');
        stop = this.dispatch.run(this.context,this.budget,unit.slot) >>> 0;
        regions = this.dispatch.regions.value >>> 0;
        if (this.chainedColdEnabled) {
          chainedColdRuns = this.dispatch.coldRuns.value >>> 0;
          chainedColdWork = this.dispatch.coldWork.value >>> 0;
          chainedColdRetired = this.dispatch.coldRetired.value >>> 0;
        }
      } else {stop = unit.run(this.context, this.budget) >>> 0; regions = 1;}
    }
    catch (error) { failure = error; throw error; }
    finally {
      let publicationFailure;
      try {
        if(this.linkedCore && this.#linkGlobals['link.valid'].value !== 0) unit.publish(this.context);
      } catch(error) {publicationFailure=error;}
      const ended = this.native.core_end(); this.open = false;
      if (unit.transient) {this.transient = null; this.transientRuns++;}
      if (!failure && publicationFailure) throw publicationFailure;
      if (ended !== 0 && !failure) throw new Error(`core_end returned ${ended}`);
    }
    const result = this.finishRun(stop,before,regions,'compiled',startPc,chainedColdRuns,chainedColdWork,chainedColdRetired,
      unit.transient ? unit.id : null);
    this.installRecordedTrace();
    return result;
  }
  installRecordedTrace() {
    if (!this.native.core_trace_ready || this.native.core_trace_ready() !== 1) return;
    const root = this.native.core_trace_root() >>> 0;
    const status = this.native.core_trace_finish(root,this.instructions,this.blocks) >>> 0;
    // The optimizer already selected the immutable owner. Installation must
    // never invoke baseline formation, including after a cold-only capture.
    if (status === 0) {
      const installedRoot = this.native.core_trace_ready() === 2
        ? this.native.core_trace_root() >>> 0 : root;
      this.#selectedEntry(installedRoot,0);
    }
    else if (![4,7,8,18,20].includes(status))
      throw Object.assign(new Error(`core_trace_finish returned ${status}`),{operation:'core_trace_finish',status});
  }
  executeCold() {
    if (this.dirtyWork > 0xffffffff - this.budget) this.publish();
    const pointer = this.native.core_context_ptr() >>> 0;
    if (pointer) this.retainContext(pointer);
    const before = this.context ? this.word(40) : 0, startPc = this.pc;
    let stop;
    this.open = true;
    try {stop = this.native.core_cold_run(this.budget);}
    finally {this.open = false;}
    if (Number.isInteger(stop) && stop < 0)
      throw Object.assign(new Error(`core_cold_run returned ${stop}`),{operation:'core_cold_run',status:-stop});
    this.retainContext(this.native.core_context_ptr() >>> 0);
    const result = this.finishRun(stop,before,0,'cold',startPc);
    this.installRecordedTrace();
    return result;
  }
  finishRun(stop,before,regions,kind,startPc,chainedColdRuns=0,chainedColdWork=0,chainedColdRetired=0,transientId=null) {
    const cold = kind === 'cold', retired = (this.word(40) - before) >>> 0;
    const remaining = this.word(44), work = this.budget - remaining;
    if (!Number.isInteger(chainedColdRuns) || chainedColdRuns < 0 ||
        !Number.isInteger(chainedColdWork) || chainedColdWork < 0 ||
        !Number.isInteger(chainedColdRetired) || chainedColdRetired < 0 ||
        chainedColdRetired > chainedColdWork || chainedColdRetired > retired || chainedColdWork > work ||
        chainedColdRuns > chainedColdWork+1 || (!chainedColdRuns && (chainedColdWork || chainedColdRetired)))
      throw new Error('invalid chained cold execution counters');
    if (remaining > this.budget || retired > work || !Number.isInteger(stop) || stop < 1 || stop > 10 || stop === 8 ||
        !Number.isInteger(regions) || regions < (cold ? 0 : 1) || regions > work-chainedColdWork+1)
      throw new Error(`invalid core stop ${stop} at 0x${this.pc.toString(16)}`);
    const identity = new DataView(this.memory.buffer).getBigUint64(this.context+120,true);
    const coldEnd = !cold && this.chain && this.chainedColdEnabled && chainedColdRuns > 0 && identity === 0n;
    if (cold ? identity !== 0n : transientId !== null ? identity !== transientId : !coldEnd && !this.units.has(identity))
      throw new Error(cold ? 'cold execution retained a compiled identity' : 'core dispatch selected an unretained region');
    if (!cold && this.jitPc === startPc && stop === 1 && work === 0 && this.pc === startPc)
      throw new Error(`core JIT handoff made no progress at 0x${startPc.toString(16)}`);
    this.jitPc = (cold || coldEnd) && stop === 1 ? this.pc : null;
    if (cold) {this.coldRuns++;this.coldRetired += retired;this.coldWork += work;}
    else {this.runs++;this.regions += regions;}
    this.chainedColdRuns += chainedColdRuns; this.chainedColdWork += chainedColdWork; this.chainedColdRetired += chainedColdRetired;
    this.retired += retired; this.work += work; this.dirtyWork += work; this.lastStop = stop; this.dirty = true;
    if (stop !== 1) this.publish();
    if (stop === 4) throw Object.assign(new Error(`unsupported instruction at 0x${this.pc.toString(16)}`), {pc:this.pc,reason:stop});
    return {pc: this.pc, coreReason: stop, reason: stop === 1 ? null : this.engine.get(72),
      detail: stop === 1 ? null : this.engine.get(80), retired, work, kind, id:cold || coldEnd ? null : identity, regions,
      chainedColdRuns,chainedColdWork,chainedColdRetired};
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
  sampleUsage() {
    if (this.open) throw new Error('cannot sample an active core cache');
    if (this.chain && this.context) {
      this.pristine();
      const data = this.nativeSnapshot('core_cache_status_ptr',64);
      this.nativeCache = Object.fromEntries(NATIVE_COUNTERS.map((name,index) => [name,data.getBigUint64(index*8,true).toString()]));
    }
    return this.stats();
  }
  coldDiagnostics() {
    if (this.open) throw new Error('cannot sample active cold execution');
    if (!this.native.core_cold_status_ptr || !this.context) return null;
    this.pristine();
    const data=this.nativeSnapshot('core_cold_status_ptr',104);
    if (data.getBigUint64(0,true)!==1n) throw new Error('invalid cold diagnostic version');
    return {schema:1,...Object.fromEntries(COLD_COUNTERS.map((name,index)=>[name,data.getBigUint64((index+1)*8,true).toString()]))};
  }
  stats() {
    return {backend: 'core', chainEnabled:this.chain, runs: this.runs, regions:this.regions, retired: this.retired, work: this.work, admissions: this.admissions,
      coldEnabled:!!this.native.core_cold_run,coldRuns:this.coldRuns,coldRetired:this.coldRetired,coldWork:this.coldWork,
      chainedColdRuns:this.chainedColdRuns,chainedColdWork:this.chainedColdWork,chainedColdRetired:this.chainedColdRetired,
      compileInstructionLimit:this.instructions, compileBlockLimit:this.blocks, workBudget:this.budget,
      engineCacheHits: this.hits, evictions: this.evictions, generatedModuleBytes: this.generatedModuleBytes,
      retainedModuleBytes: this.retainedModuleBytes, installedUnits: this.units.size, unitCapacity: this.capacity,
      transientAdmissions:this.transientAdmissions,transientRuns:this.transientRuns,transientModuleBytes:this.transientModuleBytes,
      byteCapacity:MAX_CODE_BYTES, evictionReasons:{...this.evictionReasons},
      prunedNative:this.prunedNative, prunedNativeBytes:this.prunedNativeBytes,
      nativeCache:this.nativeCache && {...this.nativeCache},
      ...(this.compilation ? {cacheDiagnostics:this.compilationSnapshot()} : {}),
      wasmMemoryBytes: this.memory.buffer.byteLength};
  }
}
