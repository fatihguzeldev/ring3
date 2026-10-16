import assert from 'node:assert/strict';
import {performance} from 'node:perf_hooks';
export function afterCaptureEffectCall(call,{api,guest,read,pair,metadata,take,request,measurements,memory}) {
  if(call.name==='InitialAECMode') assert.equal(read(guest.addresses.aecMode+8),0);
  if(call.name==='ProcessedAECMode') assert.equal(read(guest.addresses.aecMode+8),2,'paired playing render reference executes actual AEC');
  if(call.name==='ResetAECStatus') assert.equal(read(guest.addresses.aecStatus),0);
  if(call.name==='UnlockProcessedAEC') {
    const packed=read(guest.addresses.aecFirst);
    const samples=[(packed<<16)>>16,packed>>16];
    assert.ok(samples.every(sample=>Math.abs(sample)<500),`adapted capture PCM attenuates the authored 2458 echo: ${samples}`);
    measurements.push({adaptedCaptureSamples:samples,inputEchoSample:2458});
  }
  if(call.hook!=='aec-benchmark') return;
  const durations=[];let warmedPages;
  for(let chunk=0;chunk<400;chunk++) {
    for(let half=0;half<2;half++) {
      assert.equal(api.audio_submit(240),0);const audio=metadata(6);
      assert.equal(api.audio_acknowledge(...pair(audio.epoch),...pair(audio.values[0]),240),0);
    }
    assert.equal(api.dsound_capture_request(0),0);const capture=metadata(17),receipt=take(64);
    assert.ok(capture.values[0]>0n);assert.equal(capture.values[2],BigInt(chunk*480));
    const payload=Buffer.alloc(64+960);receipt.copy(payload);
    for(let offset=64;offset<payload.length;offset+=2) payload.writeInt16LE(2458,offset);
    request(payload);const start=performance.now();assert.equal(api.dsound_receive_capture(payload.length),0);
    const elapsed=(performance.now()-start)*1e6;
    if(chunk>=100) durations.push(elapsed);
    if(chunk===99) warmedPages=memory.buffer.byteLength/65536;
  }
  durations.sort((a,b)=>a-b);
  assert.equal(memory.buffer.byteLength/65536,warmedPages,'steady capture DSP does not grow Wasm memory after warmup');
  measurements.push({sampleRate:48000,channels:1,historyMs:128,chunkFrames:480,medianNs:durations[150],p95Ns:durations[285],deadlineNs:10_000_000,warmedPages});
}
