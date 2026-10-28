import assert from 'node:assert/strict';
import test from 'node:test';
import {DirtyPages} from '../runtime/v86/dirty-pages.js';
import {DIRECTORY,TABLES} from '../runtime/v86/layout.js';

function fixture() {
  const memory=new WebAssembly.Memory({initial:100});
  let flushes=0;
  const dirty=new DirtyPages(memory,0,()=>{flushes++;});
  const directory=new Uint32Array(memory.buffer,DIRECTORY,1024);
  const ptes=new Uint32Array(memory.buffer,TABLES,1<<20);
  const row=(page,slot)=>({page,slot,pte:TABLES+page*4});
  return {dirty,directory,ptes,row,flushes:()=>flushes};
}

test('partitioned dirty collection visits accessed groups and deduplicates physical aliases',()=>{
  const f=fixture();
  f.dirty.add(f.row(1,7));f.dirty.add(f.row(2,7));f.dirty.add(f.row(2048,8));
  f.directory[0]=0x27;f.directory[2]=7;
  f.ptes[1]=0x47;f.ptes[2]=0x47;f.ptes[2048]=7;
  const result=f.dirty.collect(true);
  assert.deepEqual(result.slots,[7]);
  assert.equal(result.scannedPtes,2);
  assert.equal(result.scannedDirectories,2);
  assert.equal(f.directory[0]&32,0);assert.equal(f.ptes[1]&64,0);assert.equal(f.ptes[2]&64,0);
  assert.equal(f.flushes(),1);
});

test('a read-only accessed group still resets the TLB before a later write',()=>{
  const f=fixture();f.dirty.add(f.row(5,2));f.directory[0]=0x27;f.ptes[5]=7;
  assert.deepEqual(f.dirty.collect(true).slots,[]);
  assert.equal(f.flushes(),1,'clearing directory accessed state also invalidates cached translations');
  f.directory[0]|=32;f.ptes[5]|=64;
  assert.deepEqual(f.dirty.collect(true).slots,[2]);assert.equal(f.flushes(),2);
  assert.equal(f.dirty.collect(true).scannedPtes,0);
  assert.equal(f.flushes(),2,'no newly accessed directory needs no extra TLB flush');
});

test('removed and replaced mappings cannot publish an old physical slot',()=>{
  const f=fixture(),old=f.row(1025,3),next=f.row(1025,4);
  f.dirty.add(old);f.dirty.remove(old);f.dirty.add(next);
  f.directory[1]=0x27;f.ptes[1025]=0x47;
  assert.deepEqual(f.dirty.collect(true).slots,[4]);
  f.dirty.remove(next);f.directory[1]|=32;f.ptes[1025]|=64;
  const result=f.dirty.collect(true);
  assert.deepEqual(result.slots,[]);assert.equal(result.scannedPtes,0);
});

test('legacy collection scans all mapped rows and leaves directory access state intact',()=>{
  const f=fixture();f.dirty.add(f.row(1,1));f.dirty.add(f.row(2048,2));
  f.directory[0]=0x27;f.ptes[2048]=0x47;
  const result=f.dirty.collect(false);
  assert.equal(result.scannedPtes,2);assert.equal(result.scannedDirectories,0);
  assert.deepEqual(result.slots,[2]);assert.equal(f.directory[0]&32,32);
  assert.equal(f.flushes(),1);
  assert.deepEqual(f.dirty.collect(false).slots,[]);assert.equal(f.flushes(),1);
});
