let initialized=false;
async function bootstrap(event) {
  if(initialized||event.source!==parent||event.origin!==location.origin||event.data?.type!=='initialize-profile-producer'||event.ports.length!==1)return;
  const url=new URL(event.data.url,location.href);
  if(url.origin!==location.origin||url.pathname!=='/runtime/worker.js')return;
  initialized=true;removeEventListener('message',bootstrap);
  const port=event.ports[0];let proof=null,profiler=null,events=[],profileStart=null,stopping=null,timeout=null;
  const profileResult=result=>port.postMessage({type:'diagnostic-profile',...result});
  async function stopProfile(reason) {
    if(stopping)return stopping;
    if(!profiler)return;
    const current=profiler;profiler=null;
    clearTimeout(timeout);
    const end=performance.now(),saved={proofId:proof,start:profileStart,events:[...events]};
    stopping=(async()=>{
      try {
        const trace=await current.stop();
        profileResult({kind:'game-frames',status:reason==='complete'?'PASS':'INCOMPLETE',reason,
          ...saved,sampleInterval:current.sampleInterval,timeOrigin:performance.timeOrigin,
          end,trace,producerPlacement:'diagnostic-window-iframe',
          limits:['original game code and engine in a diagnostic Window realm; not production Worker timing','original frame-probe realm label is retained; actual producer placement is Window iframe','sample leaf counts are statistical observations, not hardware-cycle percentages']});
      } catch(error) {
        profileResult({kind:'game-frames',status:'INCOMPLETE',reason:'profiler-stop-failed',
          ...saved,error:error.stack??error.message});
      }
    })().finally(()=>{stopping=null;});
    return stopping;
  }
  globalThis.postMessage=(message,transfer=[])=>{
    const d=message.type==='diagnostic'&&message.message==='frame-probe'?message.detail:null;
    if(d?.proof_id===proof){
      if(events.length<4096)events.push(d);
      if(d.event==='finished')void stopProfile(d.reason);
    }
    if(message.type==='status'&&['stopped','error','exited'].includes(message.state)&&(profiler||stopping)){
      void stopProfile(`terminal-${message.state}`).then(()=>port.postMessage(message,transfer));return;
    }
    port.postMessage(message,transfer);
  };
  try {
    await import(url.href);
    const originalHandler=globalThis.onmessage;globalThis.onmessage=null;
    if(typeof originalHandler!=='function')throw new Error('original runtime handler missing');
    port.onmessage=event=>{
      const message=event.data;
      if(message.type==='profile-authored'){
        void import('./authored-profile.mjs').then(m=>m.profileAuthored()).then(profileResult)
          .catch(error=>profileResult({kind:'authored-native-generated',status:'FAIL',error:error.stack??error.message}));
        return;
      }
      if(message.type==='frame-probe-arm'){
        if(profiler||stopping){profileResult({kind:'game-frames',status:'FAIL',error:'profile already active or draining'});return;}
        proof=message.id;events=[];
        try{
          profiler=new Profiler({sampleInterval:1,maxBufferSize:6000});profileStart=performance.now();
          profiler.addEventListener('samplebufferfull',()=>void stopProfile('sample-buffer-full'));
          timeout=setTimeout(()=>void stopProfile('timeout'),45000);
        }catch(error){profileResult({kind:'game-frames',status:'FAIL',error:error.message});}
      }
      if(message.type==='stop')void stopProfile('host-stop');
      originalHandler(event);
    };
    port.start();port.postMessage({type:'producer-ready'});
  }catch(error){port.postMessage({type:'producer-error',error:error.stack??error.message});}
}
addEventListener('message',bootstrap);
