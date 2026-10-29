import assert from "node:assert/strict";
import { test } from "node:test";
import { ActivationCallbacks } from "./activation.js";

function request({provider=1, message=0x1c, active=1}={}) {
  const data=new DataView(new ArrayBuffer(96));
  for(const [offset,value] of [[0,0x43573352],[4,0x10001],[8,96],[12,provider],
    [16,0x1000],[20,4],[32,3],[36,message],[40,active],[44,2]]) data.setUint32(offset,value,true);
  data.setBigUint64(24,7n,true);
  return data;
}
test("only accepted user32 activation callbacks read a clock and completion is recorded once",()=>{
  let calls=0; const tracker=new ActivationCallbacks(()=>++calls);
  assert.equal(tracker.begin(request({provider:4}),1),undefined);
  assert.equal(tracker.begin(request({message:6}),1),undefined);
  assert.equal(calls,0);
  const record=tracker.begin(request(),1);
  assert.deepEqual(record,{thread:1,procedure:0x1000,continuation:"7",window:3,
    active:1,otherThread:2,startedAtMs:1});
  assert.equal(tracker.snapshot().records[0].completedAtMs,undefined);
  tracker.complete(record); tracker.complete(record);
  assert.equal(calls,2);
  const snapshot=tracker.snapshot(); snapshot.records[0].active=-1;
  assert.equal(tracker.snapshot().records[0].active,1);
  assert.equal(tracker.snapshot().records[0].completedAtMs,2);
});
test("activation diagnostic history is bounded and overwritten records remain visible",()=>{
  const tracker=new ActivationCallbacks(()=>0);
  for(let i=0;i<18;i++) tracker.begin(request({active:i%2}),1);
  assert.equal(tracker.snapshot().records.length,16);
  assert.equal(tracker.snapshot().overwritten,2);
});
