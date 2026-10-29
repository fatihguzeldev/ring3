import assert from "node:assert/strict";
import { CodeCache } from "../../cache.js";

export const moduleBytes = new Uint8Array([
  0, 97, 115, 109, 1, 0, 0, 0, 1, 5, 1, 96, 0, 1, 127, 3, 2, 1, 0, 7, 7, 1, 3, 114, 117, 110, 0, 0,
  10, 6, 1, 4, 0, 65, 0, 11,
]);

const value = (args, index) => BigInt(args[index] >>> 0) | (BigInt(args[index + 1] >>> 0) << 32n);
const pcs = [0x1000, 0x1009, 0x1020, 0x2000];

export function fixture({
  installFailure,
  prepareFailure,
  cancelFailure,
  invalidModule = false,
  acknowledgementFailure,
} = {}) {
  const calls = [];
  const installed = new Map();
  let response,
    nextId = 1n,
    nextSerial = 1n,
    pending,
    cohort;

  function publish(kind, pc, id, slot, tier = 0, victim = 0n) {
    assert.equal(pending, undefined);
    const serial = kind === 0 ? 0n : nextSerial++;
    pending = kind === 0 ? undefined : { kind, pc, id, slot, tier, victim, serial };
    const packet = new DataView(response.buffer);
    packet.setUint32(0, 0x51433352, true);
    packet.setUint32(8, 64, true);
    packet.setUint32(12, 1, true);
    packet.setBigUint64(16, serial, true);
    packet.setUint32(24, kind, true);
    packet.setUint32(28, pc, true);
    packet.setBigUint64(32, id, true);
    packet.setBigUint64(40, victim, true);
    packet.setUint32(48, slot, true);
    packet.setUint32(52, kind === 2 ? slot : 0xffffffff, true);
    packet.setUint32(56, tier, true);
  }

  function eviction() {
    const victim = cohort.victims[cohort.acknowledged];
    publish(2, pcs[0], 0n, victim.slot, 1, victim.id);
  }

  const engine = {
    memory: new WebAssembly.Memory({ initial: 1 }),
    api: { guard_dispatch_entry() {}, find_installed_resident() {} },
    take: (length) => response.slice(0, length),
    ok(operation, ...args) {
      calls.push({ operation, args });
      response = new Uint8Array(80);
      const packet = new DataView(response.buffer);
      if (operation === "dispatcher_module") {
        packet.setUint32(0, 0x50443352, true);
        packet.setUint32(8, 32, true);
        packet.setUint32(24, 1024, true);
        packet.setUint32(28, moduleBytes.length, true);
        new Uint8Array(this.memory.buffer, 1024, moduleBytes.length).set(moduleBytes);
      } else if (operation === "prepare_cache_promotion") {
        assert.equal(cohort, undefined);
        cohort = { victims: pcs.slice(0, 3).map((pc) => installed.get(pc)), acknowledged: 0 };
        eviction();
      } else if (operation === "prepare_cache_entry") {
        const pc = args[2];
        if (cohort) {
          assert.equal(pc, pcs[0]);
          if (cohort.acknowledged === 1 && prepareFailure) throw prepareFailure;
          if (cohort.acknowledged < cohort.victims.length) eviction();
          else publish(1, pc, nextId++, 0, 1);
        } else if (installed.has(pc)) {
          const row = installed.get(pc);
          publish(0, pc, row.id, row.slot);
        } else publish(1, pc, nextId++, installed.size);
      } else if (operation === "acknowledge_cache_eviction") {
        assert.equal(pending.kind, 2);
        assert.equal(value(args, 2), pending.serial);
        assert.equal(value(args, 4), pending.victim);
        assert.equal(args[6], pending.slot);
        assert.equal(cache.table.get(pending.slot), null);
        assert.equal(cache.slots.has(pending.slot), false);
        if (cohort.acknowledged === 1 && acknowledgementFailure) throw acknowledgementFailure;
        const victim = cohort.victims[cohort.acknowledged++];
        assert.equal(installed.get(victim.pc).id, pending.victim);
        installed.delete(victim.pc);
        pending = undefined;
      } else if (operation === "resident_module") {
        assert.equal(value(args, 0), pending.id);
        const bytes = pending.tier === 1 && invalidModule ? new Uint8Array([1, 2, 3]) : moduleBytes;
        new Uint8Array(this.memory.buffer, 2048, bytes.length).set(bytes);
        packet.setUint32(16, 2048, true);
        packet.setUint32(20, bytes.length, true);
      } else if (operation === "install_cached_entry") {
        assert.equal(pending.kind, 1);
        assert.equal(value(args, 2), pending.serial);
        assert.equal(value(args, 4), pending.id);
        assert.equal(args[6], pending.slot);
        assert.equal(cache.slots.get(pending.slot).id, pending.id);
        assert.equal(cache.table.get(pending.slot), cache.slots.get(pending.slot).generated.run);
        if (pending.tier === 1 && installFailure) throw installFailure;
        installed.set(pending.pc, { id: pending.id, slot: pending.slot, pc: pending.pc });
        pending = undefined;
        cohort = undefined;
      } else if (operation === "cancel_cache_request") {
        assert.equal(pending.kind, 1);
        assert.equal(value(args, 2), pending.serial);
        assert.equal(cache.table.get(pending.slot), null);
        assert.equal(cache.slots.has(pending.slot), false);
        if (cancelFailure) throw cancelFailure;
        pending = undefined;
        cohort = undefined;
      } else assert.fail(`unexpected operation ${operation}`);
    },
  };
  const cache = new CodeCache(engine, { slots: pcs.length });
  const before = pcs.map((pc) => cache.entry(pc));
  calls.length = 0;
  return { cache, calls, before, installed, pending: () => pending };
}
