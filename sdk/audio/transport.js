export class PcmTransport {
  constructor({port=null,onDiagnostic=()=>{},onStats=()=>{}}={}) {
    this.onDiagnostic=onDiagnostic;this.onStats=onStats;
    this.pending=new Map();this.controls=new Map();this.request=0;
    this.epoch=null;this.sequence=1n;this.firstFrame=0n;this.failure=null;
    this.acceptedFrames=0;this.renderedFrames=0;this.closed=false;this.port=null;
    if(port)this.bind(port);
  }
  bind(port) {
    if(this.port||this.closed)throw new Error('audio transport already connected or closed');
    this.port=port;port.onmessage=({data})=>this.receive(data);
    port.onmessageerror=()=>this.fail(new Error('audio message could not be decoded'));
    port.start?.();
  }
  detach() {
    if(!this.port||this.pending.size||this.controls.size||this.epoch!==null)
      throw new Error('audio handoff requires an unused transport');
    const port=this.port;port.onmessage=null;port.onmessageerror=null;this.port=null;return port;
  }
  accept({samples, channels, sampleRate, frames, epoch, sequence, firstFrame}, onConsumed = () => {}) {
    if (!this.port) return Promise.reject(new Error('PCM arrived before audio output was ready'));
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
        if (this.epoch === null) this.port.postMessage({type:'reset',epoch});
        this.port.postMessage({type:'pcm',epoch,sequence,firstFrame,frames,samples}, [samples.buffer]);
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
      // direct consumption can synchronously refill through the producer callback.
      const completed=data.consumedFrames===pending.frames;
      if(completed)this.pending.delete(key);
      try{pending.onConsumed(data);}catch(error){pending.reject(error);throw error;}
      if(completed)pending.resolve(data);
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
      try {this.port.postMessage({type,request});}
      catch (error) {clearTimeout(timer); this.controls.delete(request); reject(error);}
    });
  }
  fail(error) {
    this.failure = error;
    for (const entry of this.pending.values()) entry.reject(error);
    this.pending.clear();
    for (const entry of this.controls.values()) {clearTimeout(entry.timer); entry.reject(error);}
    this.controls.clear();
    this.onDiagnostic(error.message);
  }
  close() {
    if(this.closed)return this.closedSnapshot;
    this.closed=true;
    const snapshot={epoch:this.epoch,acceptedFrames:this.acceptedFrames,renderedFrames:this.renderedFrames,
      queuedFrames:0,cancelledPendingFrames:[...this.pending.values()].reduce((sum,p)=>sum+p.frames-p.consumedFrames,0),
      semantics:'reported frames copied into Web Audio output; cancelled pending PCM is not consumed'};
    this.fail(new Error('audio output closed'));this.port?.close();this.port=null;
    this.epoch=null;this.sequence=1n;this.firstFrame=0n;
    this.closedSnapshot=snapshot;return snapshot;
  }
}
