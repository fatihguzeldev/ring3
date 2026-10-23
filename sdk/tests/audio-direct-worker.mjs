import {parentPort,workerData} from 'node:worker_threads';
import {readFile} from 'node:fs/promises';
import vm from 'node:vm';
import {PcmTransport} from '../audio/transport.js';
import {Media} from '../runtime/media.js';

const counters=new Int32Array(workerData.counters),port=workerData.port;
const send=(type,value={})=>parentPort.postMessage({type,...value});
if(workerData.role==='processor') {
  let Processor,timer;
  const context=vm.createContext({Float32Array,sampleRate:48000,currentFrame:0,
    AudioWorkletProcessor:class {constructor(){this.port=port;}},
    registerProcessor(_name,type){Processor=type;},
  });
  vm.runInContext(await readFile(new URL('../browser/audio/processor.js',import.meta.url),'utf8'),context);
  const processor=new Processor({processorOptions:{capacityFrames:1024,primeFrames:512}});
  parentPort.on('message',message=>{
    if(message.type==='start')timer=setInterval(()=>{
      processor.process([],[[new Float32Array(128),new Float32Array(128)]]);
      context.currentFrame+=128;Atomics.store(counters,3,processor.renderedFrames);
    },2);
    if(message.type==='close'){clearInterval(timer);port.close();send('closed');}
  });
  send('ready');
} else {
  let latest,sequence=0n,firstFrame=0n,closing=false,media;
  const fail=error=>{if(closing)return;Atomics.store(counters,4,1);send('failure',{message:error.message??error});};
  const output=new PcmTransport({port,onDiagnostic:message=>fail(new Error(message))});
  const engine={
    media(operation,frames){
      if(operation!=='audio_enqueue')throw new Error('unexpected native operation');
      latest={epoch:7n,values:[++sequence,firstFrame,BigInt(frames)]};firstFrame+=BigInt(frames);return latest;
    },
    take(length){const bytes=new Uint8Array(length);new Float32Array(bytes.buffer,64).fill(.25);return bytes;},
    ok(operation,...args){
      if(operation!=='audio_acknowledge')throw new Error('unexpected native acknowledgement');
      Atomics.add(counters,args.at(-1)===256?0:1,1);
    },
  };
  media=new Media(engine,7n,packet=>{
    Atomics.add(counters,5,1);Atomics.store(counters,2,Math.max(Atomics.load(counters,2),media.audio.length));
    void output.accept(packet,progress=>media.consumedAudio(progress)).catch(fail);
  });
  parentPort.on('message',async message=>{
    try {
      if(message.type==='start'){media.fillAudio();send('started');}
      if(message.type==='pause'){
        media.pauseAudio();const snapshot=await output.control('pause');
        if(snapshot.renderedFrames!==output.renderedFrames)throw new Error('pause passed earlier output progress');
        send('paused',{snapshot,reportedFrames:output.renderedFrames});
      }
      if(message.type==='resume'){await output.control('resume');media.resumeAudio();send('resumed');}
      if(message.type==='close'){closing=true;media.pauseAudio();send('closed',{snapshot:output.close()});}
    } catch(error){fail(error);}
  });
  send('ready');
}
