import assert from 'node:assert/strict';
import test from 'node:test';
import {Media} from '../runtime/media.js';

function fixture() {
  const events=[],acknowledged=[],queries=[];
  let pending=null,queryFailure=null,ackFailure=null;
  const engine={
    media(operation) {
      assert.equal(operation,'pending_user32_cursor');
      queries.push(pending?.revision??0n);
      if(queryFailure)throw queryFailure;
      return {values:pending?[...pending.values]:[0n,0n,0n,0n,0n]};
    },
    ok(operation,...args) {
      if(operation==='sync_windows_input')return;
      assert.equal(operation,'acknowledge_user32_cursor');
      const revision=BigInt(args[0]>>>0)|(BigInt(args[1]>>>0)<<32n);
      acknowledged.push({revision,args});
      if(ackFailure)throw ackFailure;
      if(!pending||pending.revision!==revision)
        throw Object.assign(new Error('native cursor invalid token'),{operation,status:14});
      pending=null;
    },
  };
  const media=new Media(engine,7n,event=>events.push(event),null);
  media.fillAudio=media.publishDialogs=media.publishGdi=()=>{};
  return {media,events,acknowledged,queries,
    set(revision) {pending=revision?{revision,values:[revision,256n|(42n<<32n),9n|(10n<<32n),0n,0n]}:null;},
    pending:()=>pending?{revision:pending.revision,values:[...pending.values]}:null,
    failQuery:error=>{queryFailure=error;},failAck:error=>{ackFailure=error;},
    update:()=>media.update(0n),
  };
}

test('a newer native cursor survives an older emitted revision ACK and is published next',()=>{
  const f=fixture();f.set(1n);f.update();
  assert.deepEqual(f.events.map(event=>event.revision),[1n]);assert.equal(f.media.lastCursor,1n);
  f.set(2n);const newer=f.pending();
  assert.doesNotThrow(()=>f.media.cursorApplied(1n));
  assert.deepEqual(f.acknowledged,[]);assert.deepEqual(f.pending(),newer);assert.equal(f.media.lastCursor,1n);
  f.update();assert.deepEqual(f.events.map(event=>event.revision),[1n,2n]);
  f.media.cursorApplied(2n);
  assert.deepEqual(f.acknowledged.map(row=>row.revision),[2n]);assert.equal(f.pending(),null);
});

test('ACKs older than the latest emitted cursor do not query or acknowledge native state',()=>{
  const f=fixture();f.set(1n);f.update();f.set(2n);f.update();
  const queryCount=f.queries.length,before=f.pending();
  assert.doesNotThrow(()=>f.media.cursorApplied(1n));
  assert.equal(f.queries.length,queryCount);assert.deepEqual(f.acknowledged,[]);assert.deepEqual(f.pending(),before);
  assert.equal(f.media.lastCursor,2n);
});

test('an un-emitted native revision is preserved until update publishes it',()=>{
  const f=fixture();f.set(2n);f.update();f.set(3n);
  const before=f.pending();
  assert.doesNotThrow(()=>f.media.cursorApplied(2n));assert.deepEqual(f.pending(),before);assert.deepEqual(f.acknowledged,[]);
  assert.throws(()=>f.media.cursorApplied(3n),/cursor|revision|issued/i);
  assert.deepEqual(f.pending(),before);assert.deepEqual(f.acknowledged,[]);assert.equal(f.media.lastCursor,2n);
  f.update();f.media.cursorApplied(3n);
  assert.deepEqual(f.events.map(event=>event.revision),[2n,3n]);
  assert.deepEqual(f.acknowledged.map(row=>row.revision),[3n]);assert.equal(f.pending(),null);
});

test('a duplicate ACK of an already cleared cursor is idempotent without resetting last emitted revision',()=>{
  const f=fixture();f.set(2n);f.update();f.media.cursorApplied(2n);
  assert.doesNotThrow(()=>f.media.cursorApplied(2n));
  assert.deepEqual(f.acknowledged.map(row=>row.revision),[2n]);assert.equal(f.pending(),null);assert.equal(f.media.lastCursor,2n);
  f.update();assert.deepEqual(f.events.map(event=>event.revision),[2n]);
});

test('zero, unknown future and never-issued cursor ACKs are rejected before native acknowledgement',()=>{
  for(const [publish,revision] of [[false,1n],[true,0n],[true,2n],[true,0x100000001n]]) {
    const f=fixture();f.set(1n);if(publish)f.update();const before=f.pending();
    assert.throws(()=>f.media.cursorApplied(revision),/cursor|revision|issued/i);
    assert.deepEqual(f.acknowledged,[]);assert.deepEqual(f.pending(),before);
    assert.equal(f.media.lastCursor,publish?1n:0n);
  }
});

test('a native pending revision behind the matching emitted ACK is an ordering error',()=>{
  const f=fixture();f.set(2n);f.update();f.set(1n);const before=f.pending();
  assert.throws(()=>f.media.cursorApplied(2n),/cursor|revision|order/i);
  assert.deepEqual(f.acknowledged,[]);assert.deepEqual(f.pending(),before);assert.equal(f.media.lastCursor,2n);
});

test('native cursor query and strict-ACK failures are propagated instead of mistaken for obsolete ACKs',()=>{
  for(const [stage,status] of [['query',9],['ack',9],['ack',14]]) {
    const f=fixture();f.set(2n);f.update();const before=f.pending();
    const failure=Object.assign(new Error('unexpected native cursor fault'),{status});
    if(stage==='query')f.failQuery(failure);else f.failAck(failure);
    assert.throws(()=>f.media.cursorApplied(2n),error=>error===failure);
    assert.deepEqual(f.pending(),before);assert.equal(f.media.lastCursor,2n);
    assert.equal(f.acknowledged.length,stage==='query'?0:1);
  }
});

test('matching cursor acknowledgement normalizes and preserves both halves of a 64-bit emitted revision',()=>{
  const f=fixture(),revision=0x100000002n;f.set(revision);f.update();f.media.cursorApplied(revision.toString());
  assert.deepEqual(f.acknowledged,[{revision,args:[2,1]}]);assert.equal(f.pending(),null);
});
