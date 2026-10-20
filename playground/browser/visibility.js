const terminal = new Set(['error','exited','stopped','disposed']);

export class VisibilityController {
  constructor(session,{hidden,onError,onDiagnostic=()=>{}}) {
    this.session = session;
    this.hidden = hidden;
    this.onError = onError;
    this.onDiagnostic = onDiagnostic;
    this.identity = 0;
    this.state = session.status().state;
    this.owned = false;
    this.command = null;
    this.manual = null;
    this.halted = false;
  }
  reset() {
    this.identity++;
    this.state = this.session.status().state;
    this.owned = false;
    this.command = null;
    this.manual = null;
    this.halted = false;
  }
  status(info) {
    this.state = info.state;
    if (terminal.has(info.state)) {
      this.halted = true;
      this.owned = false;
      this.command = null;
      this.manual = null;
      return;
    }
    if (this.command && info.state === this.command.state) this.command.acknowledged = true;
    this.reconcile();
  }
  changed() {
    this.reconcile();
  }
  manualToggle() {
    this.owned = false;
    this.manual = this.state === 'paused' ? 'running' : 'paused';
    this.reconcile();
  }
  reconcile() {
    if (this.halted || !['running','paused'].includes(this.state)) return;
    if (this.command) {
      if (!this.command.settled || !this.command.acknowledged) return;
      this.command = null;
    }
    if (this.manual) {
      if (this.state !== this.manual) {
        this.request(this.manual);
        return;
      }
      this.manual = null;
    }
    if (this.owned && this.state === 'running') this.owned = false;
    if (this.hidden() && this.state === 'running') {
      this.owned = true;
      this.request('paused');
    } else if (!this.hidden() && this.owned && this.state === 'paused') {
      this.request('running');
    }
  }
  request(state) {
    const identity = this.identity;
    const command = {state,settled:false,acknowledged:false};
    this.command = command;
    Promise.resolve().then(() => {
      if (this.identity !== identity || this.command !== command || this.halted) return;
      if (this.owned && !this.manual) this.onDiagnostic(state === 'paused' ? 'tab visibility requested pause' : 'tab visibility requested resume');
      return state === 'paused' ? this.session.pause() : this.session.resume();
    }).then(() => {
      if (this.identity !== identity || this.command !== command) return;
      command.settled = true;
      this.reconcile();
    }).catch(error => {
      if (this.identity !== identity || this.command !== command) return;
      this.command = null;
      this.owned = false;
      this.manual = null;
      this.onError(error);
    });
  }
}
