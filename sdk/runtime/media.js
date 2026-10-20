import {record,view,pair} from './engine.js';
import {GraphicsTransport} from './media/graphics.js';
import {InputMeasurements} from './media/input/measurement.js';
export class Media {
  constructor(engine,epoch,emit,windows) {this.engine=engine;this.epoch=epoch;this.emit=emit;this.windows=windows;this.sequence=0n;this.input=[];this.inputMeasurements=engine.inputMeasurementsEnabled?new InputMeasurements():null;this.audio=[];this.audioPaused=false;this.audioSequence=0n;this.audioFrame=0n;this.lastCursor=0n;this.graphicsTransport=new GraphicsTransport(engine,emit);this.gdi=null;this.dialogs=new Set();}
  enqueue(events,sentAtMs) {if(this.input.length+events.length>4096)throw new Error('host input queue exceeded4096');this.input.push(...events);this.inputMeasurements?.enqueue(events.length,sentAtMs);}
  inputStatus() {return this.inputMeasurements?.snapshot(this.input.length);}
  update(now) {
    const h=this.engine;
    while(this.input.length) {
      if(this.input[0].kind==='pointer') {
        const {x,y}=this.input.shift();
        h.ok('submit_windows_pointer',...pair(this.epoch),...pair(now),x>>>0,y>>>0);
        this.inputMeasurements?.pointer(x,y,now);
        continue;
      }
      const nextPointer=this.input.findIndex(event=>event.kind==='pointer');
      const events=this.input.splice(0,nextPointer<0?120:Math.min(120,nextPointer));
      const bytes=new Uint8Array(32+32*events.length), d=view(bytes);
      bytes.set(new TextEncoder().encode('R3IB'));d.setUint16(4,1,true);d.setUint16(6,32,true);d.setBigUint64(8,this.epoch,true);d.setBigUint64(16,++this.sequence,true);d.setUint32(24,events.length,true);
      events.forEach((event,i)=>{
        const at=32+i*32;d.setBigUint64(at,now,true);let fields;
        if(event.kind==='key')fields=[1,event.code,event.down?1:0,0];
        else if(event.kind==='button')fields=[2,event.button,event.down?1:0,0];
        else if(event.kind==='relative')fields=[3,event.dx,event.dy,event.wheel??0];
        else if(event.kind==='focus')fields=[4,event.focused?1:0,0,0];
        else if(event.kind==='capture')fields=[5,event.captured?1:0,0,0];
        else throw new Error(`unknown physical input kind ${event.kind}`);
        fields.forEach((n,j)=>d.setUint32(at+8+j*4,n>>>0,true));
      });
      h.request(bytes);h.ok('submit_windows_input',bytes.length);
      this.inputMeasurements?.submitted(events,this.sequence,now);
      const focus = events.findLast(event => event.kind === 'focus');
      if (focus) this.lastPhysicalFocus = {focused:!!focus.focused,sequence:this.sequence.toString(),timestampNs:now.toString()};
    }
    h.ok('sync_windows_input');
    const cursor=h.media('pending_user32_cursor'),revision=cursor.values[0];
    if(revision&&revision!==this.lastCursor) {
      this.lastCursor=revision;const flags=cursor.values[1], point=cursor.values[2], a=cursor.values[3],b=cursor.values[4],signed=n=>Number(BigInt.asIntN(32,n));
      this.emit({type:'cursor',revision,visible:!!(flags&256n),cursor:Number(flags>>32n),x:signed(point),y:signed(point>>32n),clip:flags&512n?{left:signed(a),top:signed(a>>32n),right:signed(b),bottom:signed(b>>32n)}:undefined});
    }
    this.fillAudio();
    this.publishDialogs();
    this.publishGdi();
  }
  publishGdi() {
    if (this.gdi) return;
    const h = this.engine;
    h.ok('pending_gdi_frame',0);
    const d = view(h.take(80));
    if (!d.getUint32(12,true)) return;
    const frame = {id:d.getBigUint64(16,true),revision:d.getBigUint64(24,true),window:d.getUint32(32,true),windowDc:!!d.getUint32(36,true),width:d.getUint32(40,true),height:d.getUint32(44,true),stride:d.getUint32(48,true),bytes:d.getUint32(52,true),format:d.getUint32(72,true)};
    if(this.windows&&!this.windows.publish(frame.window)){h.ok('acknowledge_gdi_frame',...pair(frame.id),...pair(frame.revision));return;}
    if (frame.bytes > 128*1024*1024 || frame.stride !== frame.width*4 || frame.bytes !== frame.stride*frame.height) throw new Error('invalid GDI frame size');
    const pixels = new Uint8Array(frame.bytes);
    for (let offset = 0; offset < pixels.length; offset += 4096) {
      const length = Math.min(4096,pixels.length-offset);
      h.ok('read_gdi_frame',...pair(frame.id),...pair(frame.revision),offset,length);
      pixels.set(h.take(length),offset);
    }
    this.gdi = frame;
    this.emit({type:'gdi',frame,pixels},[pixels.buffer]);
    h.ok('acknowledge_gdi_frame',...pair(frame.id),...pair(frame.revision));
  }
  gdiComplete(message) {
    if (this.gdi && this.gdi.id === BigInt(message.id) && this.gdi.revision === BigInt(message.revision)) this.gdi = null;
  }
  fillAudio() {
    while(!this.audioPaused && this.audio.length<4) {
      const submission=this.engine.media('audio_enqueue',256),[sequence,firstFrame,frames]=submission.values;
      if(submission.epoch!==this.epoch || sequence!==this.audioSequence+1n || firstFrame!==this.audioFrame || frames!==256n)
        throw new Error('invalid queued audio identity');
      const pcmBytes=this.engine.take(64+Number(frames)*8).slice(64),samples=new Float32Array(pcmBytes.buffer);
      const packet={epoch:submission.epoch,sequence,firstFrame,frames:Number(frames),consumedFrames:0};
      this.audio.push(packet); this.audioSequence=sequence; this.audioFrame+=frames;
      this.emit({type:'pcm',...packet,samples,channels:2,sampleRate:48000},[samples.buffer]);
    }
  }
  consumedAudio(message) {
    if(message.epoch!==this.epoch)return;
    const packet=this.audio[0];
    if(!packet || message.sequence!==packet.sequence || message.firstFrame!==packet.firstFrame || message.frames!==packet.frames ||
        !Number.isInteger(message.consumedFrames) || message.consumedFrames<1 || message.consumedFrames>packet.frames)
      throw new Error('invalid audio head consumption identity or order');
    if(message.consumedFrames<=packet.consumedFrames)return;
    this.engine.ok('audio_acknowledge',...pair(packet.epoch),...pair(packet.sequence),message.consumedFrames);
    packet.consumedFrames=message.consumedFrames;
    if(packet.consumedFrames===packet.frames)this.audio.shift();
    this.fillAudio();
  }
  pauseAudio() {this.audioPaused=true;}
  resumeAudio() {this.audioPaused=false;this.fillAudio();}
  publishDialogs() {
    const h=this.engine;
    for(let index=0;index<256;index++) {
      h.ok('pending_user32_dialog',index);const d=view(h.take(96));if(!d.getUint32(12,true))break;
      const epoch=d.getBigUint64(16,true),id=d.getBigUint64(24,true);if(this.dialogs.has(id))continue;
      const read=(field,total)=>{let text='';for(let offset=0;offset<total;offset+=1900){h.ok('read_user32_dialog_text',...pair(epoch),...pair(id),field,offset,Math.min(1900,total-offset));const page=view(h.take(4096)),count=page.getUint32(84,true);for(let i=0;i<count;i++)text+=String.fromCharCode(page.getUint16(96+i*2,true));}return text;};
      const dialog={type:'dialog',epoch,id,thread:d.getUint32(32,true),owner:d.getUint32(36,true),style:d.getUint32(40,true),language:d.getUint32(44,true),helpContext:d.getUint32(48,true),caption:read(1,d.getUint32(64,true)),text:read(2,d.getUint32(68,true))};this.dialogs.add(id);this.emit(dialog);
    }
  }
  async graphicsOutput() {
    return this.graphicsTransport.output();
  }
  graphicsComplete(message) {
    return this.graphicsTransport.complete(message);
  }
  graphicsStatus() {
    return this.graphicsTransport.status();
  }
  dialogResult(message) {this.engine.ok('complete_user32_dialog',...pair(message.epoch),...pair(message.id),message.button,message.error??0);this.dialogs.delete(BigInt(message.id));}
  dialogHelp(message) {this.engine.ok('request_user32_dialog_help',...pair(message.epoch),...pair(message.id));}
  cursorApplied(revision) {this.engine.ok('acknowledge_user32_cursor',...pair(revision));}
}
