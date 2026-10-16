import assert from 'node:assert/strict';
export function afterEffectCall(call,{api,guest,read,pair,metadata,take,pcm}) {
  if (['GetGargleParameters','DetachedGargleParameters'].includes(call.name)) {
    assert.equal(read(guest.addresses.fxReadback),100);assert.equal(read(guest.addresses.fxReadback+4),1);
  }
  if (call.name==='InstallGargle') assert.equal(read(guest.addresses.fxResult),2,'software FX is allocated immediately');
  if (call.hook==='fx-pcm') {
    assert.equal(api.audio_submit(8),0);const packet=metadata(6),sequence=packet.values[0];
    assert.equal(api.audio_read_submission(...pair(packet.epoch),...pair(sequence),0,8),0);
    const output=take(64);const samples=Array.from({length:16},(_,i)=>output.readFloatLE(i*4));
    for (const sample of samples.slice(0,8)) assert.equal(sample,0.5,'gargle square active half preserves authored PCM');
    samples.forEach(sample=>assert.ok(Number.isFinite(sample)));pcm.push(samples);
    assert.equal(api.audio_acknowledge(...pair(packet.epoch),...pair(sequence),8),0);
  }
}
