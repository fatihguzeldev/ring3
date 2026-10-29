export class ActivationCallbacks {
  constructor(now) {
    this.now = now;
    this.records = [];
    this.overwritten = 0;
  }
  begin(frame, thread) {
    if (frame.byteLength !== 96 || frame.getUint32(0, true) !== 0x43573352 ||
        frame.getUint32(4, true) !== 0x10001 || frame.getUint32(8, true) !== 96 ||
        frame.getUint32(12, true) !== 1 || frame.getUint32(20, true) !== 4 ||
        frame.getUint32(36, true) !== 0x1c) return;
    const record = {
      thread,
      procedure: frame.getUint32(16, true),
      continuation: frame.getBigUint64(24, true).toString(),
      window: frame.getUint32(32, true),
      active: frame.getUint32(40, true),
      otherThread: frame.getUint32(44, true),
      startedAtMs: this.now(),
    };
    if (this.records.length === 16) { this.records.shift(); this.overwritten++; }
    this.records.push(record);
    return record;
  }
  complete(record) {
    if (record && record.completedAtMs === undefined) record.completedAtMs = this.now();
  }
  snapshot() {
    return { records: this.records.map(record => ({...record})), overwritten: this.overwritten };
  }
}
