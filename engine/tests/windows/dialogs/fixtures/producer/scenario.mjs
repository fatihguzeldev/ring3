import {builder} from '../../../../support/pe32/image.mjs';
const u32 = value => {const bytes=Buffer.alloc(4);bytes.writeUInt32LE(value);return [...bytes];};

function strings(bytes) {
  const rva=0xc000,raw=0xa200,block=Buffer.alloc(512);
  for(const [at,value] of [[12,0x10000],[16,6],[20,0x80000020],[0x2c,0x10000],[0x30,1],[0x34,0x80000040],
    [0x4c,0x10000],[0x50,0x411],[0x54,0x60],[0x60,rva+0x80]]) block.writeUInt32LE(value>>>0,at);
  let at=0x80;
  for(let id=0;id<16;id++){
    const text=id===1?'見出し':id===2?'日本語の本文😀':'';
    const data=Buffer.from(text,'utf16le');block.writeUInt16LE(data.length/2,at);at+=2;data.copy(block,at);at+=data.length;
  }
  block.writeUInt32LE(at-0x80,0x64);
  bytes.writeUInt32LE(rva,0x98+112);bytes.writeUInt32LE(at,0x98+116);block.copy(bytes,raw);
}
export function image(){
  const b=builder([
    ['kernel32.dll',['GetCurrentThreadId','ExitProcess']],
    ['user32.dll',['MessageBoxIndirectW','MessageBoxA']],
  ],{callbacks:[{words:1,result:0xdeadbeef,record:'help'}]});
  for(const name of ['mainTid','helpSize','helpContext'])b.allocate(name);
  const a=b.addresses;
  const parameters=b.data('parameters',[40,0,0x400000,2,1,0x4001,0,0xabcdef,b.callbacks[0].entry,0x411]);
  const text=b.data('ansiText',[0xfd]);
  const caption=b.string('ansiCaption','ansi');
  b.external('GetCurrentThreadId',0xc0002,[],1,'mainTid');
  b.external('MessageBoxIndirectW',0x2005f,[parameters],2,undefined,'resource-dialog');
  b.external('MessageBoxA',0x2005c,[0,text,caption,4],7,undefined,'ansi-dialog');
  b.external('ExitProcess',0x10003,[0],'exit');
  const guest=b.seal(),callback=guest.callbacks[0],at=0x200+callback.entry-0x401000;
  const old=[...guest.bytes.subarray(at,at+callback.length)];
  const code=old.slice(0,-8);
  code.push(0x8b,0x44,0x24,4,0x8b,0x08,0x89,0x0d,...u32(a.helpSize));
  code.push(0x8b,0x48,16,0x89,0x0d,...u32(a.helpContext),...old.slice(-8));
  guest.bytes.set(code,at);callback.length=code.length;
  strings(guest.bytes);
  let start=guest.entry;
  guest.segments=guest.calls.map(call=>{const segment={...call,start,limit:guest.entry+call.end,words:call.name==='GetCurrentThreadId'?0:call.name==='MessageBoxA'?4:1,thread:1};start=segment.limit;return segment;});
  return guest;
}
