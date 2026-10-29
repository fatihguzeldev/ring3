const BUTTONS = [
  [[1,'OK']], [[1,'OK'],[2,'Cancel']], [[3,'Abort'],[4,'Retry'],[5,'Ignore']],
  [[6,'Yes'],[7,'No'],[2,'Cancel']], [[6,'Yes'],[7,'No']],
  [[4,'Retry'],[2,'Cancel']], [[2,'Cancel'],[10,'Try again'],[11,'Continue']],
];

export class DialogHost {
  constructor(element, {onResult, onHelp, onVisibility = () => {}}) {
    this.element = element;
    this.caption = element.querySelector('#dialog-caption');
    this.text = element.querySelector('#dialog-text');
    this.buttons = element.querySelector('#dialog-buttons');
    this.onResult = onResult;
    this.onHelp = onHelp;
    this.onVisibility = onVisibility;
    this.queue = [];
    this.current = null;
    element.addEventListener('cancel', event => {
      event.preventDefault();
      const choices = BUTTONS[this.current?.style & 15] ?? [];
      if (choices.some(([id]) => id === 2)) this.complete(2);
      else if (choices.length === 1 && choices[0][0] === 1) this.complete(1);
    });
  }
  show(request) {
    const key = `${request.epoch}:${request.id}`;
    if (this.current?.key === key || this.queue.some(item => item.key === key)) return;
    if (!BUTTONS[request.style & 15]) throw new Error(`unsupported dialog button set ${request.style & 15}`);
    if (this.queue.length >= 16) throw new Error('too many concurrent guest dialogs');
    this.queue.push({...request,key});
    this.next();
  }
  next() {
    if (this.current || this.queue.length === 0) return;
    this.current = this.queue.shift();
    this.caption.textContent = this.current.caption || 'Application';
    this.text.textContent = this.current.text;
    this.buttons.replaceChildren();
    const choices = BUTTONS[this.current.style & 15];
    const defaultIndex = Math.min((this.current.style >>> 8) & 3, choices.length - 1);
    choices.forEach(([id,label], index) => {
      const button = document.createElement('button');
      button.textContent = label;
      button.addEventListener('click', () => this.complete(id));
      if (index === defaultIndex) button.autofocus = true;
      this.buttons.append(button);
    });
    if (this.current.style & 0x4000) {
      const help = document.createElement('button');
      help.textContent = 'Help';
      help.addEventListener('click', () => this.onHelp(this.current));
      this.buttons.append(help);
    }
    this.onVisibility(true);
    this.element.showModal();
    this.buttons.children[defaultIndex].focus();
  }
  complete(button) {
    const request = this.current;
    if (!request) return;
    this.current = null;
    this.element.close();
    this.onResult({...request,button,error:0});
    this.onVisibility(false);
    this.next();
  }
  close() {
    this.current = null;
    this.queue.length = 0;
    this.element.close();
    this.onVisibility(false);
  }
}
