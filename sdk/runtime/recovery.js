// Immutable recovery metadata belongs to the selected native module allocation.
// No real-engine allocation, memory growth or owner-ID registry is involved.
export const RECOVERY_SECTION = 'ring3.recovery';
export const RECOVERY_IMPORT = 'recovery.base';
const U32_MAX = 0xffffffff;
const invalid = () => {throw new Error('invalid core recovery metadata');};

function u32(bytes,cursor,end) {
  let value=0;
  for(let shift=0;shift<=28;shift+=7) {
    if(cursor.at>=end)invalid();const byte=bytes[cursor.at++];
    if(shift===28&&(byte&0xf0))invalid();
    value+=(byte&127)*2**shift;
    if(!(byte&128))return value;
  }
  invalid();
}

export function recoveryBlob(bytes) {
  if(!(bytes instanceof Uint8Array)||bytes.length<8||
    ![0,97,115,109,1,0,0,0].every((value,index)=>bytes[index]===value))invalid();
  let found;
  const cursor={at:8};
  while(cursor.at<bytes.length) {
    const kind=bytes[cursor.at++],size=u32(bytes,cursor,bytes.length),end=cursor.at+size;
    if(end>bytes.length)invalid();
    if(kind===0) {
      const length=u32(bytes,cursor,end),nameEnd=cursor.at+length;
      if(nameEnd>end)invalid();
      const name=bytes.subarray(cursor.at,nameEnd);
      if(name.length===RECOVERY_SECTION.length&&
        [...name].every((value,index)=>value===RECOVERY_SECTION.charCodeAt(index))) {
        if(found)invalid();
        found={offset:nameEnd,bytes:bytes.subarray(nameEnd,end)};
      }
    }
    cursor.at=end;
  }
  return found;
}

export function inspectRecoveryBlob(bytes) {
  if(!(bytes instanceof Uint8Array)||bytes.length<40)invalid();
  const d=new DataView(bytes.buffer,bytes.byteOffset,bytes.byteLength);
  const version=d.getUint32(4,true),header=version===1?40:version===2?48:0;
  if(d.getUint32(0,true)!==0x44523352||!header||bytes.length<header)invalid();
  const words=d.getUint32(8,true),bits=d.getUint32(12,true),packed=d.getUint32(16,true),
    dictionary=d.getUint32(20,true),packedOffset=d.getUint32(24,true),dictionaryOffset=d.getUint32(28,true),
    identity=d.getBigUint64(32,true),mode=version===2?d.getUint32(40,true):0,
    highCount=version===2?d.getUint32(44,true):0;
  const expectedBits=Math.max(1,32-Math.clz32(dictionary-1)),totalBits=words*bits;
  const entryBytes=mode===0?8:mode===1?5:mode===2?6:0;
  const highTableOffset=dictionaryOffset+dictionary*entryBytes;
  if(!words||!dictionary||dictionary>words||!identity||bits!==expectedBits||bits>32||
    totalBits>U32_MAX||packed!==Math.ceil(totalBits/64)||packedOffset!==header||
    dictionaryOffset!==header+8*packed||!entryBytes||
    (mode===0?highCount!==0:!highCount||highCount>dictionary||
      (mode===1?highCount>256:highCount<=256||highCount>65536)||
      dictionary*entryBytes+highCount*4>=dictionary*8)||
    highTableOffset+highCount*4!==bytes.length||bytes.length>U32_MAX)invalid();
  if(mode!==0)for(let index=0;index<dictionary;index++) {
    const at=dictionaryOffset+index*entryBytes+4;
    if((mode===1?d.getUint8(at):d.getUint16(at,true))>=highCount)invalid();
  }
  return {identity,version,words,bits,packed,dictionary,packedOffset,dictionaryOffset,
    mode,highCount,highTableOffset,byteLength:bytes.length};
}

export function moduleRecovery(module) {
  const imports=WebAssembly.Module.imports(module).filter(item=>item.name===RECOVERY_IMPORT);
  const sections=WebAssembly.Module.customSections(module,RECOVERY_SECTION);
  if(!imports.length&&!sections.length)return undefined;
  if(imports.length!==1||imports[0].module!=='ring3'||imports[0].kind!=='global'||sections.length!==1)invalid();
  const bytes=new Uint8Array(sections[0]);return {bytes,...inspectRecoveryBlob(bytes)};
}

function pointer32(value) {
  if(!Number.isInteger(value)||value < -0x80000000||value>U32_MAX)invalid();
  return value<0?value+0x100000000:value;
}

export function bindNativeRecovery(module,source,{memory,pointer,length,identity}) {
  const metadata=moduleRecovery(module);
  if(!metadata)return undefined;
  if(!(memory instanceof WebAssembly.Memory)||
    typeof identity!=='bigint'||identity<=0n||identity>0xffffffffffffffffn)invalid();
  pointer=pointer32(pointer);
  if(!pointer||!Number.isInteger(length)||length<8||length>16*1024*1024||
    !(source instanceof Uint8Array)||source.length!==length||pointer+length>memory.buffer.byteLength)invalid();
  const original=recoveryBlob(source);
  if(!original||inspectRecoveryBlob(original.bytes).identity!==identity||metadata.identity!==identity||
    metadata.bytes.length!==original.bytes.length||
    !metadata.bytes.every((value,index)=>value===original.bytes[index]))invalid();
  // Source may be transformed for diagnostics. Its recovery section must still
  // be exactly the original owner's blob, whose complete native module bytes
  // are validated at this captured pointer before any instantiation.
  const native=new Uint8Array(memory.buffer,pointer,length);
  if(!source.every((value,index)=>value===native[index]))invalid();
  const base=pointer+original.offset;
  if(base>U32_MAX||base+metadata.bytes.length>pointer+length)invalid();
  return new WebAssembly.Global({value:'i32',mutable:false},base>=0x80000000?base-0x100000000:base);
}

// Instrumented modules may reuse a retained unit's already-bound immutable
// global even when a different owner is currently selected in the engine.
export function retainedRecoveryBase(module,memory,base) {
  const metadata=moduleRecovery(module);if(!metadata)return undefined;
  if(!(memory instanceof WebAssembly.Memory)||!(base instanceof WebAssembly.Global))invalid();
  const pointer=pointer32(base.value);
  if(!pointer||pointer+metadata.bytes.length>memory.buffer.byteLength)invalid();
  const native=new Uint8Array(memory.buffer,pointer,metadata.bytes.length);
  if(!metadata.bytes.every((value,index)=>value===native[index]))invalid();
  return base;
}
