import assert from 'node:assert/strict';
import {test} from 'node:test';
import {Session} from '../runtime/session.js';

test('stopped acknowledgement never republishes unpublished work into a closed engine',() => {
  let emitted;
  const session = {
    stopped:true,
    code:{publish(){throw new Error('closed engine publication');},
      sampleUsage(){throw new Error('closed engine sampling');},stats:()=>({backend:'core'})},
    emit:value=>{emitted=value;},
  };
  Session.prototype.status.call(session,'stopped','stopped');
  assert.equal(emitted.state,'stopped');
  assert.equal(emitted.stats.backend,'core');
});
