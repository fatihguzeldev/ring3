import {PcmTransport} from '../../audio/transport.js';

export class AudioHost extends PcmTransport {
  constructor(options={}) {
    super(options);this.context=null;this.node=null;this.volume=0.75;this.muted=false;
    this.remote=null;this.remoteSnapshot=null;
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
      this.closed=false;this.bind(node.port);
      node.onprocessorerror = () => {if(this.node===node){this.fail(new Error('audio worklet failed'));this.remote?.postMessage({type:'audio-error',error:'audio worklet failed'});}};
      await resumed;
      this.onDiagnostic(`audio output ready: ${context.sampleRate} Hz stereo`);
    } catch (error) {
      await this.close();
      throw error;
    }
  }
  connectWorker(worker) {
    if(this.remote)throw new Error('audio Worker already connected');
    const port=this.detach();this.remote=worker;
    this.port={postMessage:message=>worker.postMessage({type:'audio-control',operation:message.type,request:message.request}),close(){}};
    return port;
  }
  accept(packet,onConsumed) {
    if(this.remote)return Promise.reject(new Error('PCM uses the direct Worker transport'));
    return super.accept(packet,onConsumed);
  }
  receive(data) {
    if(this.remote&&(data.type==='stats'||this.controls.get(data.request)?.expected===data.type))this.recordRemote(data);
    super.receive(data);
  }
  recordRemote(snapshot) {
    this.remoteSnapshot=snapshot;this.epoch=snapshot.epoch;
    this.acceptedFrames=snapshot.acceptedFrames;this.renderedFrames=snapshot.renderedFrames;
  }
  reportClosed(snapshot) {if(this.remote)this.recordRemote(snapshot);}
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
  async close() {
    if(!this.context&&!this.node)return;
    const snapshot=this.remoteSnapshot??{epoch:this.epoch,acceptedFrames:this.acceptedFrames,renderedFrames:this.renderedFrames,
      cancelledPendingFrames:[...this.pending.values()].reduce((sum,p)=>sum+p.frames-p.consumedFrames,0)};
    const complete=!this.remote||this.remoteSnapshot?.cancelledPendingFrames!==undefined;
    super.close();this.node?.disconnect();const context=this.context;
    this.node=this.context=this.gain=this.remote=null;this.remoteSnapshot=null;
    if(context&&context.state!=='closed')await context.close();
    this.onStats({...snapshot,type:'stats',event:'closed',queuedFrames:0,contextState:context?.state??'closed',
      cancelledPendingFrames:complete?(snapshot.cancelledPendingFrames??snapshot.queuedFrames??0):null,
      cancellationSnapshotComplete:complete,
      semantics:'reported frames copied into Web Audio output; cancelled pending PCM is not consumed'});
  }
}
