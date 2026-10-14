import {controls, incomingFlags, savedRAM} from './images.mjs';

export function ordinary({h, record, words, exit, PC, observed, open, compile, observe, wantEntered,
  taskHelpers, call, bound, cloneRAM, writeRAM}) {
  const firstGenerated = observed.generatedInvocations;
  let contexts = 0, compiles = 0, requests = 0;
  function request(c, label, name, input, payload) {
    const want = Buffer.from(input); want.set(payload, 140);
    const material = {run: () => {const status = c.api[name](); c.calls.push(call(name, [], status)); return status;}};
    observed.invocations++; observed.hostQueryAndRefusalInvocations++; requests++;
    const row = h.observe(c, label, {material, input, want, helpers: [call(name, [])], kind: 'task-query-export'});
    row.classification = 'executed-host-contract'; return row;
  }
  for (const resident of [false, true]) for (const entries of [false, true]) {
    for (const code of ['cf', '66cf']) {
      const {c, input, row} = open(resident, entries, {code, label: 'PG0 dword task return ' + code}); contexts++;
      const material = compile(c, row.label, row.code, false); compiles++;
      if (!resident && !entries && code === 'cf' && material) {
        const name = 'cpu_task_return', args = [0x1002];
        const direct = {run: () => {const status = c.api[name](...args); c.calls.push(call(name, args, status)); return status;}};
        observed.invocations++; observed.hostQueryAndRefusalInvocations++; requests++;
        const refusal = h.observe(c, 'wrong nextPC from another IRET length is neutral before real task commit',
          {material: direct, input, want: input, status: 7, helpers: [call(name, args, 7)], kind: 'task-helper-refusal'});
        refusal.classification = 'executed-host-contract';
      }
      const beforeRAM = h.ram(c), wantRAM = savedRAM(cloneRAM, writeRAM, beforeRAM, row.current, code, row.oldRegisters);
      const want = wantEntered(input, row);
      const entered = observe(c, 'real ' + code + ' commits all incoming state; old TF does not trap', material,
        input, want, {wantRAM, helpers: taskHelpers(c, material, row)});
      if (!entered?.passed) {
        h.skip(c, 'stale old task revision', 'completed task transition');
        if (code === 'cf') h.skip(c, 'live query and busy clear', 'completed task transition');
        h.host(c, 'close'); continue;
      }
      // Use the same CS key and source PC: this isolates the retained task revision from decode-key admission.
      const samePC = Buffer.from(want); samePC.writeUInt32LE(PC, 48);
      observe(c, 'actual old emitted guard rejects old revision with same CS and PC', material,
        samePC, samePC, {status: 3});
      if (code === 'cf') {
        request(c, 'live controls retain PG0 CR3 and load LDTR/TS/unblocked NMI', 'read_task_controls', want,
          controls(record, {cr0: 0x3b, ldtr: 0x50, nmi: 1}));
        const contextWant = request(c, 'raw task query serializes live incoming state and revision2',
          'read_task_context', want, row.context(want));
        if (!contextWant?.passed) h.skip(c, 'fresh live-register query', 'task-context query');
        else {
          const queryInput = Buffer.from(want); queryInput.set(row.context(want), 140); queryInput.writeUInt32LE(0x2100, 48);
          h.upload(c, 0x2100, Buffer.from('0f00c90f00c20f01e3ebfe', 'hex'));
          h.put(c, 0, queryInput); observed.compiles++; compiles++;
          const query = h.compile(c, 'fresh revision projects STR ECX, SLDT EDX, SMSW EBX', [[0x2100, 11]], []);
          const queryWant = Buffer.from(queryInput);
          queryWant.writeUInt32LE(0x4b, 20); queryWant.writeUInt32LE(0x50, 24);
          queryWant.writeUInt32LE(0xddee003b, 28); queryWant.writeUInt32LE(0x2109, 48);
          exit(queryWant, 1, 3, 1);
          const queried = observe(c, 'fresh module reads live TR, LDTR and CR0.TS', query, queryInput, queryWant,
            {budget: 3});
          if (!queried?.passed) h.skip(c, 'busy clear visible by LAR', 'live query');
          else {
            request(c, 'task-context query uses live GPRs and PC after real query instructions',
              'read_task_context', queryWant, row.context(queryWant));
            const busyInput = Buffer.from(queryWant); busyInput.set(row.context(queryWant), 140);
            busyInput.writeUInt32LE(0x2200, 48);
            h.upload(c, 0x2200, Buffer.from('b9400000000f02c1b9480000000f02d1ebfe', 'hex'));
            h.put(c, 0, busyInput); observed.compiles++; compiles++;
            const integer = 'cpu_integer' + (resident ? '_resident' : '');
            const busy = h.compile(c, 'fresh LAR observes old busy clear and retained target busy', [[0x2200, 18]], [integer]);
            const busyWant = Buffer.from(busyInput);
            busyWant.writeUInt32LE(0xe900, 16); busyWant.writeUInt32LE(0x48, 20);
            busyWant.writeUInt32LE(0xeb00, 24); busyWant.writeUInt32LE(0x2210, 48);
            busyWant.set(record('R3MH', 40, [], 7), 100);
            busyWant.set(words([0xeb00, incomingFlags, 0xe900, 0x50, 0xddee003b, 0x48]), 140);
            exit(busyWant, 1, 4, 6);
            observe(c, 'CPL3-visible LAR old TSS type9 versus target type11', busy, busyInput, busyWant,
              {budget: 4, helpers: [call(integer, bound(c, busy, [0x30003, 0])),
                call(integer, bound(c, busy, [0x30003, 0]))]});
          }
        }
      }
      h.host(c, 'close');
    }
    const {c, input, row} = open(resident, entries,
      {code: 'cf', current: 0xfe0, label: 'committed outgoing save overwrites source code'}); contexts++;
    const material = compile(c, row.label, row.code, false); compiles++;
    const want = wantEntered(input, row), wantRAM = savedRAM(cloneRAM, writeRAM, h.ram(c), row.current, row.code, row.oldRegisters);
    const entered = observe(c, 'source SMC returns11 after incoming task and save commit', material, input, want,
      {status: 11, wantRAM, helpers: taskHelpers(c, material, row, 11)});
    if (entered?.passed) {
      observe(c, 'actual old emitted guard rejects consumed source after committed SMC', material, want, want,
        {status: 4});
    } else h.skip(c, 'SMC old source rejection', 'committed source SMC');
    h.host(c, 'close');
  }
  return {contexts, compiles, generated: observed.generatedInvocations - firstGenerated, requests};
}
