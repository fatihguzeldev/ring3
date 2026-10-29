const body = [1,1,127,2,64,3,64,32,0,69,13,1,32,1,32,0,106,33,1,32,0,65,1,107,33,0,12,0,11,11,32,1,11];
const bytes = new Uint8Array([0,97,115,109,1,0,0,0,1,6,1,96,1,127,1,127,3,2,1,0,7,7,1,3,114,117,110,0,0,10,body.length+2,1,body.length,...body]);
document.querySelector('#sample').onclick = async () => {
  const button = document.querySelector('#sample'), output = document.querySelector('#summary');
  button.disabled = true; output.textContent = 'sampling';
  const result = { userAgent: navigator.userAgent, isolated: crossOriginIsolated, profiler: typeof Profiler };
  try {
    const wasm = new WebAssembly.Instance(new WebAssembly.Module(bytes));
    if (wasm.exports.run(5) !== 15) throw new Error('authored sum oracle failed');
    for (let i=0;i<20;i++) wasm.exports.run(5000000);
    const profiler = new Profiler({sampleInterval:1,maxBufferSize:1000});
    result.sampleInterval = profiler.sampleInterval;
    const start = performance.now(); let calls=0, value=0;
    while (performance.now()-start < 1000) { value=wasm.exports.run(5000000);calls++; }
    result.elapsedMs=performance.now()-start; result.calls=calls; result.value=value;
    result.expected=(5000000*5000001/2)|0;
    if (value !== result.expected) throw new Error('bounded sum oracle failed');
    result.trace=await profiler.stop();
    result.sampleCount=result.trace.samples.length;
    result.wasmFrames=result.trace.frames.filter(f=>String(result.trace.resources[f.resourceId]??'').startsWith('wasm:'));
    result.status=result.wasmFrames.length?'WASM_FRAMES_VISIBLE':'NO_WASM_FRAMES';
  } catch(error) { result.status='FAIL';result.error={name:error.name,message:error.message}; }
  output.textContent=JSON.stringify(Object.fromEntries(Object.entries(result).filter(([key])=>key!=='trace')),null,2);
  document.querySelector('#result').textContent=JSON.stringify(result,null,2);
};
