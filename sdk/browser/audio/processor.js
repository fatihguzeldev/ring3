class PlaygroundPcm extends AudioWorkletProcessor {
  constructor() {
    super();
    this.ring = new Float32Array(48000 * 2 / 5);
    this.read = 0;
    this.write = 0;
    this.available = 0;
    this.pending = null;
    this.started = false;
    this.underruns = 0;
    this.lastReport = 0;
    this.port.onmessage = ({data}) => {
      if (data.type === 'clear') {
        this.available = this.read = this.write = 0;
        this.pending = null;
        this.started = false;
        return;
      }
      if (data.type !== 'pcm') return;
      if (!(data.samples instanceof Float32Array) || data.samples.length === 0 || data.samples.length % 2 || data.samples.length > this.ring.length || this.pending) {
        this.port.postMessage({type:'error', sequence:data.sequence, message:'invalid or overlapping PCM submission'});
        return;
      }
      this.pending = data;
      this.acceptPending();
    };
  }
  acceptPending() {
    const packet = this.pending;
    if (!packet || packet.samples.length > this.ring.length - this.available) return;
    const first = Math.min(packet.samples.length, this.ring.length - this.write);
    this.ring.set(packet.samples.subarray(0, first), this.write);
    this.ring.set(packet.samples.subarray(first), 0);
    this.write = (this.write + packet.samples.length) % this.ring.length;
    this.available += packet.samples.length;
    this.pending = null;
    this.started = true;
    this.port.postMessage({type:'accepted', sequence:packet.sequence, queuedFrames:this.available / 2});
  }
  process(_inputs, outputs) {
    const output = outputs[0];
    if (!output?.length) return true;
    const left = output[0], right = output[1] ?? left;
    let missing = false;
    for (let i = 0; i < left.length; i++) {
      if (this.available >= 2) {
        left[i] = this.ring[this.read];
        right[i] = this.ring[(this.read + 1) % this.ring.length];
        this.read = (this.read + 2) % this.ring.length;
        this.available -= 2;
      } else {
        left[i] = right[i] = 0;
        missing = true;
      }
    }
    if (missing && this.started) this.underruns++;
    this.acceptPending();
    if (currentFrame - this.lastReport >= sampleRate) {
      this.lastReport = currentFrame;
      this.port.postMessage({type:'stats', queuedFrames:this.available / 2, underruns:this.underruns});
    }
    return true;
  }
}
registerProcessor('ring3-playground-pcm', PlaygroundPcm);
