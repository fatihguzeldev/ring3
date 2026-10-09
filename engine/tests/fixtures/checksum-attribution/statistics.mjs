import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {readFileSync, writeFileSync} from 'node:fs';
import {dirname, isAbsolute, join, relative, resolve} from 'node:path';
import {fileURLToPath} from 'node:url';

const KINDS = ['direct', 'dispatch', 'guard'];
const BLOCKS = [1, 8];
const ROUNDS = 32;
const MAX_NS = 60_000_000_000n;
const REPO = resolve(dirname(fileURLToPath(import.meta.url)), '../../../..');

export function parseNs(value) {
  assert.equal(typeof value, 'string', 'duration must be a decimal string');
  assert.match(value, /^[1-9][0-9]*$/, 'duration must be positive canonical nanoseconds');
  const ns = BigInt(value);
  assert.ok(ns <= MAX_NS, 'duration exceeds the child deadline');
  return Number(ns);
}

export function describe(values) {
  assert.ok(Array.isArray(values) && values.length > 0);
  assert.ok(values.every(value => Number.isFinite(value) && value > 0));
  const sorted = [...values].sort((a, b) => a - b);
  const quantile = probability => {
    const position = (sorted.length - 1) * probability;
    const low = Math.floor(position), high = Math.ceil(position);
    return sorted[low] + (position - low) * (sorted[high] - sorted[low]);
  };
  const q1 = quantile(0.25), q3 = quantile(0.75);
  return {n: sorted.length, min: sorted[0], q1, median: quantile(0.5), q3,
    max: sorted.at(-1), iqr: q3 - q1};
}

function index(value, maximum, label) {
  assert.ok(Number.isSafeInteger(value) && value >= 0 && value < maximum, `${label}: invalid index`);
}

function object(value, label) {
  assert.ok(value && typeof value === 'object' && !Array.isArray(value), `${label}: expected object`);
}

export function summarizeProcess(result, journal) {
  object(result, 'result');
  assert.equal(result.schema_version, 1);
  assert.equal(result.status, 'complete');
  index(result.process_index, 3, 'process');
  assert.ok(Array.isArray(result.events) && Array.isArray(result.samples) && Array.isArray(journal));
  assert.equal(result.samples.length, 192, 'expected six series of 32 samples');
  assert.deepEqual(journal.filter(row => row.kind === 'event').map(row => row.event), result.events,
    'physical journal and result event records differ');
  const events = new Map();
  for (const event of result.events) {
    object(event, 'event');
    assert.ok(!events.has(event.action_id), 'duplicate event ID');
    events.set(event.action_id, event);
  }
  const timed = result.events.filter(event => event.timed === true);
  assert.equal(timed.length, 192, 'timed event census differs');
  assert.ok(timed.every(event => ['call', 'guard'].includes(event.kind)), 'unexpected timed event kind');
  const usedEvents = new Set(), groups = new Map(), owners = new Map();
  for (const sample of result.samples) {
    object(sample, 'sample');
    assert.deepEqual(Object.keys(sample).sort(),
      ['process', 'blocks', 'kind', 'round', 'ns', 'calls', 'status_or', 'event', 'context'].sort());
    assert.equal(sample.process, result.process_index);
    assert.ok(BLOCKS.includes(sample.blocks));
    assert.ok(KINDS.includes(sample.kind));
    index(sample.round, ROUNDS, 'round');
    assert.ok(Number.isSafeInteger(sample.context) && sample.context > 0);
    assert.ok(Number.isSafeInteger(sample.event) && sample.event >= 0);
    assert.equal(sample.calls, sample.kind === 'guard' ? 1024 : 1);
    assert.equal(sample.status_or, 0, 'sample returned nonzero status');
    parseNs(sample.ns);
    const event = events.get(sample.event);
    assert.ok(event && event.timed === true, 'sample has no timed event');
    assert.equal(event.kind, sample.kind === 'guard' ? 'guard' : 'call');
    assert.equal(event.channel, sample.kind);
    assert.equal(event.context, sample.context);
    assert.equal(event.blocks, sample.blocks);
    assert.equal(event.round, sample.round);
    assert.equal(event.count, sample.calls);
    assert.equal(event.status_or, sample.status_or);
    assert.equal(event.ns, sample.ns, 'sample duration differs from physical event');
    assert.ok(!usedEvents.has(sample.event), 'event used by more than one sample');
    usedEvents.add(sample.event);
    if (owners.has(sample.blocks)) assert.equal(owners.get(sample.blocks), sample.context, 'partition changed owner');
    owners.set(sample.blocks, sample.context);
    const key = `${sample.kind}/${sample.blocks}`, group = groups.get(key) ?? new Map();
    assert.ok(!group.has(sample.round), 'duplicate series round');
    group.set(sample.round, sample); groups.set(key, group);
  }
  assert.equal(new Set(owners.values()).size, 2, 'partitions must use distinct owners');
  assert.ok(timed.every(event => usedEvents.has(event.action_id)), 'timed event missing its sample');
  const series = [];
  for (const kind of KINDS) for (const blocks of BLOCKS) {
    const group = groups.get(`${kind}/${blocks}`);
    assert.equal(group?.size, ROUNDS, 'series has missing rounds');
    const rows = Array.from({length: ROUNDS}, (_, round) => group.get(round));
    assert.ok(rows.every(Boolean), 'round census is incomplete');
    const values = rows.map(row => parseNs(row.ns)), calls = kind === 'guard' ? 1024 : 1;
    series.push({kind, blocks, context: owners.get(blocks), calls_per_sample: calls,
      raw_samples: rows, window_ns: describe(values),
      [kind === 'guard' ? 'ns_per_export' : 'ns_per_call']: describe(values.map(ns => ns / calls)),
      ...(kind === 'guard' ? {} : {
        expected_instructions_per_call: 1283,
        ns_per_expected_instruction: describe(values.map(ns => ns / 1283)),
      }),
    });
  }
  object(result.first_use, 'first-use metadata');
  parseNs(result.first_use.engine_read_ns); parseNs(result.first_use.engine_module_ns);
  return {process: result.process_index, series, first_use: result.first_use,
    environment: result.environment};
}

export function blockEffect(kind, processes) {
  assert.ok(KINDS.includes(kind));
  assert.equal(processes.length, 3);
  const effects = processes.map((process, index) => {
    assert.equal(process.process, index, 'processes must be separately ordered');
    const a = process.series.find(row => row.kind === kind && row.blocks === 1)?.window_ns;
    const b = process.series.find(row => row.kind === kind && row.blocks === 8)?.window_ns;
    assert.ok(a && b);
    for (const row of [a, b]) assert.ok(
      [row.q1, row.median, row.q3].every(value => Number.isFinite(value) && value > 0)
      && row.q1 <= row.median && row.median <= row.q3);
    return {process: index, relative_effect: (b.median - a.median) / a.median,
      strictly_separated_iqr: a.q3 < b.q1 || b.q3 < a.q1};
  });
  const direction = Math.sign(effects[0].relative_effect);
  const qualifies = direction !== 0 && effects.every(row =>
    Math.sign(row.relative_effect) === direction && Math.abs(row.relative_effect) >= 0.1
      && row.strictly_separated_iqr);
  return {kind, a_blocks: 1, b_blocks: 8, processes: effects,
    qualifies_for_separate_attribution: qualifies};
}

function within(directory, name) {
  assert.equal(typeof name, 'string'); assert.ok(!isAbsolute(name));
  const target = resolve(directory, name), difference = relative(directory, target);
  assert.ok(difference && !difference.startsWith('..') && !isAbsolute(difference), 'path escapes evidence directory');
  return target;
}

function pin(bytes) {
  return {bytes: bytes.length, sha256: createHash('sha256').update(bytes).digest('hex')};
}

function physical(directory, record) {
  object(record, 'physical file receipt');
  const bytes = readFileSync(within(directory, record.file));
  assert.deepEqual(pin(bytes), {bytes: record.bytes, sha256: record.sha256}, 'physical file receipt differs');
  return bytes;
}

function json(bytes) {return JSON.parse(bytes.toString('utf8'));}

export function readCampaign(directory) {
  const campaignBytes = readFileSync(join(directory, 'campaign.json')), campaign = json(campaignBytes);
  const semanticBytes = readFileSync(join(directory, 'semantic-check.json')), semantic = json(semanticBytes);
  assert.equal(campaign.schema_version, 1); assert.equal(campaign.status, 'complete', 'no decisions for incomplete campaigns');
  assert.equal(semantic.schema_version, 1); assert.equal(semantic.complete, true);
  assert.equal(semantic.semantic_certification, true, 'independent semantic certification is required');
  assert.equal(semantic.campaign_sha256, pin(campaignBytes).sha256, 'semantic receipt belongs to another campaign');
  assert.equal(campaign.children.length, 3); assert.equal(semantic.processes.length, 3);
  object(campaign.engine, 'campaign engine'); object(campaign.source_pins, 'campaign sources');
  assert.equal(campaign.engine.bytes, 586988);
  assert.equal(campaign.engine.sha256, '4d5e42c9e4adbcb774827f0a6c99687e5c9aed5b4a5ed34c8348bb4b62cf1bf2');
  assert.deepEqual(pin(readFileSync(campaign.engine.path)),
    {bytes: campaign.engine.bytes, sha256: campaign.engine.sha256}, 'release artifact changed');
  const paths = Object.keys(campaign.source_pins).sort();
  for (const name of ['plan.mjs', 'run.mjs', 'check.mjs', 'statistics.mjs', 'measure.py']) {
    assert.ok(paths.includes(`engine/tests/fixtures/checksum-attribution/${name}`), 'missing fixture source pin');
  }
  for (const dependency of ['resident-phases/plan.mjs', 'resident-phases/measure.py', 'p2-pe32-sort/oracle.mjs']) {
    assert.ok(paths.includes(`engine/tests/fixtures/${dependency}`), 'missing shared dependency pin');
  }
  const expectedPins = paths.map(path => {
    const expected = campaign.source_pins[path];
    assert.deepEqual(pin(readFileSync(within(REPO, path))), expected, `frozen source changed: ${path}`);
    return {path, ...expected};
  });
  const processes = [], inputs = [];
  for (let process = 0; process < 3; process++) {
    const binding = campaign.children[process], check = semantic.processes[process];
    assert.equal(binding.process_index, process); assert.equal(check.process_index, process);
    assert.equal(binding.status, 'complete'); assert.equal(binding.exit_code, 0); assert.equal(binding.timed_out, false);
    assert.equal(check.complete, true);
    const file = within(directory, binding.result_file), resultBytes = readFileSync(file), result = json(resultBytes);
    assert.deepEqual(pin(resultBytes), binding.result_pin, 'result differs from supervisor receipt');
    assert.equal(pin(resultBytes).sha256, check.result_sha256, 'result differs from semantic receipt');
    assert.equal(result.process_index, process);
    assert.deepEqual(result.engine, {bytes: campaign.engine.bytes, sha256: campaign.engine.sha256});
    assert.deepEqual(result.source_pins_before, expectedPins); assert.deepEqual(result.source_pins_after, expectedPins);
    const raw = physical(dirname(file), result.raw);
    assert.equal(pin(raw).sha256, check.raw_sha256, 'arena evidence differs from semantic receipt');
    const journalBytes = physical(dirname(file), result.journal);
    const lines = journalBytes.toString('utf8').split('\n');
    assert.equal(lines.pop(), '', 'journal must retain its complete final line');
    assert.equal(lines.length, result.journal.rows);
    const journal = lines.map(line => JSON.parse(line));
    processes.push(summarizeProcess(result, journal));
    inputs.push({process, file: binding.result_file, ...pin(resultBytes), journal: result.journal, raw: result.raw});
  }
  const comparisons = KINDS.map(kind => blockEffect(kind, processes));
  return {schema_version: 1, status: 'complete', engine: campaign.engine, source_pins: campaign.source_pins,
    metadata: campaign.metadata, processes, comparisons,
    separate_attribution_candidates: comparisons.filter(row => row.qualifies_for_separate_attribution),
    provenance: {campaign: pin(campaignBytes), semantic_check: pin(semanticBytes), inputs},
    policy: {rounds: ROUNDS, quantile: 'linear interpolation at (n-1)*p', primary_gates: 3,
      relative_threshold: 0.1, rule: 'same direction, >=10% median effect and strict IQR separation in all three processes',
      pooling: false, subtraction: false, ratio_gate: false,
      limits: ['authored checksum kernel only', 'block count changes membership/safepoints/V8 layout',
        'rotation balances position but not every carryover pair', 'GC/JIT/host scheduling/frequency and per-call clock overhead',
        'guard loop timing is not pure Wasm cost and cannot be subtracted', 'descriptive heuristic, not confidence or causal attribution',
        'no regression, optimization or representative game performance claim']}};
}

export function markdown(summary) {
  const lines = ['# Checksum block attribution', '',
    'All three processes are complete and bound to the independent semantic receipt. Each execution sample contains one individually observed 1,283-instruction checksum call.', '',
    '| Process | Blocks | Kind | Samples | Median ns/call or export | IQR | Min | Max |',
    '| --- | --- | --- | --- | --- | --- | --- | --- |'];
  const number = value => value.toFixed(3);
  for (const process of summary.processes) for (const series of process.series) {
    const value = series.ns_per_call ?? series.ns_per_export;
    lines.push(`| ${process.process} | ${series.blocks} | ${series.kind} | ${value.n} | ${number(value.median)} | ${number(value.iqr)} | ${number(value.min)} | ${number(value.max)} |`);
  }
  lines.push('', '## Three descriptive block-effect gates', '',
    '| Kind | Process 0 effect | Process 1 effect | Process 2 effect | Qualifies |',
    '| --- | --- | --- | --- | --- |');
  for (const gate of summary.comparisons) lines.push(`| ${gate.kind} | ${gate.processes.map(row => `${number(row.relative_effect * 100)}%`).join(' | ')} | ${gate.qualifies_for_separate_attribution ? 'yes' : 'no'} |`);
  lines.push('', 'Every raw sample, quartile and strict-IQR result is retained in statistics.json. Processes remain separate; no overhead subtraction or ratio gate is applied.', '',
    'A signal supports a separate attribution decision only. Shared execution composition, entry currency, helper interaction and V8 layout remain possible contributors. This kernel does not establish representative application/game cost or justify optimization.', '');
  return lines.join('\n');
}

function synthetic(process) {
  const samples = [], events = [];
  for (const kind of KINDS) for (const blocks of BLOCKS) for (let round = 0; round < ROUNDS; round++) {
    const action = events.length, context = process * 2 + (blocks === 1 ? 1 : 2);
    const ns = String(1000 + round + (blocks === 8 ? 200 : 0));
    const sample = {process, blocks, kind, round, ns, calls: kind === 'guard' ? 1024 : 1,
      status_or: 0, event: action, context};
    samples.push(sample); events.push({action_id: action, context, kind: kind === 'guard' ? 'guard' : 'call',
      channel: kind, count: sample.calls, blocks, round, timed: true, ns, status_or: 0});
  }
  const result = {schema_version: 1, process_index: process, status: 'complete', samples, events,
    first_use: {engine_read_ns: '1', engine_module_ns: '2'}, environment: {synthetic: true}};
  return {result, journal: events.map(event => ({kind: 'event', event: structuredClone(event)}))};
}

function selfTest() {
  assert.deepEqual(describe(Array.from({length: 32}, (_, index) => index + 1)),
    {n: 32, min: 1, q1: 8.75, median: 16.5, q3: 24.25, max: 32, iqr: 15.5});
  for (const value of ['0', '-1', '1.5', 'NaN', '01', 1, '60000000001']) assert.throws(() => parseNs(value));
  const fixtures = Array.from({length: 3}, (_, process) => synthetic(process));
  const processes = fixtures.map(({result, journal}) => summarizeProcess(result, journal));
  assert.equal(processes[0].series.length, 6);
  assert.ok(Math.abs(processes[0].series.find(row => row.kind === 'direct').ns_per_expected_instruction.median
    - 1015.5 / 1283) < Number.EPSILON);
  assert.ok(KINDS.every(kind => blockEffect(kind, processes).qualifies_for_separate_attribution));
  const changed = structuredClone(fixtures[0]); changed.result.samples[0].ns = '999';
  assert.throws(() => summarizeProcess(changed.result, changed.journal), /duration differs/);
  const journalChanged = structuredClone(fixtures[0]); journalChanged.journal[0].event.ns = '999';
  assert.throws(() => summarizeProcess(journalChanged.result, journalChanged.journal), /journal and result/);
  const missing = structuredClone(fixtures[0]); missing.result.samples.pop();
  assert.throws(() => summarizeProcess(missing.result, missing.journal), /six series/);
  const duplicate = structuredClone(fixtures[0]); duplicate.result.samples[1] = duplicate.result.samples[0];
  assert.throws(() => summarizeProcess(duplicate.result, duplicate.journal), /more than one/);
  const invalid = structuredClone(fixtures[0]); invalid.result.samples[0].calls = 2;
  assert.throws(() => summarizeProcess(invalid.result, invalid.journal));
  const touching = structuredClone(processes);
  for (const process of touching) {
    process.series.find(row => row.kind === 'direct' && row.blocks === 8).window_ns.q1 =
      process.series.find(row => row.kind === 'direct' && row.blocks === 1).window_ns.q3;
  }
  assert.equal(blockEffect('direct', touching).qualifies_for_separate_attribution, false);
  const opposite = structuredClone(processes);
  const reversed = opposite[2].series.find(row => row.kind === 'direct' && row.blocks === 8);
  reversed.window_ns = describe([500, 501, 502]);
  assert.equal(blockEffect('direct', opposite).qualifies_for_separate_attribution, false);
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  assert.equal(process.argv.length, 3, 'usage: statistics.mjs OUTPUT_CAMPAIGN_DIR or --self-test');
  if (process.argv[2] === '--self-test') {
    selfTest(); console.log('checksum statistics self-test: PASS');
  } else {
    const output = resolve(process.argv[2]), summary = readCampaign(output);
    writeFileSync(join(output, 'statistics.json'), JSON.stringify(summary, null, 2) + '\n', {flag: 'wx'});
    writeFileSync(join(output, 'statistics.md'), markdown(summary), {flag: 'wx'});
    console.log(JSON.stringify({status: summary.status, primary_gates: summary.comparisons.length,
      candidates: summary.separate_attribution_candidates.length}));
  }
}
