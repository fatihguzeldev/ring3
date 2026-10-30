import { view } from "../engine.js";
import { ActivationCallbacks } from "./diagnostics/activation.js";

const methods = new Map([
  [7, "Acquire"],
  [8, "Unacquire"],
  [9, "GetDeviceState"],
  [10, "GetDeviceData"],
  [13, "SetCooperativeLevel"],
]);
const copy = (record) => record && { ...record, args: [...record.args] };
const d3dMethods = new Map([
  [0x4010f, "CreateDevice"],
  [0x40203, "TestCooperativeLevel"],
  [0x4020d, "CreateAdditionalSwapChain"],
  [0x4020e, "Reset"],
  [0x4020f, "Present"],
  [0x4021f, "SetRenderTarget"],
  [0x40222, "BeginScene"],
  [0x40223, "EndScene"],
  [0x40224, "Clear"],
  [0x40c03, "Present"],
]);
const retainedD3d = new Set([0x4010f, 0x40203, 0x4020d, 0x4020e, 0x4020f, 0x40c03]);
const clockMethods = new Map([
  [0xc0010, "QueryPerformanceCounter"],
  [0xc0011, "QueryPerformanceFrequency"],
]);

export class CompletedWindowsCalls {
  constructor(
    engine,
    { pendingLimit = 128, stickyLimit = 16, now = () => performance.now() } = {},
  ) {
    if (
      !Number.isInteger(pendingLimit) ||
      pendingLimit < 1 ||
      pendingLimit > 128 ||
      !Number.isInteger(stickyLimit) ||
      stickyLimit < 1 ||
      stickyLimit > 16
    )
      throw new RangeError("invalid Windows call diagnostic bounds");
    this.engine = engine;
    this.pendingLimit = pendingLimit;
    this.stickyLimit = stickyLimit;
    this.now = now;
    this.activation = new ActivationCallbacks(now);
    this.pending = [];
    this.directInput = new Map();
    this.d3d = new Map();
    this.clocks = new Map();
    this.negativeResults = [];
    this.droppedReturns = 0;
    this.droppedDirectInput = 0;
    this.droppedD3d = 0;
    this.overwrittenNegativeResults = 0;
    this.malformedCalls = 0;
  }
  capture(receipt, frame, thread) {
    if (
      frame.byteLength !== 112 ||
      frame.getUint32(0, true) !== 0x46433352 ||
      frame.getUint32(4, true) !== 0x10001 ||
      frame.getUint32(8, true) !== 112 ||
      frame.getUint32(12, true) !== 0 ||
      frame.getUint32(20, true) !== receipt.id
    ) {
      this.malformedCalls++;
      return;
    }
    const convention = frame.getUint32(24, true),
      words = frame.getUint32(28, true),
      token = frame.getUint32(16, true);
    if (convention < 1 || convention > 3 || words > 16 || token === 0) {
      this.malformedCalls++;
      return;
    }
    if (this.pending.length === this.pendingLimit) {
      this.pending.shift();
      this.droppedReturns++;
    }
    this.pending.push({
      id: receipt.id,
      pc: receipt.pc,
      token,
      thread,
      args: Array.from({ length: words }, (_, index) => frame.getUint32(48 + index * 4, true)),
      returnPc: frame.getUint32(40, true),
      returnEsp: (frame.getUint32(36, true) + 4 + (convention === 1 ? 0 : words * 4)) >>> 0,
    });
  }
  settle(thread) {
    if (this.pending.length === 0) return;
    const pc = this.engine.get(48),
      esp = this.engine.get(32);
    for (let index = this.pending.length - 1; index >= 0; index--) {
      const call = this.pending[index];
      if (call.thread !== thread || call.returnPc !== pc || call.returnEsp !== esp) continue;
      this.pending.splice(index, 1);
      const result = this.engine.get(16),
        record = {
          ...call,
          result,
          resultSigned: result | 0,
          resultHex: `0x${result.toString(16).padStart(8, "0")}`,
          completedAtMs: this.now(),
        };
      this.lastCompleted = record;
      if (record.resultSigned < 0) {
        if (this.negativeResults.length === 16) {
          this.negativeResults.shift();
          this.overwrittenNegativeResults++;
        }
        this.negativeResults.push(record);
      }
      const tag = (call.id >>> 8) & 255,
        slot = call.id & 255;
      if (call.id >>> 16 === 9 && tag >= 9 && tag <= 16 && methods.has(slot)) {
        const key = `${call.id}:${call.args[0]}`;
        this.directInput.delete(key);
        if (this.directInput.size === this.stickyLimit) {
          this.directInput.delete(this.directInput.keys().next().value);
          this.droppedDirectInput++;
        }
        this.directInput.set(key, { ...record, method: methods.get(slot) });
      }
      if (call.id >>> 16 === 4) {
        const key = `${call.id}:${call.args[0]}`;
        this.d3d.delete(key);
        if (this.d3d.size === 32) {
          const candidate = [...this.d3d].find(([, value]) => !retainedD3d.has(value.id));
          this.d3d.delete(candidate?.[0] ?? this.d3d.keys().next().value);
          this.droppedD3d++;
        }
        this.d3d.set(key, { ...record, method: d3dMethods.get(call.id) });
      }
      if (clockMethods.has(call.id)) {
        const previous = this.clocks.get(call.id);
        if (!previous || record.completedAtMs - previous.completedAtMs >= 1000)
          this.clocks.set(call.id, { ...record, method: clockMethods.get(call.id) });
      }
      return;
    }
  }
  sampleInput({ closed = false } = {}) {
    if (closed || typeof this.engine.api.input_state !== "function") return;
    try {
      this.engine.ok("input_state");
      const packet = view(this.engine.take(96));
      if (
        packet.byteLength !== 96 ||
        packet.getUint32(0, true) !== 0x524d3352 ||
        packet.getUint32(4, true) !== 0x90001 ||
        packet.getUint32(8, true) !== 64 ||
        packet.getUint32(12, true) !== 0 ||
        packet.getBigUint64(16, true) === 0n ||
        packet.getBigUint64(24, true) > 3n
      )
        throw new Error("invalid input state descriptor");
      const flags = packet.getBigUint64(24, true);
      const pressedKeys = [];
      let pressedKeyCount = 0;
      for (let code = 0; code < 256; code++) {
        if (!(packet.getUint8(64 + (code >>> 3)) & (1 << (code & 7)))) continue;
        pressedKeyCount++;
        if (pressedKeys.length < 16) pressedKeys.push(code);
      }
      this.inputState = {
        effectiveFocused: !!(flags & 1n),
        effectiveCaptured: !!(flags & 2n),
        epoch: packet.getBigUint64(16, true).toString(),
        pressedKeys,
        pressedKeyCount,
        clippedKeys: pressedKeyCount - pressedKeys.length,
        sampledAtMs: this.now(),
      };
    } catch (error) {
      this.inputState = { error: error.message, sampledAtMs: this.now() };
    }
  }
  snapshot() {
    return {
      clock: "worker performance.now milliseconds",
      coverage: "JavaScript host-returned calls only; excludes v86 inline and Wasm-completed calls, whose aggregate counts remain in CPU stats",
      lastCompletedWindowsCall: copy(this.lastCompleted),
      directInputCalls: [...this.directInput.values()].map(copy),
      recentD3DCalls: [...this.d3d.values()].map(copy),
      recentNegativeWindowsCalls: this.negativeResults.map(copy),
      clockCalls: [...this.clocks.values()].map(copy),
      clockOutputRead: false,
      inputState: this.inputState && {
        ...this.inputState,
        pressedKeys: this.inputState.pressedKeys?.slice(),
      },
      pendingReturns: this.pending.length,
      droppedReturns: this.droppedReturns,
      droppedDirectInput: this.droppedDirectInput,
      droppedD3D: this.droppedD3d,
      overwrittenNegativeResults: this.overwrittenNegativeResults,
      malformedCalls: this.malformedCalls,
      activationCallbacks: this.activation.snapshot(),
    };
  }
}
