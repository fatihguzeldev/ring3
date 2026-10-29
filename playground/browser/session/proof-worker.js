import {Engine,pair} from '../../runtime/engine.js';
import {configure} from '../../runtime/configuration.js';
import {Media} from '../../runtime/media.js';

let engine,media,paused = false,started = 0,events = 0,batches = 0;
function status(state,message) {postMessage({type:'status',state,message,events,batches,engineHash:engine?.hash});}
self.onmessage = async ({data}) => {
  try {
    if (data.type === 'start') {
      engine = await Engine.create('/engine.wasm');
      media = new Media(engine,configure(engine),() => {});
      media.audio = {};
      media.publishDialogs = () => {};
      media.publishGdi = () => {};
      started = performance.now();
      status('running','engine input ready');
    } else if (data.type === 'input' && media) {
      const now = BigInt(Math.floor((performance.now()-started)*1e6));
      engine.ok('observe_cpu_clock',...pair(now));
      media.enqueue(data.events);
      media.update(now);
      events += data.events.length; batches++;
      postMessage({type:'accepted',events,batches,input:data.events});
    } else if (data.type === 'pause') {
      paused = true; status('paused','worker acknowledged pause');
    } else if (data.type === 'resume') {
      paused = false; status('running','worker acknowledged resume');
    }
  } catch (error) {status('error',error.message);}
};
