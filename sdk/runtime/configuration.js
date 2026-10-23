import {record,view} from './engine.js';
export const DEFAULT_EXECUTION = Object.freeze({capacity:null,budget:50000,instructions:2048,blocks:128,chain:true});
export const SYSTEM_MODULES = [
  ['kernel32',0x70000000],['user32',0x72000000],['dsound',0x73000000],
  ['dinput',0x74000000],['dinput8',0x75000000],['ole32',0x76000000],
  ['winmm',0x77000000],['advapi32',0x78000000],['d3d8',0x79000000],
  ['gdi32',0x68000000],['oleaut32',0x69000000],['shlwapi',0x6a000000],
  ['shell32',0x6b000000],['version',0x6c000000],['msimg32',0x6d000000],
  ['imm32',0x6e000000],['comctl32',0x6f000000],['commdlg32',0x7a000000,'comdlg32.dll'],
];
function config(engine,magic,operation,values) {const bytes=record(magic,64);values.forEach((v,i)=>view(bytes).setUint32(16+i*4,v,true));engine.request(bytes);engine.ok(operation,bytes.length);}
export function configure(h) {
  h.ok('configure_cpu_clock',1_000_000_000,0,0,0);
  h.ok('configure_windows_memory',4096,65536,64,1_048_576,4096,64*1024*1024);
  h.ok('configure_media',48000);const mediaEpoch=view(h.take(64)).getBigUint64(16,true);
  h.ok('configure_synchronization',1024,4096,8192,64);h.ok('configure_threads',64,1024,256,16,4096);
  h.ok('configure_module_limits',128,192*1024*1024,64,131072,8*1024*1024);
  h.ok('configure_user32',256,1024,16384,32,0x30000000,512);h.ok('set_user32_desktop',0,0,1920,1080);
  h.ok('configure_gdi',4096,1024,32,128*1024*1024,0x2c000000,4096,1920,1080);
  h.ok('configure_automation',4096,32*1024*1024,16,32);
  config(h,'R3CS','configure_dsound',[0x21000000,2048,0x22000000,1024,1024,128,8192,64*1024*1024,0]);
  config(h,'R3CI','configure_dinput',[0x23000000,256,0x23200000,128,1024,128,8192]);
  h.ok('configure_ole32',0x24000000,1024,2048,64);h.ok('configure_resources',0x28000000,4096,4096,64,1024*1024);
  h.ok('configure_registry',4096,8192,2048,16*1024*1024,8192,0);
  h.ok('configure_activation',256,1024,64,4*1024*1024,65536,256,16*1024*1024,0x2a000000,4096);
  const modes=[[640,480,60,22],[800,600,60,22],[1024,768,60,22],[1280,720,60,22],[1920,1080,60,22]], bytes=record('R3C8',128+modes.length*16,8), d=view(bytes);
  const values=[modes.length,0x25000000,4096,0x26000000,4096,4096,8,512,4096,256*1024*1024,8*1024*1024,2048,8*1024*1024,1024,64,16*1024*1024,800,600,22,60,4096,256,16,1,0x80000001];
  values.forEach((n,i)=>d.setUint32(16+i*4,n,true));modes.forEach((mode,i)=>mode.forEach((n,j)=>d.setUint32(128+i*16+j*4,n,true)));h.request(bytes);h.ok('configure_d3d8',bytes.length);
  SYSTEM_MODULES.forEach(([name,base])=>h.ok(`register_${name}_module`,base));
  return mediaEpoch;
}
