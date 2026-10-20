import assert from 'node:assert/strict';
import {readFile} from 'node:fs/promises';
import test from 'node:test';
import vm from 'node:vm';

const source = await readFile(new URL('../browser/audio/processor.js', import.meta.url), 'utf8');

function fixture() {
  const messages = [];
  let Processor;
  const context = vm.createContext({
    Float32Array, sampleRate:48000, currentFrame:0,
    AudioWorkletProcessor:class {
      constructor() {this.port = {postMessage:message => messages.push(message)};}
    },
    registerProcessor(_name, type) {Processor = type;},
  });
  vm.runInContext(source, context);
  const processor = new Processor({processorOptions:{capacityFrames:1024,primeFrames:512}});
  const send = message => processor.port.onmessage({data:message});
  const render = (frames = 128) => {
    const output = [new Float32Array(frames),new Float32Array(frames)];
    assert.equal(processor.process([], [output]), true);
    context.currentFrame += frames;
    return output;
  };
  const pcm = (sequence, firstFrame, value = sequence / 10, epoch = 1n) => ({
    type:'pcm',epoch,sequence:BigInt(sequence),firstFrame:BigInt(firstFrame),frames:256,
    samples:new Float32Array(512).fill(value),
  });
  send({type:'reset',epoch:1n});
  return {messages,send,render,pcm};
}

test('copying queued PCM grants no consumed credit and priming leaves the cursor still', () => {
  const {messages,send,render,pcm} = fixture();
  send(pcm(1,0,0.25));
  assert.equal(messages.filter(message => message.type === 'consumed').length,0);
  assert.deepEqual([...render()[0]],new Array(128).fill(0));
  assert.equal(messages.filter(message => message.type === 'consumed').length,0);
  send(pcm(2,256,0.5));
  assert.deepEqual([...render()[0]],new Array(128).fill(0.25));
  const progress = messages.filter(message => message.type === 'consumed');
  assert.equal(progress.length,1);
  assert.equal(progress[0].epoch,1n);
  assert.equal(progress[0].sequence,1n);
  assert.equal(progress[0].firstFrame,0n);
  assert.equal(progress[0].frames,256);
  assert.equal(progress[0].consumedFrames,128);
});

test('a render crossing packet boundaries reports each cumulative prefix in FIFO order', () => {
  const {messages,send,render,pcm} = fixture();
  send(pcm(1,0,0.25)); send(pcm(2,256,0.5));
  const [left,right] = render(300);
  assert.deepEqual([...left], [...new Array(256).fill(0.25),...new Array(44).fill(0.5)]);
  assert.deepEqual([...right],[...left]);
  const progress = messages.filter(message => message.type === 'consumed');
  assert.deepEqual(progress.map(message => [message.sequence,message.consumedFrames]),[[1n,256],[2n,44]]);
});

test('the four-packet hard bound rejects overflow without replacing queued output', () => {
  const {messages,send,render,pcm} = fixture();
  for(let index=0;index<4;index++)send(pcm(index+1,index*256,(index+1)/4));
  send(pcm(5,1024,-1));
  assert.equal(messages.filter(message => message.type === 'error' && message.sequence === 5n).length,1);
  for(let index=0;index<4;index++) {
    const [left] = render(256);
    assert.deepEqual([...left],new Array(256).fill((index+1)/4));
  }
  assert.deepEqual([...render()[0]],new Array(128).fill(0));
  assert.equal(messages.filter(message => message.type === 'consumed').at(-1).sequence,4n);
});

test('silence inserted during underrun earns no credit and playback re-primes before advancing', () => {
  const {messages,send,render,pcm} = fixture();
  send(pcm(1,0,0.25));send(pcm(2,256,0.5));
  render(512);
  const before = messages.filter(message => message.type === 'consumed').length;
  assert.deepEqual([...render()[0]],new Array(128).fill(0));
  assert.equal(messages.filter(message => message.type === 'consumed').length,before);
  send(pcm(3,512,0.75));
  assert.deepEqual([...render()[0]],new Array(128).fill(0));
  assert.equal(messages.filter(message => message.type === 'consumed').length,before);
  send(pcm(4,768,1));
  assert.deepEqual([...render()[0]],new Array(128).fill(0.75));
  assert.equal(messages.filter(message => message.type === 'consumed').at(-1).consumedFrames,128);
});

test('pause freezes queued samples and consumption until an explicit resume', () => {
  const {messages,send,render,pcm} = fixture();
  send(pcm(1,0,0.25));send(pcm(2,256,0.5));
  render();
  send({type:'pause',request:7});
  assert.equal(messages.at(-1).type,'paused');
  assert.equal(messages.at(-1).request,7);
  const frozen=messages.at(-1);
  assert.equal(frozen.epoch,1n);
  assert.equal(frozen.renderedFrames,128);
  assert.equal(frozen.queuedFrames,384);
  const before = messages.filter(message => message.type === 'consumed').length;
  for(let index=0;index<3;index++)assert.deepEqual([...render()[0]],new Array(128).fill(0));
  assert.equal(messages.filter(message => message.type === 'consumed').length,before);
  send({type:'resume',request:8});
  assert.equal(messages.at(-1).type,'resumed');
  assert.equal(messages.at(-1).renderedFrames,frozen.renderedFrames);
  assert.equal(messages.at(-1).queuedFrames,frozen.queuedFrames);
  assert.deepEqual([...render()[0]],new Array(128).fill(0.25));
  assert.equal(messages.filter(message => message.type === 'consumed').at(-1).consumedFrames,256);
});

test('wrong identities and a reset cannot acknowledge or revive cancelled PCM', () => {
  const {messages,send,render,pcm} = fixture();
  send(pcm(1,0,0.25));
  for(const invalid of [pcm(1,0,1),pcm(2,257,1),pcm(2,256,1,2n)])send(invalid);
  assert.equal(messages.filter(message => message.type === 'error').length,3);
  send({type:'reset',epoch:2n});
  send(pcm(2,256,1,1n));
  send(pcm(1,0,-0.5,2n));send(pcm(2,256,-0.25,2n));
  assert.deepEqual([...render()[0]],new Array(128).fill(-0.5));
  const progress = messages.filter(message => message.type === 'consumed');
  assert.equal(progress.length,1);
  assert.equal(progress[0].epoch,2n);
  assert.equal(progress[0].sequence,1n);
});
