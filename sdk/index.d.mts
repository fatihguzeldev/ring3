export interface SessionStatus {
  readonly state: string;
  readonly backend: 'core'|'v86';
  readonly cpuHash?: string;
  readonly message?: string;
  readonly pc?: number;
  readonly stats?: Readonly<Record<string,unknown>>;
  readonly [name: string]: unknown;
}
export interface LaunchOptions {
  engineUrl?: string;
  backend?: 'core'|'v86';
  v86?: {wasmUrl?:string;sha256?:string};
  executable?: string;
  commandLine?: string;
  manifestUrl?: string;
  bytesUrl?: string;
  manifest?: {root?: string;executable?: string;files: ReadonlyArray<{id:number;path:string;size:number;role?:string;isDirectory?:boolean}>};
  localFiles?: Map<bigint,File>;
  execution?: {capacity?:number;budget?:number;instructions?:number;blocks?:number;chain?:boolean};
  frameProbe?: boolean;
  observability?: {level?:'off'|'basic'|'detailed';[name:string]:unknown};
}
export interface Session {
  launch(options:LaunchOptions):Promise<SessionStatus>;
  pause():Promise<SessionStatus>;
  resume():Promise<SessionStatus>;
  stop():Promise<SessionStatus>;
  status():SessionStatus;
  focus():void;
  captureMouse():Promise<void>;
  setVolume(value:number,muted?:boolean):void;
  armFrameProbe():void;
  dispose():Promise<void>;
}
export function createSession(options:{container:HTMLElement;engineUrl?:string;onStatus?:(status:SessionStatus)=>void;
  onDiagnostic?:(message:string,detail?:unknown)=>void;onAudioStats?:(stats:unknown)=>void;
  onCapture?:(captured:boolean)=>void;onObservation?:(status:unknown)=>void}):Session;
export function selectedFolder(files:Iterable<File>):(LaunchOptions & {configured:boolean;root:string;
  executables:Array<{path:string;label:string}>;selectedExecutable:string})|null;
