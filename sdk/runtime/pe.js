import {view} from './engine.js';

export async function inspectImports(size,read) {
  const pages=new Map();
  const span=async(offset,length)=>{
    if(!Number.isSafeInteger(offset)||!Number.isSafeInteger(length)||offset<0||length<0||offset+length>size)throw new Error('PE metadata points outside the selected file');
    const bytes=new Uint8Array(length);
    for(let at=0;at<length;){
      const page=Math.floor((offset+at)/4096)*4096;
      if(!pages.has(page)){if(pages.size>=64)throw new Error('PE inspection exceeded256 KiB of metadata');pages.set(page,await read(page,Math.min(4096,size-page)));}
      const source=pages.get(page),start=offset+at-page,count=Math.min(length-at,source.length-start);
      if(!count)throw new Error('PE metadata read made no progress');
      bytes.set(source.subarray(start,start+count),at);at+=count;
    }
    return view(bytes);
  };
  const dos=await span(0,64);
  if(dos.getUint16(0,true)!==0x5a4d)throw new Error('selected executable lacks an MZ header');
  const offset=dos.getUint32(60,true),coff=await span(offset,24);
  if(coff.getUint32(0,true)!==0x4550||coff.getUint16(4,true)!==0x14c)throw new Error('the playground accepts i386 PE32 executables');
  const count=coff.getUint16(6,true),optionalSize=coff.getUint16(20,true);
  if(count>96||optionalSize<112||optionalSize>4096)throw new Error('PE header exceeds the playground inspection bounds');
  const header=await span(offset+24,optionalSize+count*40);
  if(header.getUint16(0,true)!==0x10b)throw new Error('the playground accepts PE32, not PE32+');
  if(header.getUint32(92,true)<2)return [];
  const imports=header.getUint32(104,true),length=header.getUint32(108,true);
  if(!imports||!length)return [];
  if(length>131072)throw new Error('PE import directory exceeds the playground inspection budget');
  const headersSize=header.getUint32(60,true);
  const locate=(rva,bytes)=>{
    if(rva<headersSize&&rva+bytes<=headersSize)return rva;
    for(let i=0;i<count;i++){
      const at=optionalSize+i*40,base=header.getUint32(at+12,true),rawSize=header.getUint32(at+16,true),raw=header.getUint32(at+20,true);
      if(rva>=base&&rva-base+bytes<=rawSize)return raw+rva-base;
    }
    throw new Error(`PE import RVA0x${rva.toString(16)} has no file-backed section`);
  };
  const names=[];
  for(let at=0;at+20<=length;at+=20){
    const descriptor=await span(locate(imports+at,20),20);
    if([0,4,8,12,16].every(n=>descriptor.getUint32(n,true)===0))return names;
    const nameRva=descriptor.getUint32(12,true);let name='';
    for(let index=0;index<256;index++){
      const byte=(await span(locate(nameRva+index,1),1)).getUint8(0);
      if(!byte){if(!name)throw new Error('PE import has an empty module name');names.push(name);break;}
      if(byte>127)throw new Error('PE import has a non-ASCII module name');
      name+=String.fromCharCode(byte);
      if(index===255)throw new Error('PE module name exceeds256 bytes');
    }
  }
  throw new Error('PE import directory lacks its terminating descriptor');
}
