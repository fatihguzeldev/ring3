import {Engine,pair,view} from '../runtime/engine.js';
import {Media} from '../runtime/media.js';

let engine=null,media=null,voice=null,request=0,used=false,started=false;
let packets=0,progress=0,maximumQueue=0,firstPacket=null;
const stamp=()=>performance.timeOrigin+performance.now();
const require=(value,message)=>{if(!value)throw new Error(message);};
const send=(type,id,extra={})=>postMessage({type,id,request,atEpochMs:stamp(),...extra});
function context() {
  const pointer=engine.api.core_context_ptr()>>>0;
  require(pointer===0,'audio-only control created a CPU context');
  return pointer;
}
function record(operation,kind,...args) {
  engine.ok(operation,...args);
  const bytes=engine.take(64),d=view(bytes);
  require(String.fromCharCode(...bytes.subarray(0,4))==='R3MR'&&d.getUint16(4,true)===1&&
    d.getUint16(6,true)===kind&&d.getUint32(8,true)===64&&d.getUint32(12,true)===0,'invalid typed native audio record');
  return {epoch:d.getBigUint64(16,true),values:Array.from({length:5},(_,i)=>d.getBigUint64(24+i*8,true))};
}
function handle(packet) {return [...pair(packet.epoch),Number(packet.values[0]),Number(packet.values[1])];}
async function create(message) {
  require(!used,'audio-only control requires a fresh Worker');used=true;request=message.request;
  require(Number.isSafeInteger(request)&&request>0,'invalid audio-only generation');
  engine=await Engine.create(message.engineUrl,16);
  context();engine.ok('configure_cpu_clock',1_000_000_000,0,0,0);
  const configured=record('configure_media',1,48000),buffer=record('audio_create_buffer',3,48000,1,8,4);
  engine.request(new Uint8Array([0,128,192,255]));engine.ok('audio_write_buffer',...handle(buffer),0,4);
  voice=record('audio_create_voice',4,...handle(buffer));engine.ok('audio_play',...handle(voice),1);
  media=new Media(engine,configured.epoch,(message,transfer)=>{
    require(message.type==='pcm','unexpected non-audio output');
    packets++;maximumQueue=Math.max(maximumQueue,media.audio.length);
    if(!firstPacket) {
      const pattern=[-1,-1,0,0,.5,.5,127/128,127/128];
      require(message.frames===256&&message.samples.length===512,'invalid first PCM shape');
      for(let i=0;i<message.samples.length;i++)require(message.samples[i]===pattern[i%8],'native PCM differs from the literal pattern');
      firstPacket={epoch:message.epoch,sequence:message.sequence,firstFrame:message.firstFrame,
        frames:message.frames,checkedSamples:message.samples.length,pattern};
    }
    postMessage({...message,request},transfer);
  });
  send('audio-only-ready',message.id,{engineHash:engine.hash,epoch:configured.epoch,coreContext:context(),
    producer:{sampleRate:48000,channels:1,bits:8,bytes:[0,128,192,255],looping:true},
    noGuest:true,cpuExecutionRequested:false});
}
function snapshot(id) {
  require(media.audioPaused,'native snapshot requires the producer hold');
  const state=record('audio_state',7),position=record('audio_voice_state',8,...handle(voice));
  require(state.epoch===media.epoch&&position.epoch===media.epoch,'native audio epoch changed');
  send('audio-only-snapshot',id,{state,position,coreContext:context(),packets,progress,maximumQueue,firstPacket,
    queue:media.audio.map(({sequence,firstFrame,frames,consumedFrames})=>({sequence,firstFrame,frames,consumedFrames}))});
}
self.onmessage=({data})=>{
  if(data?.type==='audio-only-create') {
    void create(data).catch(error=>send('audio-only-error',data.id,{message:error.message}));return;
  }
  if(!engine||data.request!==request)return;
  try {
    if(data.type==='audio-only-start') {
      require(!started,'producer already started');started=true;media.fillAudio();send('audio-only-started',data.id);
    } else if(data.type==='pcm-consumed') {require(started,'consumption before start');media.consumedAudio(data);progress++;}
    else if(data.type==='audio-only-hold') {media.pauseAudio();send('audio-only-held',data.id,{coreContext:context()});}
    else if(data.type==='audio-only-snapshot')snapshot(data.id);
    else if(data.type==='audio-only-close') {
      media?.pauseAudio();context();engine.ok('close');
      const closedAudioStatus=engine.status('audio_state');require(closedAudioStatus===5,'closed native audio API was not rejected');
      media=null;engine=null;send('audio-only-closed',data.id,{closedAudioStatus});
    } else throw new Error('unknown audio-only control');
  } catch(error) {media?.pauseAudio();send('audio-only-error',data.id,{message:error.message});}
};
