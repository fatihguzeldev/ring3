export class AudioHost {
  constructor({onDiagnostic = () => {}, onStats = () => {}} = {}) {
    this.onDiagnostic = onDiagnostic;
    this.onStats = onStats;
    this.context = null;
    this.node = null;
    this.pending = new Map();
    this.volume = 0.75;
    this.muted = false;
  }
  async start() {
    if (this.context) { await this.context.resume(); return; }
    const Context = window.AudioContext ?? window.webkitAudioContext;
    if (!Context) throw new Error('Web Audio is unavailable in this browser');
    const context = new Context({sampleRate:48000, latencyHint:'interactive'});
    this.context = context;
    try {
      if (context.sampleRate !== 48000) throw new Error(`the PCM host requires 48000 Hz; browser selected ${context.sampleRate}`);
      const resumed = context.resume();
      await context.audioWorklet.addModule(new URL('./processor.js', import.meta.url));
      this.node = new AudioWorkletNode(context, 'ring3-playground-pcm', {numberOfInputs:0, numberOfOutputs:1, outputChannelCount:[2]});
      this.gain = context.createGain();
      this.setVolume(this.volume, this.muted);
      this.node.connect(this.gain).connect(context.destination);
      this.node.port.onmessage = ({data}) => {
        if (data.type === 'stats') { this.onStats(data); return; }
        const key = String(data.sequence), pending = this.pending.get(key);
        if (!pending) return;
        this.pending.delete(key);
        if (data.type === 'accepted') pending.resolve(data);
        else pending.reject(new Error(data.message ?? 'PCM output rejected'));
      };
      this.node.onprocessorerror = () => this.fail(new Error('audio worklet failed'));
      await resumed;
      this.onDiagnostic(`audio output ready: ${context.sampleRate} Hz stereo`);
    } catch (error) {
      await this.close();
      throw error;
    }
  }
  accept({samples, channels, sampleRate, frames, sequence}) {
    if (!this.node || !this.context) return Promise.reject(new Error('PCM arrived before audio output was ready'));
    if (!(samples instanceof Float32Array) || channels !== 2 || sampleRate !== 48000 || samples.length !== frames * channels || frames < 1 || frames > 4096) return Promise.reject(new Error('invalid engine PCM shape'));
    for (const sample of samples) if (!Number.isFinite(sample)) return Promise.reject(new Error('non-finite engine PCM sample'));
    const key = String(sequence);
    if (this.pending.has(key) || this.pending.size >= 2) return Promise.reject(new Error('overlapping engine PCM sequence'));
    return new Promise((resolve, reject) => {
      this.pending.set(key, {resolve, reject});
      this.node.port.postMessage({type:'pcm', sequence, samples}, [samples.buffer]);
    });
  }
  setVolume(value, muted = this.muted) {
    this.volume = Math.max(0, Math.min(1, Number(value)));
    this.muted = muted;
    if (this.gain) this.gain.gain.value = muted ? 0 : this.volume;
  }
  async pause() { if (this.context) await this.context.suspend(); }
  async resume() { if (this.context) await this.context.resume(); }
  fail(error) {
    for (const entry of this.pending.values()) entry.reject(error);
    this.pending.clear();
    this.onDiagnostic(error.message);
  }
  async close() {
    this.fail(new Error('audio output closed'));
    this.node?.port.close();
    this.node?.disconnect();
    const context = this.context;
    this.node = this.context = this.gain = null;
    if (context && context.state !== 'closed') await context.close();
  }
}
