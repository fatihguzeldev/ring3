import {createGraphicsHost} from './index.js';
import {PacketStream,require} from './packet.js';
import {GdiHost} from '../browser/gdi.js';

export function runGdiValidation() {
  const canvas = document.createElement('canvas');
  const windows = new Map([[1,{style:0}],[2,{style:0x40000000,parent:1,x:1,y:1}]]);
  const host = new GdiHost(canvas,windows);
  const frame = {window:1,windowDc:false,width:2,height:2,stride:8,format:1};
  const pixels = new Uint8Array([0x11,0x22,0x33,0,0x11,0x22,0x33,0,0x11,0x22,0x33,0,0x11,0x22,0x33,0]);
  host.accept(frame,pixels);
  require(pixels.every((value,index) => value === [51,34,17,255][index % 4]),'GDI exact BGRX conversion and opaque alpha');
  host.accept({...frame,window:2,width:1,height:1,stride:4},new Uint8Array([0,255,0,0]));
  const pixel = (x,y) => Array.from(canvas.getContext('2d').getImageData(x,y,1,1).data);
  // canvas readback can contain one-bit noise from browser fingerprinting protection.
  const matches = (actual,expected) => actual.every((value,index) => Math.abs(value - expected[index]) <= 1);
  const color = pixel(0,0), child = pixel(1,1);
  require(matches(color,[51,34,17,255]),`GDI BGRX color and opaque alpha: ${color}`);
  require(matches(child,[0,255,0,255]),`GDI child position and composition: ${child}`);
  host.presented(1);
  require(pixel(0,0)[3] <= 1 && matches(pixel(1,1),[0,255,0,255]),'GDI overlay after D3D presentation');
  host.clear();
  require(canvas.hidden,'GDI session cleanup');
  return {ok:true,color,child};
}

class Writer {
  constructor(){this.parts=[];this.length=0;}
  bytes(b){this.parts.push(b);this.length+=b.length;return this;}
  u32(n){const b=new Uint8Array(4);new DataView(b.buffer).setUint32(0,n,true);return this.bytes(b);}
  u64(n){const b=new Uint8Array(8);new DataView(b.buffer).setBigUint64(0,BigInt(n),true);return this.bytes(b);}
  words(xs){for(const x of xs)this.u32(x);return this;}
  floats(xs){for(const x of xs){const b=new Uint8Array(4);new DataView(b.buffer).setFloat32(0,x,true);this.bytes(b);}return this;}
  resource(slot){return this.u64(1).words([slot,1]);}
  version(slot,version){return this.resource(slot).u64(version);}
  surface(slot,version){return this.version(slot,version).words([0,0]);}
  targets(version){return this.surface(0,version).u32(0);}
  finish(){const out=new Uint8Array(this.length);let at=0;for(const p of this.parts){out.set(p,at);at+=p.length;}return out;}
}
const w=()=>new Writer();
function definition(slot,type,format,width,height,length){return w().resource(slot).words([type,format,type===1?1:0,0,width,height,1,1,0,1,0,0,1,width,height,1,type===1?width*4:length,length,length,0,0]);}
function state(id,shader=0x44,pixel=0){
  const rs=new Array(174).fill(0);for(const [i,v] of [[8,3],[19,2],[20,1],[22,1],[23,4],[25,8],[168,15],[171,1],[172,3]])rs[i]=v;
  const out=w().u64(id).words([2,174,...rs,8]);
  for(let i=0;i<8;i++){const s=new Array(29).fill(0);s[1]=1;s[4]=1;s[13]=s[14]=s[16]=s[17]=1;out.words([29,...s]);}
  out.words([266,9,...new Array(9).fill(0)]).floats(new Array(17).fill(0)).words([0,6]).floats(new Array(24).fill(0)).words([0,0,0,0,32,32]).floats([0,1]).words([shader,pixel,0,96]);
  out.floats(new Array(384).fill(0)).u32(8).floats(new Array(32).fill(0));return out;
}
function draw(id,slot,stride,version=3){const out=w().u64(id).words([4,1]).targets(version).words(new Array(8).fill(0));for(let i=0;i<16;i++){out.u32(i===0?1:0);if(i===0)out.version(slot,2).u32(stride);}return out.words([0,0,0]);}
function program(stage,operation,dest,sources){const out=w().words([stage,1,0,1,operation,0,1,...dest,15,0,0]);for(let i=0;i<3;i++){out.u32(sources[i]?1:0);if(sources[i])out.words([...sources[i],0xe4,0,0]);}return out.u32(0);}

export async function runGraphicsValidation(canvas){
  const commands=[],add=(opcode,payload)=>commands.push({opcode,bytes:payload.finish()}),vertex=w().floats([4,4,0.5,1]).u32(0xffff0000).floats([28,4,0.5,1]).u32(0xffff0000).floats([16,28,0.5,1]).u32(0xffff0000).finish();
  add(1,definition(0,1,21,32,32,4096));add(1,definition(1,6,100,60,1,60));
  add(3,w().version(1,2).words([0,0,60,60,60,1,1,60]).bytes(vertex));
  add(9,w().targets(2).u32(1).u32(0xff0000ff).floats([1]).words([0,0]));add(6,state(1));add(8,draw(1,1,20));
  const readback=seq=>w().u64(1).u64(seq).surface(0,3).words([1,1,21,32,32,1,128,4096,4096]);
  add(14,readback(1));
  // second pass uses a real programmable vertex declaration and pixel IR.
  const vs=w().u32(0x80000001).u32(1).words([2,0,0,0,3,0,16,5,4]).words([20,...new Array(15).fill(0),0,0,1]);
  vs.bytes(program(1,1,[5,0],[[1,0]]).finish());add(4,vs);
  const ps=w().u32(0x80000002).u32(2);ps.bytes(program(2,1,[0,0],[[2,0]]).finish());add(4,ps);
  const programmable=state(2,0x80000001,0x80000002);const bytes=programmable.finish();
  // the first pixel constant is green; constants are part of the pipeline snapshot.
  const data=new DataView(bytes.buffer);const pcStart=bytes.length-8*16;data.setFloat32(pcStart+4,1,true);data.setFloat32(pcStart+12,1,true);add(6,w().bytes(bytes));
  const clip=w().floats([-0.75,0.75,0.5,1]).u32(0xffffffff).floats([0.75,0.75,0.5,1]).u32(0xffffffff).floats([0,-0.75,0.5,1]).u32(0xffffffff).finish();
  add(3,w().version(1,2).words([0,0,60,60,60,1,1,60]).bytes(clip));add(8,draw(2,1,20));add(14,readback(2));
  add(11,w().u64(1).words([1,0,1,0]).surface(0,3).words([0,0,0]));
  const body=w();commands.forEach((c,i)=>body.bytes(w().words([]).finish()).bytes((()=>{const h=new Uint8Array(32),v=new DataView(h.buffer);v.setUint16(0,c.opcode,true);v.setUint16(2,3,true);v.setUint32(4,32+c.bytes.length,true);v.setBigUint64(8,BigInt(i+1),true);v.setUint32(20,c.bytes.length,true);return h;})()).bytes(c.bytes));
  const header=new Uint8Array(64),v=new DataView(header.buffer);header.set(new TextEncoder().encode('R3D8'));v.setUint16(4,2,true);v.setUint16(6,8,true);v.setUint32(8,64+body.length,true);v.setUint32(12,commands.length,true);v.setBigUint64(16,1n,true);v.setBigUint64(24,1n,true);v.setBigUint64(32,1n,true);v.setBigUint64(40,BigInt(commands.length),true);
  const packet=w().bytes(header).bytes(body.finish()).finish(),stream=new PacketStream();stream.decode(packet);let refused=false;try{stream.decode(packet);}catch{refused=true;}require(refused,'packet replay guard');
  const host=await createGraphicsHost(canvas);try{
    const result=await host.consume(packet.buffer);require(result.readbacks.length===2,'two actual GPU readbacks');
    const pixel=(bytes,x,y)=>Array.from(bytes.subarray((y*32+x)*4,(y*32+x)*4+4));
    require(JSON.stringify(pixel(result.readbacks[0].bytes,16,12))==='[0,0,255,255]','fixed-function GPU triangle red center');
    require(JSON.stringify(pixel(result.readbacks[0].bytes,0,0))==='[255,0,0,255]','GPU blue clear outside triangle');
    require(JSON.stringify(pixel(result.readbacks[1].bytes,16,12))==='[0,255,0,255]','programmable GPU triangle green center');
    return {ok:true,commands:commands.length,epoch:String(result.epoch),sequence:String(result.sequence),fixedCenter:pixel(result.readbacks[0].bytes,16,12),programCenter:pixel(result.readbacks[1].bytes,16,12),corner:pixel(result.readbacks[0].bytes,0,0)};
  }finally{host.close();}
}

export async function runGraphicsBatchValidation(canvas) {
  const commands=[],add=(opcode,payload)=>commands.push({opcode,bytes:payload.finish()});
  const vertex=w();
  for(const [x,y] of [[-1,-1],[64,-1],[-1,64]])vertex.floats([x,y,0.5,1]).u32(0xffff0000);
  for(const slot of [0,2,3])add(1,definition(slot,1,21,32,32,4096));
  add(1,definition(1,6,100,60,1,60));
  add(3,w().version(1,2).words([0,0,60,60,60,1,1,60]).bytes(vertex.finish()));
  add(9,w().targets(2).u32(1).u32(0xff0000ff).floats([1]).words([0,0]));
  add(6,state(1));
  for(let index=0;index<16;index++)add(8,draw(1,1,20,3+index));
  add(10,w().surface(0,18).surface(2,2).u32(0));
  const green=new Uint8Array(4096);
  for(let index=0;index<green.length;index+=4)green.set([0,255,0,255],index);
  add(3,w().version(0,19).words([0,0,128,4096,128,32,1,4096]).bytes(green));
  add(10,w().surface(0,19).surface(3,2).u32(0));
  add(2,w().version(0,19));
  for(const [sequence,slot] of [[1,2],[2,3]])
    add(14,w().u64(1).u64(sequence).surface(slot,2).words([1,1,21,32,32,1,128,4096,4096]));
  for(const slot of [2,3])add(11,w().u64(1).words([1,0,1,0]).surface(slot,2).words([0,0,0]));

  const body=w();
  commands.forEach(({opcode,bytes},index)=>{
    const header=new Uint8Array(32),v=new DataView(header.buffer);
    v.setUint16(0,opcode,true);v.setUint16(2,3,true);v.setUint32(4,32+bytes.length,true);
    v.setBigUint64(8,BigInt(index+1),true);v.setUint32(20,bytes.length,true);
    body.bytes(header).bytes(bytes);
  });
  const header=new Uint8Array(64),v=new DataView(header.buffer);
  header.set(new TextEncoder().encode('R3D8'));v.setUint16(4,2,true);v.setUint16(6,8,true);
  v.setUint32(8,64+body.length,true);v.setUint32(12,commands.length,true);
  v.setBigUint64(16,1n,true);v.setBigUint64(24,1n,true);v.setBigUint64(32,1n,true);
  v.setBigUint64(40,BigInt(commands.length),true);
  const packet=w().bytes(header).bytes(body.finish()).finish();
  new PacketStream().decode(packet);
  const presents=[],host=await createGraphicsHost(canvas,{onDiagnostic:value=>{if(value.type==='present')presents.push(value);}});
  const queue=host.device.queue,submit=queue.submit;
  let submissions=0;
  try {
    queue.submit=function(buffers){submissions++;return submit.call(queue,buffers);};
    const result=await host.consume(packet.buffer,{proof_id:'graphics-batch-validation'});
    require(result.readbacks.length===2,'two copied GPU readbacks after source destruction');
    for(const [index,expected] of [[0,[0,0,255,255]],[1,[0,255,0,255]]]) {
      const bytes=result.readbacks[index].bytes;
      require(bytes.length===4096&&bytes.every((value,at)=>value===expected[at%4]),
        index===0?'all red pixels copied before the upload':'all green pixels copied before source destruction');
    }
    require(presents.length===2&&presents[0].presents===1&&presents[1].presents===2,
      'each Present completes separately');
    require(result.presentCommands.length===2&&result.presentCommands.every(value=>value.chain==='1'),
      'two Present commands retain their chain identity');
    require(submissions===6,`batched submission target: expected6, received${submissions}`);
    const hex=bytes=>Array.from(bytes,value=>value.toString(16).padStart(2,'0')).join('');
    return {ok:true,commands:commands.length,draws:16,queueSubmissions:submissions,
      packetSha256:hex(new Uint8Array(await crypto.subtle.digest('SHA-256',packet))),
      readbacks:result.readbacks.map(value=>({receiptHex:hex(value.receipt),pixelsHex:hex(value.bytes)})),
      presentCommands:result.presentCommands,presents};
  } finally {
    queue.submit=submit;
    host.close();
  }
}
