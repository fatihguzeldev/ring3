import assert from 'node:assert/strict';
export function afterHapticCall(call,{api,context,pair,metadata,take,read,guest}) {
  if(call.name==='ConstantForceGuid') assert.equal(read(guest.addresses.forceGuid),0x13541c20);
  if(call.name==='ConstantForceStatus') assert.equal(read(guest.addresses.forceStatus),1);
  if(call.hook!=='force-frame') return;
  assert.equal(api.render_dinput_haptics(...pair(context.mediaEpoch),0,...pair(context.now)),0);
  const packet=metadata(21),frame=take(144).subarray(64);
  assert.equal(frame.subarray(0,4).toString(),'R3HF');assert.equal(frame.readUInt32LE(8),80);
  assert.equal(frame.readBigUInt64LE(16),context.mediaEpoch);assert.equal(frame.readBigUInt64LE(32),context.now);
  assert.deepEqual(Array.from({length:8},(_,i)=>frame.readInt32LE(48+i*4)),[4000,0,0,0,0,0,0,0]);
  assert.equal(api.render_dinput_haptics(...pair(context.mediaEpoch),0,...pair(context.now)),12,'pending physical frame requires acknowledgement');
  assert.equal(api.acknowledge_dinput_haptics(...pair(context.mediaEpoch),0,...pair(packet.values[0]+1n)),14,'wrong sequence cannot consume frame');
  assert.equal(api.pending_dinput_haptics(0),0);assert.deepEqual(take(144).subarray(64),frame);
  assert.equal(api.acknowledge_dinput_haptics(...pair(context.mediaEpoch),0,...pair(packet.values[0])),0);
  assert.equal(api.pending_dinput_haptics(0),0);assert.equal(metadata(21).values[3],0n);
}
export function inspectForceFileCallback(request,{read,guest}) {
  const context=request.readUInt32LE(36);
  if(request.readUInt32LE(12)!==4||![0xface,0xcafe].includes(context))return;
  const row=request.readUInt32LE(32);assert.equal(read(row),284);
  assert.equal(read(row+4),context===0xcafe?0x13541c2b:0x13541c20);
  const effect=read(row+20);assert.equal(read(effect),56);assert.equal(read(effect+28),2);
  const payload=read(effect+48);
  if(context===0xcafe) {
    assert.equal(read(effect+44),16);
    assert.deepEqual(Array.from({length:4},(_,i)=>read(payload+i*4)),[2,1000,8,guest.addresses.forceCustomSamples]);
    assert.deepEqual(Array.from({length:8},(_,i)=>read(guest.addresses.forceCustomSamples+i*4)),[4000,0,(-2000)>>>0,0,1000,0,0,0]);
  } else assert.equal(read(payload),4000);
}
