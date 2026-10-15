import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {readFileSync, mkdirSync, writeFileSync} from 'node:fs';
import {join} from 'node:path';
import {performance} from 'node:perf_hooks';
import {assertEngineExports} from '../../../support/wasm/exports.mjs';

const [enginePath, outputPath, root] = process.argv.slice(2);
const bytes = readFileSync(enginePath);
const sha256 = createHash('sha256').update(bytes).digest('hex');
assert.equal(sha256, process.env.RING3_ENGINE_SHA256, 'exact selected engine');
const module = await WebAssembly.compile(bytes);
assertEngineExports(module, root);
const {exports: e} = await WebAssembly.instantiate(module, {});
let calls = 0;
function call(name, args = [], expected = 0) {
  const fn = e[`ring3_abi_v1_${name}`];
  assert.equal(typeof fn, 'function', name);
  assert.equal(fn.length, args.length, `${name} arity`);
  const result = fn(...args);
  calls++;
  assert.equal(result, expected, name);
}
const wide = n => [Number(n & 0xffffffffn), Number(n >> 32n)];
const fbits = n => {const b = Buffer.alloc(4); b.writeFloatLE(n); return b.readUInt32LE();};
call('open', [8, 101, 0]);
const arena = e.ring3_abi_v1_arena_ptr();
const transfer = arena + 140;
const read = n => Buffer.from(new Uint8Array(e.memory.buffer, transfer, n));
const write = b => new Uint8Array(e.memory.buffer, transfer, b.length).set(b);
function record(kind) {
  const b = read(64);
  assert.equal(b.toString('ascii', 0, 4), 'R3MR');
  assert.equal(b.readUInt16LE(4), 1);
  assert.equal(b.readUInt16LE(6), kind);
  assert.equal(b.readUInt32LE(8), 64);
  assert.equal(b.readUInt32LE(12), 0);
  return {epoch: b.readBigUInt64LE(16), values: Array.from({length: 5}, (_, i) => b.readBigUInt64LE(24 + i * 8))};
}
function handle(kind) {
  const {epoch, values: [slot, generation]} = record(kind);
  return {epoch, args: [...wide(epoch), Number(slot), Number(generation)]};
}
call('configure_media', [48000], 9);
call('configure_cpu_clock', [1000000000, 0, 100, 0]);
call('observe_cpu_clock', [1000, 0]);
call('configure_media', [0], 7);
call('input_state', [], 38);
call('configure_media', [48000]);
const epoch = record(1).epoch;
call('configure_media', [48000], 39);

function vertices(color) {
  const result = Buffer.alloc(60);
  [[-1,-1],[1,-1],[0,1]].forEach(([x,y], i) => {
    result.writeFloatLE(x, i*20); result.writeFloatLE(y, i*20+4);
    result.writeFloatLE(0, i*20+8); result.writeFloatLE(1, i*20+12);
    result.set(color, i*20+16);
  });
  return result;
}
const red = vertices([255,0,0,255]), blue = vertices([0,0,255,255]);
write(red); call('graphics_create_vertex_buffer', [red.length]);
const resource = handle(2);
assert.equal(resource.epoch, epoch);
call('graphics_clear', [0,0,0,fbits(1)]);
call('graphics_draw_triangles', [...resource.args,0,3]);
write(blue); call('graphics_update_vertex_buffer', [...resource.args,0,blue.length]);
call('graphics_draw_triangles', [...resource.args,0,3]);
call('graphics_present', [77]);
call('graphics_destroy_vertex_buffer', resource.args);
call('graphics_draw_triangles', [...resource.args,0,3],40);
call('graphics_seal_batch');
const graphics = record(5);
const batchArgs = [...wide(graphics.epoch), ...wide(graphics.values[0])];
const length = Number(graphics.values[1]);
call('graphics_clear', [0,0,0,fbits(1)],12);
call('graphics_read_batch', [...batchArgs,length-1,2],7);
call('graphics_read_batch', [...batchArgs,0,length]);
const commands = read(length);
assert.equal(commands.toString('ascii',0,4),'R3GB');
assert.equal(commands.readUInt16LE(4),1); assert.equal(commands.readUInt16LE(6),1);
assert.equal(commands.readUInt32LE(8),length); assert.equal(commands.readUInt32LE(12),7);
assert.equal(commands.readBigUInt64LE(16),epoch);
assert.equal(commands.readBigUInt64LE(24),graphics.values[0]);
assert.equal(commands.readBigUInt64LE(32),0n);
const ops=[], versions=[];
let offset=40;
while(offset<length) {
  const opcode=commands.readUInt16LE(offset), size=commands.readUInt32LE(offset+4);
  assert.equal(commands.readUInt16LE(offset+2),0); assert.ok(size>=8);
  ops.push(opcode);
  if([1,2,3].includes(opcode)) {
    assert.equal(commands.readBigUInt64LE(offset+8),resource.epoch);
    assert.equal(commands.readUInt32LE(offset+16),resource.args[2]);
    assert.equal(commands.readUInt32LE(offset+20),resource.args[3]);
    versions.push(commands.readBigUInt64LE(offset+24));
  }
  if(opcode===1) assert.deepEqual(commands.subarray(offset+40,offset+size),red);
  if(opcode===2) assert.deepEqual(commands.subarray(offset+40,offset+size),blue);
  offset+=size;
}
assert.equal(offset,length); assert.deepEqual(ops,[1,5,3,2,3,6,4]);
assert.deepEqual(versions,[1n,1n,2n,2n]);
call('graphics_acknowledge_batch', [...wide(epoch), ...wide(graphics.values[0]+1n)],40);
call('graphics_acknowledge_batch', batchArgs);

call('audio_create_buffer', [44100,1,8,4],20);
call('audio_create_buffer', [48000,1,8,4]); const buffer=handle(3);
write(Buffer.from([0,128,192,255])); call('audio_write_buffer',[...buffer.args,0,4]);
call('audio_create_voice',buffer.args); const voice=handle(4);
call('audio_play',[...voice.args,1]);
call('audio_submit',[4]); const submission=record(6);
const pcm=read(96).subarray(64);
assert.deepEqual(Array.from({length:8},(_,i)=>pcm.readFloatLE(i*4)),[-1,-1,0,0,.5,.5,127/128,127/128]);
const ackArgs=[...wide(epoch),...wide(submission.values[0])];
call('audio_voice_state',voice.args); assert.deepEqual(record(8).values.slice(0,2),[0n,0n]);
call('audio_state'); assert.deepEqual(record(7).values.slice(0,2),[4n,0n]);
call('audio_read_submission',[...ackArgs,0,4]); assert.deepEqual(read(32),pcm);
call('audio_read_submission',[...ackArgs,3,2],7);
call('audio_stop',voice.args,12);
call('audio_acknowledge',[...ackArgs,5],7);
call('audio_acknowledge',[...ackArgs,2]);
call('audio_voice_state',voice.args); assert.equal(record(8).values[0],2n);
call('audio_acknowledge',[...ackArgs,2],7);
call('audio_acknowledge',[...ackArgs,4]);
call('audio_read_submission',[...ackArgs,0,1],40);
call('audio_state'); assert.deepEqual(record(7).values.slice(0,2),[4n,4n]);
call('audio_stop',voice.args); call('audio_seek',[...voice.args,1]);
call('audio_play',[...voice.args,0]); call('audio_submit',[2]); const pendingAudio=record(6);

function inputBatch(session,seq,events) {
  const b=Buffer.alloc(32+events.length*32);
  b.write('R3IB'); b.writeUInt16LE(1,4); b.writeUInt16LE(32,6);
  b.writeBigUInt64LE(session,8); b.writeBigUInt64LE(BigInt(seq),16); b.writeUInt32LE(events.length,24);
  events.forEach(([time,kind,a=0,x=0,c=0],i)=>{
    const p=32+i*32; b.writeBigUInt64LE(BigInt(time),p); b.writeUInt32LE(kind,p+8);
    b.writeUInt32LE(a>>>0,p+12); b.writeUInt32LE(x>>>0,p+16); b.writeUInt32LE(c>>>0,p+20);
  }); return b;
}
function sendInput(session,seq,events,status=0) {const b=inputBatch(session,seq,events); write(b);call('submit_input',[b.length],status);}
sendInput(epoch,1,[[1001,4,1]],41);
sendInput(epoch,1,[[900,4,1],[900,1,20,1],[901,1,20,0],[902,2,1,1],[903,3,7,-3,1],[904,5,1]]);
call('input_state'); const state=record(9);
assert.deepEqual(state.values,[3n,2n,7n,BigInt.asUintN(64,-3n),1n]);
assert.equal(read(96)[64+2]&(1<<4),0);
call('input_read_queue',[6,31,1],18);
call('input_read_queue',[6,4096,0]); const peek=read(32+6*48);
assert.equal(peek.toString('ascii',0,4),'R3IQ'); assert.equal(peek.readUInt32LE(16),6);
assert.equal(peek.readUInt32LE(32+48+8),1); assert.equal(peek.readUInt32LE(32+48+16),1);
assert.equal(peek.readUInt32LE(32+96+16),0);
call('input_read_queue',[6,4096,1]); assert.deepEqual(read(peek.length),peek);
call('input_drain_relative'); assert.deepEqual(record(11).values,[7n,BigInt.asUintN(64,-3n),1n,0n,0n]);
call('input_consume',[1],7);
sendInput(epoch,2,[[905,5,0]]);
call('input_state'); assert.equal(record(9).values[0],1n);
sendInput(epoch,3,[[906,1,21,1],[907,4,0]]);
call('input_state'); assert.deepEqual(record(9).values,[0n,0n,0n,0n,0n]);
assert.equal(read(96)[64+2]&(1<<5),0);
call('input_read_queue',[10,4096,1]); const released=read(32+5*48);
assert.equal(released.readUInt32LE(16),5);
assert.deepEqual(Array.from({length:5},(_,i)=>released.readUInt32LE(32+i*48+40)),[0,0,1,1,0]);

write(red); call('graphics_create_vertex_buffer',[red.length]); const replacement=handle(2);
assert.ok(replacement.args[3]>resource.args[3]);
call('graphics_draw_triangles',[...replacement.args,0,3]); call('graphics_seal_batch');
const pendingGraphics=record(5);
call('reset_media'); const newEpoch=record(1).epoch; assert.ok(newEpoch>epoch);
call('graphics_draw_triangles',[...resource.args,0,3],40);
call('graphics_draw_triangles',[...replacement.args,0,3],40);
call('graphics_acknowledge_batch',batchArgs,40);
call('graphics_acknowledge_batch',[...wide(epoch),...wide(pendingGraphics.values[0])],40);
call('audio_acknowledge',[...wide(epoch),...wide(pendingAudio.values[0]),2],40);
call('audio_play',[...voice.args,0],40);
sendInput(epoch,1,[[1000,4,1]],40);
sendInput(newEpoch,1,[[999,4,1]],41);
sendInput(newEpoch,1,[[1000,4,1]]);

// measures the actual bounded wasm mixer path; no browser/device performance claim.
call('audio_create_buffer',[48000,1,8,4]); const benchBuffer=handle(3);
call('audio_create_voice',benchBuffer.args); const benchVoice=handle(4);
call('audio_play',[...benchVoice.args,1]);
const mixerVoices=32;
for(let i=1;i<mixerVoices;i++) {
  call('audio_create_voice',benchBuffer.args); const another=handle(4);
  call('audio_play',[...another.args,1]);
}
const timings=[];
for(let sample=0;sample<5;sample++) {
  const start=performance.now();
  for(let i=0;i<1000;i++) {
    call('audio_submit',[256]); const current=record(6);
    call('audio_acknowledge',[...wide(newEpoch),...wide(current.values[0]),256]);
  }
  timings.push(performance.now()-start);
}
call('close'); call('reset_media',[],5); call('input_state',[],5); call('audio_submit',[1],5);
const receipt={sha256,node:process.version,platform:process.platform,arch:process.arch,calls,
  graphics:{commands:ops,versions:versions.map(String),bytes:length},
  audio:{pcmFrames:4,partialAck:true,mixerVoices,mixerCyclesPerSample:1000,framesPerCycle:256,timingsMs:timings},
  input:{orderedTransitions:true,focusReleases:2,queueFailureNeutral:true},
  reset:{oldEpoch:String(epoch),newEpoch:String(newEpoch),staleRejected:true},
  ceiling:'actual wasm host contract; no guest Windows media frontend, GPU, sound device or physical input'};
mkdirSync(outputPath,{recursive:true}); writeFileSync(join(outputPath,'receipt.json'),JSON.stringify(receipt,null,2)+'\n');
console.log(JSON.stringify(receipt));
