import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {readFileSync, writeFileSync} from 'node:fs';
import {dirname, isAbsolute, join, relative, resolve} from 'node:path';
import {fileURLToPath} from 'node:url';

const SHAPES = [
  {units: 1, blocks: 1}, {units: 1, blocks: 8},
  {units: 4, blocks: 1}, {units: 4, blocks: 8},
  {units: 8, blocks: 1}, {units: 8, blocks: 8},
];
const ORDERS = [[0, 1, 2, 3, 4, 5], [2, 3, 4, 5, 0, 1], [4, 5, 0, 1, 2, 3]];
const UNIT_PHASES = ['compile', 'copy', 'module', 'instance', 'table', 'ack'];
const CONTEXT_PHASES = ['engine_instance', 'open', 'table_create', 'input', 'map', 'upload', 'protect',
  'dispatcher_module', 'dispatcher_copy', 'dispatcher_v8_module', 'dispatcher_instance'];
const LIMIT_NS = 180_000_000_000n;

export function parseNs(value, label = 'duration') {
  assert.equal(typeof value, 'string', `${label}: expected decimal string`);
  assert.match(value, /^[1-9][0-9]*$/, `${label}: expected positive canonical nanoseconds`);
  const ns = BigInt(value);
  assert.ok(ns <= LIMIT_NS, `${label}: exceeds child deadline`);
  return Number(ns);
}

export function quantile(values, probability) {
  assert.ok(Array.isArray(values) && values.length > 0, 'quantile needs observations');
  assert.ok(Number.isFinite(probability) && probability >= 0 && probability <= 1);
  assert.ok(values.every(Number.isFinite), 'quantile needs finite observations');
  const sorted = [...values].sort((a, b) => a - b);
  const position = (sorted.length - 1) * probability;
  const low = Math.floor(position), high = Math.ceil(position);
  return sorted[low] + (position - low) * (sorted[high] - sorted[low]);
}

export function describe(values) {
  const q1 = quantile(values, 0.25), median = quantile(values, 0.5), q3 = quantile(values, 0.75);
  return {n: values.length, min: Math.min(...values), q1, median, q3, max: Math.max(...values), iqr: q3 - q1};
}

export function summarizeDurations(raw, calls, instructionsPerCall = 0) {
  assert.ok(Number.isSafeInteger(calls) && calls > 0, 'positive call count required');
  assert.ok(Number.isSafeInteger(instructionsPerCall) && instructionsPerCall >= 0);
  const values = raw.map((value, index) => parseNs(value, `sample ${index}`));
  return {
    batch_ns: describe(values),
    ns_per_call: describe(values.map(ns => ns / calls)),
    ...(instructionsPerCall ? {
      ns_per_model_expected_instruction: describe(values.map(ns => ns / calls / instructionsPerCall)),
    } : {}),
  };
}

export function compareProcesses(pairs) {
  assert.equal(pairs.length, 3, 'decision requires three independent processes');
  const processes = pairs.map(({process, a, b}, index) => {
    assert.equal(process, index);
    for (const values of [a, b]) {
      assert.ok([values.q1, values.median, values.q3].every(value => Number.isFinite(value) && value > 0));
      assert.ok(values.q1 <= values.median && values.median <= values.q3);
    }
    const relativeEffect = (b.median - a.median) / a.median;
    const separated = a.q3 < b.q1 || b.q3 < a.q1;
    return {process, relative_effect: relativeEffect, strictly_separated_iqr: separated};
  });
  const direction = Math.sign(processes[0].relative_effect);
  const qualifies = direction !== 0 && processes.every(row =>
    Math.sign(row.relative_effect) === direction && Math.abs(row.relative_effect) >= 0.1
      && row.strictly_separated_iqr);
  return {qualifies_for_separate_attribution: qualifies, processes,
    limit: 'descriptive heuristic; fixed order and execution composition confound causal attribution'};
}

function index(value, maximum, label) {
  assert.ok(Number.isSafeInteger(value) && value >= 0 && value < maximum, `${label}: invalid index`);
}

function record(object, label) {
  assert.ok(object && typeof object === 'object' && !Array.isArray(object), `${label}: expected object`);
}

export function summarizeSamples(samples, process, complete) {
  index(process, 3, 'process');
  assert.ok(Array.isArray(samples), 'samples must be an array');
  const groups = new Map(), contexts = new Map();
  for (const row of samples) {
    record(row, 'sample');
    assert.equal(row.process, process);
    index(row.shape, 6, 'shape');
    index(row.sample, 30, 'sample');
    assert.ok(Number.isSafeInteger(row.context) && row.context >= 0);
    const instructionKind = ['dispatch', 'direct'].includes(row.kind);
    assert.ok(instructionKind || ['guard', 'finder', 'empty'].includes(row.kind), 'unknown sample kind');
    assert.ok(row.kind !== 'direct' || SHAPES[row.shape].units === 1, 'direct samples require one unit');
    assert.equal(row.calls, instructionKind ? 32 : 1024, 'sample call count mismatch');
    assert.equal(row.expected_instructions_per_call, instructionKind ? 8192 : 0, 'sample instruction count mismatch');
    assert.equal(row.status_or, 0, 'nonzero or missing batch status');
    parseNs(row.ns);
    const key = `${row.shape}/${row.kind}`, group = groups.get(key) ?? new Map();
    assert.ok(!group.has(row.sample), 'duplicate sample');
    group.set(row.sample, row.ns); groups.set(key, group);
    if (contexts.has(row.shape)) assert.equal(contexts.get(row.shape), row.context, 'shape context changed');
    contexts.set(row.shape, row.context);
  }
  const summaries = [];
  if (complete) assert.equal(new Set(contexts.values()).size, 6, 'warm contexts are not distinct');
  for (const shape of ORDERS[process]) {
    const kinds = ['dispatch', 'guard', 'finder', 'empty', ...(SHAPES[shape].units === 1 ? ['direct'] : [])];
    for (const kind of kinds) {
      const group = groups.get(`${shape}/${kind}`);
      if (complete) assert.equal(group?.size, 30, `${shape}/${kind}: expected 30 samples`);
      if (!group?.size) continue;
      const rawNs = [...group].sort(([a], [b]) => a - b).map(([, ns]) => ns);
      const instructionKind = ['dispatch', 'direct'].includes(kind), calls = instructionKind ? 32 : 1024;
      const summary = summarizeDurations(rawNs, calls, instructionKind ? 8192 : 0);
      if (!instructionKind) {
        summary[kind === 'empty' ? 'ns_per_iteration' : 'ns_per_export'] = summary.ns_per_call;
        delete summary.ns_per_call;
      }
      summaries.push({shape, ...SHAPES[shape], context: contexts.get(shape), kind, calls_per_sample: calls,
        model_expected_instructions_per_call: instructionKind ? 8192 : 0, raw_ns: rawNs, ...summary});
    }
  }
  return summaries;
}

export function summarizeCold(phases, process, complete) {
  assert.ok(Array.isArray(phases), 'phases must be an array');
  const contexts = new Map();
  const firstContext = phases.find(row => row.phase === 'engine_instance')?.context;
  for (const row of phases) {
    record(row, 'phase'); assert.equal(row.process, process);
    parseNs(row.ns);
    if (row.context_kind !== 'cold' || ['cold_witness', 'close'].includes(row.phase)) continue;
    index(row.shape, 6, 'phase shape');
    assert.ok(Number.isSafeInteger(row.context) && row.context >= 0);
    assert.ok(Number.isSafeInteger(row.cold_ordinal) && row.cold_ordinal >= 1 && row.cold_ordinal <= 30);
    assert.ok(typeof row.phase === 'string' && /^[a-z0-9_]+$/.test(row.phase));
    assert.ok(Number.isSafeInteger(row.event) && row.event >= 0
      || typeof row.event === 'string' && /^open:[1-9][0-9]*$/.test(row.event));
    const unit = row.unit_ordinal;
    assert.ok(unit === null || Number.isSafeInteger(unit) && unit >= 1 && unit <= SHAPES[row.shape].units);
    if (unit !== null) assert.ok(UNIT_PHASES.includes(row.phase), 'unknown unit phase');
    else assert.ok(CONTEXT_PHASES.includes(row.phase), 'unknown construction phase');
    const ns = parseNs(row.ns), key = `${row.shape}/${row.cold_ordinal}`;
    const context = contexts.get(key) ?? {shape: row.shape, ordinal: row.cold_ordinal, context: row.context, rows: new Map()};
    assert.equal(context.context, row.context, 'cold context identity changed');
    const phaseKey = `${row.event}/${unit}/${row.phase}`;
    assert.ok(!context.rows.has(phaseKey), 'duplicate cold phase');
    context.rows.set(phaseKey, {...row, value: ns}); contexts.set(key, context);
  }
  const results = [];
  assert.equal(new Set([...contexts.values()].map(row => row.context)).size, contexts.size, 'cold context reused');
  for (const shape of ORDERS[process]) {
    const selected = [...contexts.values()].filter(row => row.shape === shape).sort((a, b) => a.ordinal - b.ordinal);
    if (complete) assert.equal(selected.length, 30, 'expected 30 cold contexts per shape');
    const totals = [], shares = new Map(), phaseTotals = new Map(), units = new Map();
    const contextRows = [];
    for (const context of selected) {
      if (complete) {
        const units = SHAPES[shape].units;
        const counts = {engine_instance: 1, open: 1, table_create: 1, input: 2 + 2 * units,
          map: units, upload: units, protect: units, dispatcher_module: 1,
          dispatcher_copy: 1, dispatcher_v8_module: 1, dispatcher_instance: 1};
        for (const [phase, count] of Object.entries(counts)) assert.equal(
          [...context.rows.values()].filter(row => row.unit_ordinal === null && row.phase === phase).length,
          count, `cold setup phase count mismatch ${shape}/${context.ordinal}/${phase}`);
        for (let unit = 1; unit <= units; unit++) for (const phase of UNIT_PHASES) {
          assert.equal([...context.rows.values()].filter(row => row.unit_ordinal === unit && row.phase === phase).length,
            1, `missing or repeated cold unit phase ${shape}/${context.ordinal}/${unit}/${phase}`);
        }
      }
      const phaseNs = {};
      for (const row of context.rows.values()) {
        phaseNs[row.phase] = (phaseNs[row.phase] ?? 0) + row.value;
        if (row.unit_ordinal !== null) {
          const key = `${row.unit_ordinal}/${row.phase}`, values = units.get(key) ?? [];
          values.push(row.value); units.set(key, values);
        }
      }
      const total = Object.values(phaseNs).reduce((sum, value) => sum + value, 0);
      totals.push(total);
      const phaseShares = {};
      for (const [phase, value] of Object.entries(phaseNs)) {
        phaseShares[phase] = value / total;
        const values = shares.get(phase) ?? []; values.push(value / total); shares.set(phase, values);
        const durations = phaseTotals.get(phase) ?? []; durations.push(value); phaseTotals.set(phase, durations);
      }
      contextRows.push({context: context.context, cold_ordinal: context.ordinal,
        process_first_use: context.context === firstContext,
        disjoint_phase_total_ns: total, phase_ns: phaseNs, phase_share: phaseShares});
    }
    if (!selected.length) continue;
    results.push({shape, ...SHAPES[shape], context_count: selected.length,
      process_first_context_included: contextRows.find(row => row.process_first_use) ?? null,
      disjoint_phase_total_ns: describe(totals), contexts: contextRows,
      phase_ns: Object.fromEntries([...phaseTotals].map(([phase, values]) => [phase, describe(values)])),
      phase_share: Object.fromEntries([...shares].map(([phase, values]) => [phase, describe(values)])),
      unit_ordinals: [...units].map(([key, values]) => {
        const [unit, phase] = key.split('/');
        if (complete) assert.equal(values.length, 30);
        return {unit_ordinal: Number(unit), comparison_scope: Number(unit) <= 4 ? 'common_1_to_4' : 'added_5_to_8',
          phase, ns: describe(values)};
      }),
    });
  }
  return results;
}

function selfTest() {
  assert.deepEqual(describe([1, 2, 3, 4]), {n: 4, min: 1, q1: 1.75, median: 2.5, q3: 3.25, max: 4, iqr: 1.5});
  const thirty = Array.from({length: 30}, (_, i) => i + 1);
  assert.deepEqual([quantile(thirty, 0.25), quantile(thirty, 0.5), quantile(thirty, 0.75)], [8.25, 15.5, 22.75]);
  for (const invalid of ['0', '-1', '1.5', 'NaN', '01', 1, '180000000001']) assert.throws(() => parseNs(invalid));
  assert.equal(summarizeDurations(['8192', '16384'], 1, 8192).ns_per_model_expected_instruction.median, 1.5);
  const pairs = Array.from({length: 3}, (_, process) => ({process,
    a: {median: 100, q1: 95, q3: 105}, b: {median: 120, q1: 115, q3: 125}}));
  assert.equal(compareProcesses(pairs).qualifies_for_separate_attribution, true);
  pairs[2].b = {median: 80, q1: 75, q3: 85};
  assert.equal(compareProcesses(pairs).qualifies_for_separate_attribution, false);
  pairs[2].b = {median: 120, q1: 105, q3: 125};
  assert.equal(compareProcesses(pairs).qualifies_for_separate_attribution, false);
  assert.throws(() => summarizeSamples([], 0, true));
  assert.throws(() => summarizeSamples([{process: 0, shape: 0, context: 0, kind: 'dispatch', sample: 0,
    ns: '1', calls: 31, expected_instructions_per_call: 8192}], 0, false));
  const samples = SHAPES.flatMap((shape, id) => ['dispatch', 'guard', 'finder', 'empty', ...(shape.units === 1 ? ['direct'] : [])]
    .flatMap(kind => Array.from({length: 30}, (_, sample) => ({process: 0, shape: id, context: id, kind, sample,
      ns: String(sample + 1), calls: ['dispatch', 'direct'].includes(kind) ? 32 : 1024,
      expected_instructions_per_call: ['dispatch', 'direct'].includes(kind) ? 8192 : 0, status_or: 0}))));
  assert.equal(summarizeSamples(samples, 0, true).length, 26);
  assert.throws(() => summarizeSamples(samples.slice(1), 0, true));
  assert.throws(() => summarizeSamples([...samples, samples[0]], 0, true));
  assert.throws(() => summarizeSamples([{...samples[0], status_or: 1}], 0, false));
  const phase = (name, ns, unit, event) => ({process: 0, shape: 0, context: 1, context_kind: 'cold', cold_ordinal: 1,
    unit_ordinal: unit, phase: name, ns: String(ns), event});
  const cold = [...UNIT_PHASES.map((name, event) => phase(name, 10, 1, event)),
    phase('input', 10, null, 6), phase('input', 20, null, 7), phase('dispatcher_module', 30, null, 8),
    phase('dispatcher_copy', 40, null, 9), phase('dispatcher_v8_module', 40, null, 9), phase('dispatcher_instance', 40, null, 9),
    phase('cold_witness', 1000, null, 10), phase('close', 200, null, 11)];
  const construction = summarizeCold(cold, 0, false)[0];
  assert.equal(construction.disjoint_phase_total_ns.median, 240);
  assert.equal(construction.phase_share.compile.median, 10 / 240);
  assert.equal(construction.phase_ns.copy.median, 10);
  assert.equal(construction.phase_ns.dispatcher_copy.median, 40);
  assert.throws(() => summarizeCold([...cold, cold[0]], 0, false));
}

function comparisons(processes) {
  if (processes.length !== 3 || processes.some(row => !row.complete)) return [];
  const results = [];
  for (const kind of ['dispatch', 'guard', 'finder']) {
    for (const [a, b, label] of [[2, 4, '4 versus 8 units, 1 block'], [3, 5, '4 versus 8 units, 8 blocks'],
      [0, 1, '1 versus 8 blocks, 1 unit'], [2, 3, '1 versus 8 blocks, 4 units'], [4, 5, '1 versus 8 blocks, 8 units']]) {
      results.push({kind, a_shape: a, b_shape: b, label, ...compareProcesses(processes.map(row => ({
        process: row.process, a: row.samples.find(s => s.shape === a && s.kind === kind).batch_ns,
        b: row.samples.find(s => s.shape === b && s.kind === kind).batch_ns,
      })))});
    }
  }
  for (const shape of [0, 1]) results.push({kind: 'composition', shape, label: `direct versus dispatcher, ${SHAPES[shape].blocks} blocks`,
    ...compareProcesses(processes.map(row => ({process: row.process,
      a: row.samples.find(s => s.shape === shape && s.kind === 'direct').batch_ns,
      b: row.samples.find(s => s.shape === shape && s.kind === 'dispatch').batch_ns,
    })))});
  for (const [a, b] of [[2, 4], [3, 5]]) for (let ordinal = 1; ordinal <= 4; ordinal++) for (const phase of UNIT_PHASES) {
    results.push({kind: 'cold_common_ordinal', a_shape: a, b_shape: b, unit_ordinal: ordinal, phase,
      label: `4 versus 8 units, ${SHAPES[a].blocks} blocks, unit ${ordinal}, ${phase}`,
      ...compareProcesses(processes.map(row => ({process: row.process,
        a: row.cold.find(s => s.shape === a).unit_ordinals.find(u => u.unit_ordinal === ordinal && u.phase === phase).ns,
        b: row.cold.find(s => s.shape === b).unit_ordinals.find(u => u.unit_ordinal === ordinal && u.phase === phase).ns,
      })))});
  }
  return results;
}

function validatePins(pins) {
  record(pins, 'source pins');
  assert.ok(Object.keys(pins).length > 0, 'missing source pins');
  for (const [path, value] of Object.entries(pins)) {
    assert.ok(path.length > 0); record(value, 'source pin');
    assert.match(value.sha256, /^[0-9a-f]{64}$/);
    assert.ok(Number.isSafeInteger(value.bytes) && value.bytes > 0);
  }
}

function sameArtifact(a, b) {
  record(a, 'artifact'); record(b, 'artifact');
  assert.match(a.sha256, /^[0-9a-f]{64}$/);
  assert.ok(Number.isSafeInteger(a.bytes) && a.bytes > 0);
  assert.equal(a.sha256, b.sha256); assert.equal(a.bytes, b.bytes);
}

function within(directory, name) {
  assert.equal(typeof name, 'string'); assert.ok(!isAbsolute(name));
  const path = resolve(directory, name), difference = relative(directory, path);
  assert.ok(difference && !difference.startsWith('..') && !isAbsolute(difference), 'result escapes campaign directory');
  return path;
}

export function summarizeCampaign(campaign, children) {
  record(campaign, 'campaign'); assert.equal(campaign.schema_version, 1);
  assert.ok(['complete', 'incomplete'].includes(campaign.status));
  assert.ok(Array.isArray(campaign.children) && campaign.children.length <= 3);
  validatePins(campaign.source_pins);
  assert.ok(Array.isArray(children) && children.length === campaign.children.length);
  const seen = new Set(), processes = [];
  for (let index = 0; index < children.length; index++) {
    const binding = campaign.children[index], child = children[index];
    indexCheck(binding.process_index, seen);
    assert.ok(['complete', 'incomplete'].includes(binding.status), 'invalid bound child status');
    if (child === null) {
      assert.notEqual(campaign.status, 'complete', 'complete campaign missing child');
      processes.push({process: binding.process_index, complete: false, missing_result: true}); continue;
    }
    record(child, 'child'); assert.equal(child.schema_version, 1);
    assert.equal(child.process_index, binding.process_index);
    assert.ok(['complete', 'failed'].includes(child.status));
    const complete = binding.status === 'complete' && binding.exit_code === 0 && child.status === 'complete';
    if (campaign.status === 'complete') assert.ok(complete, 'complete campaign has failed child');
    if (child.engine) sameArtifact(child.engine, campaign.engine);
    else assert.ok(!complete && child.samples.length === 0 && child.phases.length === 0, 'missing engine with observations');
    const expectedPins = Object.keys(campaign.source_pins).sort().map(path => ({path, ...campaign.source_pins[path]}));
    if (child.source_pins_before) assert.deepEqual(child.source_pins_before, expectedPins, 'child initial source pins differ');
    else assert.ok(!complete && !child.samples.length && !child.phases.length, 'missing initial pins with observations');
    assert.deepEqual(child.source_pins_after, expectedPins, 'child final source pins differ');
    assert.match(child.plan.sha256, /^[0-9a-f]{64}$/);
    const firstInstance = child.phases.find(row => row.phase === 'engine_instance');
    const firstDispatch = child.phases.find(row => row.phase === 'cold_witness');
    if (complete) {
      assert.ok(firstInstance && firstDispatch, 'missing process first-use phases');
      assert.equal(firstInstance.context, firstDispatch.context, 'first dispatch does not belong to first instance');
    }
    processes.push({process: child.process_index, complete,
      engine: child.engine, plan: child.plan, environment: child.environment,
      samples: summarizeSamples(child.samples, child.process_index, complete),
      cold: summarizeCold(child.phases, child.process_index, complete),
      first_use: {engine_read_ns: child.first_use?.engine_read_ns, engine_module_ns: child.first_use?.engine_module_ns,
        process_first_instance: firstInstance ?? null, process_first_dispatch: firstDispatch ?? null,
        warm_shape_first_dispatches: child.phases.filter(row => row.context_kind === 'warm' && row.phase === 'first'),
        other_phases: child.phases.filter(row => row.context_kind !== 'cold')},
    });
  }
  processes.sort((a, b) => a.process - b.process);
  const complete = campaign.status === 'complete' && processes.length === 3 && processes.every(row => row.complete);
  if (campaign.status === 'complete') assert.ok(complete, 'complete campaign needs three processes');
  if (complete) {
    assert.equal(campaign.build.exit_code, 0, 'complete campaign has failed build');
    assert.equal(campaign.build.timed_out, false, 'complete campaign build timed out');
    assert.ok(campaign.children.every(row => row.timed_out === false), 'complete campaign child timed out');
  }
  const available = processes.filter(row => !row.missing_result);
  if (available.length) {
    for (const row of available) {
      if (row.first_use.engine_read_ns !== undefined) parseNs(row.first_use.engine_read_ns, 'engine read');
      else assert.ok(!row.complete, 'missing first-use engine read');
      if (row.first_use.engine_module_ns !== undefined) parseNs(row.first_use.engine_module_ns, 'engine module');
      else assert.ok(!row.complete, 'missing first-use engine module');
    }
  }
  const decisions = complete ? comparisons(processes) : [];
  return {schema_version: 1, status: complete ? 'complete' : 'incomplete', engine: campaign.engine,
    source_pins: campaign.source_pins, build: campaign.build, processes, comparisons: decisions,
    separate_attribution_candidates: decisions.filter(row => row.qualifies_for_separate_attribution),
    policy: {samples_per_series: 30, quantile: 'linear interpolation at (n-1)*p', threshold_relative_to_a: 0.1,
      gate: 'same direction, >=10% median effect and strict IQR separation in all three processes',
      process_orders: ORDERS, pooling: false, overhead_subtraction: false,
      normalization: 'model-expected guest instructions; intermediate exits are unobserved',
      cold: 'all 30 retained: first shape includes one process-first context plus 29 subsequent constructions; other shapes have 30 subsequent constructions. Shares use recorded disjoint phase sums, excluding GC, bookkeeping, validation and close; not inclusive wall time',
      limits: ['fixed shape-order parity and adjacent pair order', 'one unit has one child entry; four/eight have 128',
        'block count changes emitted control flow', 'GC/JIT/scheduler/frequency interference',
        'IQR gate is descriptive, not a significance test', 'no regression, causal cost, browser/game or optimization claim']},
  };
}

function indexCheck(process, seen) {
  index(process, 3, 'bound process'); assert.ok(!seen.has(process), 'duplicate process'); seen.add(process);
}

function markdown(summary) {
  const lines = ['# Resident phase baseline', '', `Campaign: **${summary.status}**.`, '',
    'Nanosecond-valued timings are summarized per process. Instruction denominators are model-expected; timed intermediate exits are unobserved.', '',
    '| Process | Units | Blocks | Kind | Samples | Median ns/call or export | IQR | Median ns/expected instruction |', '| --- | --- | --- | --- | --- | --- | --- | --- |'];
  const number = value => value.toFixed(3);
  for (const process of summary.processes) for (const sample of process.samples ?? []) {
    const values = sample.ns_per_call ?? sample.ns_per_export ?? sample.ns_per_iteration;
    const instruction = sample.ns_per_model_expected_instruction;
    lines.push(`| ${process.process} | ${sample.units} | ${sample.blocks} | ${sample.kind} | ${values.n} | ${number(values.median)} | ${number(values.iqr)} | ${instruction ? number(instruction.median) : '—'} |`);
  }
  lines.push('', '## Cold recorded phase shares', '',
    'Each entry summarizes all 30 raw contexts. The first shape includes one process-first context and 29 subsequent constructions. Shares are medians of per-context ratios; their medians need not sum to 100%.', '',
    '| Process | Units | Blocks | Median disjoint ns | Compile % | Copy % | Module % | Instance % | Table % | Ack % | First context included |',
    '| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |');
  for (const process of summary.processes) for (const cold of process.cold ?? []) {
    const shares = UNIT_PHASES.map(phase => cold.phase_share[phase] ? number(cold.phase_share[phase].median * 100) : '—');
    lines.push(`| ${process.process} | ${cold.units} | ${cold.blocks} | ${number(cold.disjoint_phase_total_ns.median)} | ${shares.join(' | ')} | ${cold.process_first_context_included?.context ?? '—'} |`);
  }
  lines.push('', '## Mechanical follow-up gate', '',
    `${summary.separate_attribution_candidates.length} descriptive candidates meet the frozen rule. Incomplete campaigns receive no threshold decision.`, '');
  for (const candidate of summary.separate_attribution_candidates) lines.push(`- ${candidate.kind}: ${candidate.label}`);
  lines.push('', 'Cold phase shares and common unit ordinals are preserved in statistics.json. They use disjoint sums within each context; added ordinals 5–8 remain separate.', '',
    'Rotations preserve adjacent pair order. One-unit execution and eight-block emission change composition. Automatic GC, JIT and host scheduling remain limits. A signal opens a separate attribution contract; representative workload evidence is required before optimization.', '');
  return lines.join('\n');
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  if (process.argv[2] === '--self-test') {
    assert.equal(process.argv.length, 3); selfTest(); console.log('statistics self-test: PASS');
  } else {
    assert.equal(process.argv.length, 3, 'usage: statistics.mjs OUTPUT_CAMPAIGN_DIR');
    const output = resolve(process.argv[2]);
    const campaign = JSON.parse(readFileSync(join(output, 'campaign.json'), 'utf8'));
    const inputs = [], children = campaign.children.map(binding => {
      const path = within(output, binding.result_file);
      let bytes;
      try {
        bytes = readFileSync(path);
      } catch (error) {if (error.code === 'ENOENT' && campaign.status !== 'complete') return null; throw error;}
      const child = JSON.parse(bytes.toString('utf8'));
      const planBytes = readFileSync(within(dirname(path), child.plan.file));
      assert.equal(createHash('sha256').update(planBytes).digest('hex'), child.plan.sha256, 'physical child plan differs');
      const pin = {bytes: bytes.length, sha256: createHash('sha256').update(bytes).digest('hex')};
      if (binding.result_pin) assert.deepEqual(pin, binding.result_pin, 'physical result differs from supervisor receipt');
      else assert.notEqual(campaign.status, 'complete', 'complete result lacks supervisor pin');
      inputs.push({file: binding.result_file, ...pin});
      return child;
    });
    const summary = {...summarizeCampaign(campaign, children), raw_result_files: inputs};
    writeFileSync(join(output, 'statistics.json'), JSON.stringify(summary, null, 2) + '\n', {flag: 'wx'});
    writeFileSync(join(output, 'statistics.md'), markdown(summary), {flag: 'wx'});
    console.log(JSON.stringify({status: summary.status, separate_attribution_candidates: summary.separate_attribution_candidates.length}));
  }
}
