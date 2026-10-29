import { require } from "../packet.js";

export class ClosedEpochs {
  constructor(maxIntervals = 4096) {
    this.maxIntervals = maxIntervals;
    this.intervals = [];
  }
  has(epoch) {
    return this.intervals.some(([first, last]) => first <= epoch && epoch <= last);
  }
  add(epoch) {
    let start = 0;
    while (start < this.intervals.length && this.intervals[start][1] + 1n < epoch) start++;
    let end = start,
      first = epoch,
      last = epoch;
    while (end < this.intervals.length && this.intervals[end][0] <= last + 1n) {
      first = first < this.intervals[end][0] ? first : this.intervals[end][0];
      last = last > this.intervals[end][1] ? last : this.intervals[end][1];
      end++;
    }
    require(start !== end ||
      this.intervals.length < this.maxIntervals, "retired epoch interval budget exceeded");
    this.intervals.splice(start, end - start, [first, last]);
  }
  clear() {
    this.intervals.length = 0;
  }
}
