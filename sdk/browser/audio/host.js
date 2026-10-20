export class AudioHost {
  constructor({onDiagnostic = () => {}, onStats = () => {}} = {}) {
    this.onDiagnostic = onDiagnostic;
    this.onStats = onStats;
    this.context = null;
    this.node = null;
    this.pending = new Map();
    this.controls = new Map();
    this.request = 0;
    this.epoch = null;
    this.sequence = 1n;
    this.firstFrame = 0n;
    this.failure = null;
    this.acceptedFrames = 0;
    this.renderedFrames = 0;
    this.volume = 0.75;
    this.muted = false;
  }
  async start() {
    if (this.context) { await this.resume(); return; }
    const Context = window.AudioContext ?? window.webkitAudioContext;
    if (!Context) throw new Error('Web Audio is unavailable in this browser');
    const context = new Context({sampleRate:48000, latencyHint:'interactive'});
    this.context = context;
    try {
      if (context.sampleRate !== 48000) throw new Error(`the PCM host requires 48000 Hz; browser selected ${context.sampleRate}`);
      const resumed = context.resume();
      await context.audioWorklet.addModule(new URL('./processor.js', import.meta.url));
      const node = new AudioWorkletNode(context, 'ring3-playground-pcm', {numberOfInputs:0, numberOfOutputs:1, outputChannelCount:[2],
        processorOptions:{capacityFrames:1024,primeFrames:512}});
      this.node = node;
      this.failure = null;
      this.acceptedFrames = this.renderedFrames = 0;
      this.gain = context.createGain();
      this.setVolume(this.volume, this.muted);
      this.node.connect(this.gain).connect(context.destination);
      node.port.onmessage = ({data}) => {if (this.node === node) this.receive(data);};
      node.onprocessorerror = () => {if (this.node === node) this.fail(new Error('audio worklet failed'));};
      await resumed;
      this.onDiagnostic(`audio output ready: ${context.sampleRate} Hz stereo`);
    } catch (error) {
      await this.close();
      throw error;
    }
  }
  accept({samples, channels, sampleRate, frames, epoch, sequence, firstFrame}, onConsumed = () => {}) {
    if (!this.node || !this.context) return Promise.reject(new Error('PCM arrived before audio output was ready'));
    if (this.failure) return Promise.reject(this.failure);
    if (!(samples instanceof Float32Array) || channels !== 2 || sampleRate !== 48000 || samples.length !== frames * channels ||
        !Number.isInteger(frames) || frames < 1 || frames > 256 || typeof epoch !== 'bigint' || epoch <= 0n ||
        typeof sequence !== 'bigint' || sequence !== this.sequence || typeof firstFrame !== 'bigint' || firstFrame !== this.firstFrame ||
        this.epoch !== null && epoch !== this.epoch) return Promise.reject(new Error('invalid engine PCM identity or shape'));
    for (const sample of samples) if (!Number.isFinite(sample)) return Promise.reject(new Error('non-finite engine PCM sample'));
    const key = String(sequence);
    if (this.pending.has(key) || this.pending.size >= 4) return Promise.reject(new Error('overlapping or full engine PCM queue'));
    return new Promise((resolve, reject) => {
      this.pending.set(key, {resolve,reject,onConsumed,epoch,sequence,firstFrame,frames,consumedFrames:0,accepted:false});
      try {
        if (this.epoch === null) this.node.port.postMessage({type:'reset',epoch});
        this.node.port.postMessage({type:'pcm',epoch,sequence,firstFrame,frames,samples}, [samples.buffer]);
        this.epoch = epoch;
        this.sequence++;
        this.firstFrame += BigInt(frames);
      } catch (error) {this.pending.delete(key); reject(error);}
    });
  }
  receive(data) {
    try {
      if (data.type === 'stats') {this.onStats(data); return;}
      if (data.type === 'paused' || data.type === 'resumed') {
        const control = this.controls.get(data.request);
        if (!control) return;
        if (data.type !== control.expected) throw new Error('invalid audio control acknowledgement');
        this.controls.delete(data.request); clearTimeout(control.timer); control.resolve(data);
        return;
      }
      const key = String(data.sequence), pending = this.pending.get(key);
      if (!pending || data.epoch !== pending.epoch) return;
      if (data.type === 'error') throw new Error(data.message ?? 'PCM output rejected');
      if (data.firstFrame !== pending.firstFrame || data.frames !== pending.frames)
        throw new Error('audio output identity changed');
      if (data.type === 'accepted') {
        if (!pending.accepted) this.acceptedFrames += pending.frames;
        pending.accepted = true;
        return;
      }
      if (data.type !== 'consumed' || !pending.accepted || this.pending.values().next().value !== pending ||
          !Number.isInteger(data.consumedFrames) || data.consumedFrames < 1 || data.consumedFrames > pending.frames)
        throw new Error('invalid or out-of-order audio consumption');
      if (data.consumedFrames <= pending.consumedFrames) return;
      this.renderedFrames += data.consumedFrames - pending.consumedFrames;
      pending.consumedFrames = data.consumedFrames;
      pending.onConsumed(data);
      if (data.consumedFrames === pending.frames) {this.pending.delete(key); pending.resolve(data);}
    } catch (error) {this.fail(error);}
  }
  control(type) {
    if (this.failure) return Promise.reject(this.failure);
    const request = ++this.request;
    return new Promise((resolve,reject) => {
      const timer = setTimeout(() => {
        this.controls.delete(request);
        reject(new Error('audio control acknowledgement timed out'));
      },5000);
      this.controls.set(request,{resolve,reject,timer,expected:type === 'pause' ? 'paused' : 'resumed'});
      try {this.node.port.postMessage({type,request});}
      catch (error) {clearTimeout(timer); this.controls.delete(request); reject(error);}
    });
  }
  setVolume(value, muted = this.muted) {
    this.volume = Math.max(0, Math.min(1, Number(value)));
    this.muted = muted;
    if (this.gain) this.gain.gain.value = muted ? 0 : this.volume;
  }
  async pause() {
    const context = this.context;
    if (!context) return;
    const snapshot = await this.control('pause');
    if (this.context === context) {
      await context.suspend();
      this.onStats({...snapshot,type:'stats',event:'paused',contextState:context.state,
        semantics:'frames copied into Web Audio output; not device playback'});
    }
  }
  async resume() {
    const context = this.context;
    if (!context) return;
    await context.resume();
    if (this.context === context) {
      const snapshot = await this.control('resume');
      this.onStats({...snapshot,type:'stats',event:'resumed',contextState:context.state,
        semantics:'frames copied into Web Audio output; not device playback'});
    }
  }
  fail(error) {
    this.failure = error;
    for (const entry of this.pending.values()) entry.reject(error);
    this.pending.clear();
    for (const entry of this.controls.values()) {clearTimeout(entry.timer); entry.reject(error);}
    this.controls.clear();
    this.onDiagnostic(error.message);
  }
  async close() {
    if (!this.context && !this.node) return;
    const epoch = this.epoch, acceptedFrames = this.acceptedFrames, renderedFrames = this.renderedFrames;
    const cancelledPendingFrames = [...this.pending.values()].reduce((sum,packet)=>sum+packet.frames-packet.consumedFrames,0);
    this.fail(new Error('audio output closed'));
    this.node?.port.close();
    this.node?.disconnect();
    const context = this.context;
    this.node = this.context = this.gain = null;
    this.epoch = null; this.sequence = 1n; this.firstFrame = 0n;
    if (context && context.state !== 'closed') await context.close();
    this.onStats({type:'stats',event:'closed',epoch,acceptedFrames,renderedFrames,
      queuedFrames:0,cancelledPendingFrames,contextState:context?.state ?? 'closed',
      semantics:'reported frames copied into Web Audio output; cancelled pending PCM is not consumed'});
  }
}
