import { createBrowserRuntime } from './runtime.mjs';
import { createVerticalAcceptance } from './vertical-acceptance.mjs';

const container = document.querySelector('#runtime-container');
const bundleUrl = new URLSearchParams(location.search).get('bundle');
const output = document.querySelector('#acceptance-evidence');
const visibleLog = document.querySelector('#log');
const state = document.querySelector('#state'), status = document.querySelector('#status');
const record = { schema: 1, host: 'acceptance-host-v1', created_at: new Date().toISOString(), runtimeUrl: '/', bundleUrl, actions: [], sessions: [] };
let driver = null, unsubscribe = null, creating = false;
const brief = error => ({ code: error?.code ?? null, message: error?.message ?? String(error) });
function summary(result) {
  if (result === undefined || result === null) return 'acknowledged';
  if (typeof result !== 'object') return String(result).slice(0, 160);
  if (typeof result.message === 'string') return `${result.code ?? 'error'}: ${result.message.slice(0, 160)}`;
  const native = result.native ?? result;
  return JSON.stringify({ phase: result.phase, running: native.running, paused: native.paused, hasProcess: native.hasProcess,
    relaxedFpu: native.relaxedFpu, graceful: result.graceful, rootPersistenceFlushed: native.rootPersistenceFlushed });
}
function persist() {
  if (driver) record.current = driver.evidence();
  output.textContent = JSON.stringify(record);
}
function log(action, detail) {
  record.actions.push({ index: record.actions.length, at_ms: performance.now(), action, detail });
  visibleLog.textContent = record.actions.slice(-30).map(row => `${row.index} ${row.action} ${summary(row.detail)}`).join('\n');
  visibleLog.scrollTop = visibleLog.scrollHeight;
  persist();
}
function requireDriver() {
  if (!driver) throw new Error('Create a session first.');
  return driver;
}
async function perform(name, action) {
  log(`${name}-requested`, {});
  try {
    const result = await action();
    status.textContent = `${name}: ${summary(result)}`;
    log(`${name}-acknowledged`, result ?? null);
  } catch (error) {
    status.textContent = `${name}: rejected — ${error.message}`;
    log(`${name}-rejected`, brief(error));
  }
}
document.querySelector('#bundle').textContent = bundleUrl ? `Private local bundle: ${bundleUrl}` : 'Missing ?bundle=<same-origin-owned.wgb> query. No launch will be attempted.';
document.querySelector('#create').addEventListener('click', () => perform('create', async () => {
  if (creating || container.querySelector('iframe')) throw new Error('Stop or force dispose the existing session first.');
  if (!bundleUrl) throw new Error('Supply the private bundle URL through ?bundle=.');
  creating = true;
  try {
    if (driver) record.sessions.push(driver.evidence());
    unsubscribe?.(); unsubscribe = null;
    driver = await createVerticalAcceptance({ createRuntime: createBrowserRuntime, container, runtimeUrl: '/', bundleUrl });
    state.textContent = summary(driver.runtime.state);
    unsubscribe = driver.runtime.subscribe(next => {
      state.textContent = summary(next);
      log('state', next);
    });
    return driver.runtime.state;
  } finally { creating = false; }
}));
for (const [button, action] of [
  ['launch', () => requireDriver().launch()],
  ['pause', () => requireDriver().pause()],
  ['resume', () => requireDriver().resume()],
  ['status-button', () => requireDriver().status()],
  ['stop', () => requireDriver().stop()],
  ['dispose', () => { requireDriver().dispose(); }],
  ['assert-closed', () => requireDriver().assertClosed()],
]) document.getElementById(button).addEventListener('click', () => perform(button, action));
window.addEventListener('pagehide', () => { unsubscribe?.(); driver?.dispose(); }, { once: true });
persist();
