import {createSession,selectedFolder} from '../index.js';
const element = id => document.getElementById(id), query = new URL(location.href).searchParams;
const events = [], report = {backend:'core',events};
let local = null;
function record(type,value) {
  events.push({at_ms:performance.timeOrigin+performance.now(),type,value});
  if (events.length > 100) events.shift();
  element('ring3-report').textContent = JSON.stringify(report,(_key,value) => typeof value === 'bigint' ? String(value) : value);
}
const session = createSession({container:element('display'),engineUrl:query.get('engine') ?? '/engine.wasm',
  onStatus(value) {
    report.status = value; record('status',value);
    element('status').textContent = `${value.state}: ${value.message ?? ''}\n${JSON.stringify({pc:value.pc,stats:value.stats,presentation:value.presentation})}`;
  },
  onDiagnostic(message,detail) {record('diagnostic',{message,detail});},
  onAudioStats(value) {report.audio = value; record('audio',value);},
});
element('folder').addEventListener('change',() => {
  try {
    local = selectedFolder(element('folder').files);
    element('executable').replaceChildren(...(local?.executables ?? []).map(({path}) => {
      const option = document.createElement('option'); option.value = option.textContent = path; return option;
    }));
    record('local-folder',{files:local?.manifest.files.length ?? 0,executables:local?.executables.length ?? 0});
  } catch (error) {record('error',error.message); element('status').textContent = error.message;}
});
const actions = {
  launch: () => session.launch({...(local ?? {}),
    ...(local ? {executable:element('executable').value} : {}),
    commandLine:query.get('args') ?? undefined,
    frameProbe:query.get('frameProbe') === '1',observability:{level:query.get('observation') ?? 'off'},
  }),
  pause: () => session.pause(), resume: () => session.resume(), stop: () => session.stop(),
  capture: () => session.captureMouse(), probe: () => session.armFrameProbe(), dispose: () => session.dispose(),
};
for (const [name,action] of Object.entries(actions)) element(name).addEventListener('click',async () => {
  try {const result = await action(); record('action',{name,result});}
  catch (error) {record('action-error',{name,message:error.message}); element('status').textContent = error.message;}
});
