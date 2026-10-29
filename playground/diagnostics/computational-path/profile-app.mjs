const NativeWorker=globalThis.Worker;
const summary=document.querySelector('#producer-profile-summary');
const output=document.querySelector('#producer-profile-result');
const profiles=[];
function profileResult(result){
  profiles.push(result);output.textContent=JSON.stringify(profiles,null,2);
  const frames=result.trace?.frames??[],resources=result.trace?.resources??[];
  summary.textContent=JSON.stringify({kind:result.kind,status:result.status,reason:result.reason,
    error:result.error,proofId:result.proofId,retired:result.retired,sampleInterval:result.sampleInterval,
    samples:result.trace?.samples.length,frames:frames.length,
    wasmFrames:frames.filter(f=>String(resources[f.resourceId]??'').startsWith('wasm:')).length,
    resources},null,2);
  void fetch('/api/diagnostics',{method:'POST',headers:{'content-type':'application/json'},
    body:JSON.stringify(result)}).then(response=>{
      if(!response.ok)throw new Error(`profile retention HTTP ${response.status}`);
    }).catch(error=>{summary.textContent+=`\nretention failed: ${error.message}`;});
}
class IframeProducer {
  constructor(url){
    this.url=String(url);this.ready=false;this.closed=false;this.queue=[];
    this.iframe=document.createElement('iframe');this.iframe.hidden=true;
    this.iframe.src='/diagnostics/computational-path/producer.html';
    this.iframe.onload=()=>{
      if(this.closed)return;
      const channel=new MessageChannel();this.port=channel.port1;
      this.port.onmessage=event=>{
        if(event.data.type==='producer-ready'){
          this.ready=true;for(const [message,transfer]of this.queue)this.port.postMessage(message,transfer);this.queue=[];return;
        }
        if(event.data.type==='diagnostic-profile'){profileResult(event.data);return;}
        if(event.data.type==='producer-error'){this.onerror?.({message:event.data.error});return;}
        this.onmessage?.(event);
      };
      this.port.onmessageerror=event=>this.onmessageerror?.(event);this.port.start();
      this.iframe.contentWindow.postMessage({type:'initialize-profile-producer',url:this.url},location.origin,[channel.port2]);
    };
    document.body.append(this.iframe);
  }
  postMessage(message,transfer=[]){
    if(this.closed)throw new Error('diagnostic producer closed');
    if(message.type==='start')message={...message,compiledEngine:undefined};
    if(this.ready)this.port.postMessage(message,transfer);else this.queue.push([message,transfer]);
  }
  terminate(){this.closed=true;this.port?.close();this.queue=[];this.iframe.remove();}
}
globalThis.Worker=class {
  constructor(url,options){
    const resolved=new URL(String(url),location.href);
    if(resolved.origin===location.origin&&resolved.pathname==='/runtime/worker.js')return new IframeProducer(resolved);
    return new NativeWorker(url,options);
  }
};
document.querySelector('#authored-profile').onclick=()=>{
  const producer=new IframeProducer(new URL('/runtime/worker.js',location.href));
  producer.postMessage({type:'profile-authored'});
  summary.textContent='authored native/generated sampling running';
  setTimeout(()=>{producer.terminate();},30000);
};
await import('/browser/app.js');
