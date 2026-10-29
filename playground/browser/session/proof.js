import {Session} from '../session.js';
import {InputHost} from '../input.js';
import {VisibilityController} from '../visibility.js';

const surface = document.querySelector('#surface'), result = document.querySelector('#result');
const pause = document.querySelector('#pause'), history = document.querySelector('#history');
let control,events = 0,batches = 0,audio;
const rows = [];
function log(text) {rows.push(text); history.textContent = rows.slice(-30).join('\n');}
const session = Object.assign(Object.create(Session.prototype),{
  state:'idle',generation:0,failed:false,worker:null,
  audio:{pause:() => audio.suspend(),resume:() => audio.resume()},
  onStatus:info => {
    control.status(info);
    pause.disabled = !['running','paused'].includes(info.state);
    pause.textContent = info.state === 'paused' ? 'resume' : 'pause';
    result.textContent = `${info.state} · accepted events ${events} · batches ${batches} · ${info.message}`;
    log(`${info.state}: ${info.message} · hidden=${document.hidden} focused=${document.hasFocus()}`);
  },
});
session.input = new InputHost(surface,{send:input => session.post({type:'input',events:input})});
control = new VisibilityController(session,{hidden:() => document.hidden,onError:error => log(error.message)});
document.addEventListener('visibilitychange',() => control.changed());
pause.addEventListener('click',() => control.manualToggle());
document.querySelector('#start').addEventListener('click',async event => {
  event.target.disabled = true;
  audio = new AudioContext();
  await audio.resume();
  const worker = new Worker(new URL('./proof-worker.js',import.meta.url),{type:'module'});
  session.worker = worker;
  session.generation++;
  worker.onmessage = ({data}) => {
    if (data.type === 'accepted') {
      events = data.events; batches = data.batches;
      result.textContent = `${session.state} · accepted events ${events} · batches ${batches}`;
      log(`accepted ${JSON.stringify(data.input)}`);
    } else session.message(data,worker).catch(error => log(error.message));
  };
  session.post({type:'start'});
});
