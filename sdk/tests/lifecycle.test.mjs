import assert from 'node:assert/strict';
import test from 'node:test';
import {createSession} from '../index.js';
import {Session as BrowserSession} from '../browser/session.js';

const settle = async () => {for (let i = 0; i < 12; i++) await Promise.resolve();};
function documentFixture() {
  const document = {hasFocus: () => true, addEventListener() {}};
  document.createElement = () => ({
    ownerDocument:document, style:{}, children:[], attributes:new Set(),
    append(...children) {this.children.push(...children);},
    setAttribute(name) {this.attributes.add(name);},
    querySelector(selector) {return this.children.find(child => child.attributes.has(selector.slice(1,-1)));},
    addEventListener() {}, getContext() {return {};}, remove() {},
  });
  return document;
}
async function fixture(methods, check) {
  const document = documentFixture(), saved = {document:globalThis.document,window:globalThis.window};
  const originals = Object.fromEntries(['start','stop','dispose'].map(name => [name,BrowserSession.prototype[name]]));
  globalThis.document = document;
  globalThis.window = {addEventListener() {}};
  Object.assign(BrowserSession.prototype,methods,{async dispose() {this.status('disposed');}});
  const session = createSession({container:document.createElement('div')});
  try {await check(session);}
  finally {
    await session.dispose();
    Object.assign(BrowserSession.prototype,originals);
    for (const [name,value] of Object.entries(saved)) {
      if (value === undefined) delete globalThis[name]; else globalThis[name] = value;
    }
  }
}

test('stop reaches an in-flight launch before its running acknowledgement', async () => {
  let release, stops = 0, launched;
  await fixture({
    async start() {this.status('starting'); await new Promise(resolve => {release = resolve;});},
    async stop() {stops++; release(); this.status('stopped');},
  }, async session => {
    const launch = session.launch({});
    const outcome = launch.then(() => {launched = 'fulfilled';}, error => {launched = error.message;});
    try {
      await settle();
      assert.equal(session.status().state,'starting');
      const stopping = session.stop();
      assert.equal(session.stop(),stopping);
      void stopping.catch(() => {});
      await settle();
      assert.equal(stops,1);
      await stopping;
      await outcome;
      assert.match(launched,/stopped/);
      assert.equal(session.status().state,'stopped');
    } finally {release();}
  });
});

test('a failed stop rejects without claiming stopped and a later stop can retry', async () => {
  let stops = 0;
  await fixture({
    async start() {this.status('running');},
    async stop() {if (++stops === 1) throw new Error('drain failed'); this.status('stopped');},
  }, async session => {
    await session.launch({});
    await assert.rejects(session.stop(),/drain failed/);
    assert.equal(session.status().state,'running');
    assert.equal((await session.stop()).state,'stopped');
    assert.equal(stops,2);
  });
});

test('stop cancels a waiting launch and a later launch uses a fresh acknowledgement', async () => {
  let starts = 0, stops = 0;
  await fixture({
    async start() {starts++; this.status(starts === 1 ? 'loading' : 'running');},
    async stop() {stops++; this.status('stopped');},
  }, async session => {
    const launch = session.launch({});
    const outcome = launch.then(value => ({value}),error => ({error}));
    await settle();
    const stopping = session.stop();
    void stopping.catch(() => {});
    await settle();
    assert.equal(stops,1);
    await stopping;
    assert.match((await outcome).error?.message ?? '',/stopped/);
    assert.equal((await session.launch({})).state,'running');
    assert.equal(starts,2);
  });
});
