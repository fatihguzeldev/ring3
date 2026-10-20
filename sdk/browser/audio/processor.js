class Ring3Pcm extends AudioWorkletProcessor {
  constructor({processorOptions = {}} = {}) {
    super();
    this.capacityFrames = processorOptions.capacityFrames ?? 1024;
    this.primeFrames = processorOptions.primeFrames ?? 512;
    if (!Number.isInteger(this.capacityFrames) || this.capacityFrames < 512 || this.capacityFrames > 1024 ||
        !Number.isInteger(this.primeFrames) || this.primeFrames < 1 || this.primeFrames > this.capacityFrames)
      throw new Error('invalid PCM queue limits');
    this.queue = [];
    this.epoch = null;
    this.sequence = 1n;
    this.firstFrame = 0n;
    this.queuedFrames = 0;
    this.renderedFrames = 0;
    this.acceptedFrames = 0;
    this.paused = false;
    this.priming = true;
    this.started = false;
    this.underruns = 0;
    this.underrunFrames = 0;
    this.lastReport = 0;
    this.port.onmessage = ({data}) => this.receive(data);
  }
  receive(data) {
    if (data.type === 'pause' || data.type === 'resume') {
      this.paused = data.type === 'pause';
      this.port.postMessage({...this.snapshot(),type:this.paused ? 'paused' : 'resumed',request:data.request});
      return;
    }
    if (data.type === 'reset') {
      if (typeof data.epoch !== 'bigint' || data.epoch <= 0n || this.epoch !== null && data.epoch <= this.epoch)
        return this.reject(data,'invalid PCM reset epoch');
      this.queue.length = 0;
      this.epoch = data.epoch;
      this.sequence = 1n;
      this.firstFrame = 0n;
      this.queuedFrames = this.renderedFrames = this.acceptedFrames = 0;
      this.underruns = this.underrunFrames = 0;
      this.priming = true;
      this.started = false;
      this.lastReport = currentFrame;
      return;
    }
    if (data.type !== 'pcm') return;
    if (data.epoch !== this.epoch || data.sequence !== this.sequence || data.firstFrame !== this.firstFrame ||
        !Number.isInteger(data.frames) || data.frames < 1 || data.frames > 256 ||
        !(data.samples instanceof Float32Array) || data.samples.length !== data.frames * 2 ||
        this.queue.length >= 4 || data.frames > this.capacityFrames - this.queuedFrames)
      return this.reject(data,'invalid, out-of-order or full PCM queue');
    for (const sample of data.samples) if (!Number.isFinite(sample)) return this.reject(data,'non-finite PCM sample');
    this.queue.push({...data,consumedFrames:0});
    this.sequence++;
    this.firstFrame += BigInt(data.frames);
    this.queuedFrames += data.frames;
    this.acceptedFrames += data.frames;
    this.port.postMessage({type:'accepted',epoch:data.epoch,sequence:data.sequence,firstFrame:data.firstFrame,
      frames:data.frames,queuedFrames:this.queuedFrames});
  }
  reject(data, message) {
    this.port.postMessage({type:'error',epoch:data.epoch,sequence:data.sequence,message});
  }
  snapshot() {
    return {epoch:this.epoch,queuedFrames:this.queuedFrames,acceptedFrames:this.acceptedFrames,
      renderedFrames:this.renderedFrames,underruns:this.underruns,underrunFrames:this.underrunFrames,
      paused:this.paused,capacityFrames:this.capacityFrames,primeFrames:this.primeFrames,
      semantics:'frames copied into Web Audio output; not device playback'};
  }
  stats() {
    if (currentFrame - this.lastReport < sampleRate) return;
    this.lastReport = currentFrame;
    this.port.postMessage({type:'stats',...this.snapshot()});
  }
  process(_inputs, outputs) {
    const output = outputs[0];
    if (!output?.length) return true;
    for (const channel of output) channel.fill(0);
    const left = output[0], right = output[1] ?? left;
    if (this.paused) {this.stats(); return true;}
    if (this.priming) {
      if (this.queuedFrames < this.primeFrames) {
        if (this.started) {this.underruns++; this.underrunFrames += left.length;}
        this.stats();
        return true;
      }
      this.priming = false;
      this.started = true;
    }
    let written = 0;
    while (written < left.length && this.queue.length) {
      const packet = this.queue[0];
      const frames = Math.min(left.length - written,packet.frames - packet.consumedFrames);
      for (let index=0;index<frames;index++) {
        const source = (packet.consumedFrames + index) * 2;
        left[written + index] = packet.samples[source];
        right[written + index] = packet.samples[source + 1];
      }
      packet.consumedFrames += frames;
      written += frames;
      this.queuedFrames -= frames;
      this.renderedFrames += frames;
      this.port.postMessage({type:'consumed',epoch:packet.epoch,sequence:packet.sequence,
        firstFrame:packet.firstFrame,frames:packet.frames,consumedFrames:packet.consumedFrames});
      if (packet.consumedFrames === packet.frames) this.queue.shift();
    }
    if (written < left.length) {
      this.underruns++;
      this.underrunFrames += left.length - written;
    }
    if (!this.queuedFrames) this.priming = true;
    this.stats();
    return true;
  }
}
registerProcessor('ring3-playground-pcm', Ring3Pcm);
