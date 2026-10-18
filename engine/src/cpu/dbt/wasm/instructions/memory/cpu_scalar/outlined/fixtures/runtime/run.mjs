import assert from 'node:assert/strict';

let input = '';
for await (const chunk of process.stdin) input += chunk;
const fixture = JSON.parse(input);
const bytes = Buffer.from(fixture.module, 'hex');
assert.ok(WebAssembly.validate(bytes));
const memory = new WebAssembly.Memory({initial:1,maximum:1});
const view = new DataView(memory.buffer);
const {exports} = new WebAssembly.Instance(new WebAssembly.Module(bytes), {env:{memory}});
const packet = [0x484d3352,0x1000a,40,0,0,0xfedcba98,0,0,0,4];
const initial = [0,0,0x1000,0xfffffffe,0,0x91,0xa2,0xb3,0xc4,0xd5,0xe6,0xf7];
let controls = 0;

function check(name, {words=packet,args=initial,mode=1,base=0,descriptor=0x020fffff,expected,completed,slow=true,precheck=false}={}) {
  new Uint8Array(memory.buffer).fill(0x5a);
  words.forEach((value,index)=>view.setUint32(100+index*4,value,true));
  view.setUint16(fixture.segments+4,mode,true);
  view.setUint32(fixture.segments+32,descriptor,true);
  view.setUint32(fixture.bases+20,base,true);
  const before=new Uint8Array(memory.buffer).slice();
  const inline=exports[precheck?'inline_context':'inline'](...args).map(x=>x>>>0);
  assert.deepEqual(new Uint8Array(memory.buffer),before,`${name}: inline leaves RAM/CPU/FP intact`);
  const calls=exports.slowCalls.value;
  const outlined=exports[precheck?'outlined_context':'outlined'](...args).map(x=>x>>>0);
  assert.deepEqual(outlined,inline,`${name}: exact terminal, scratch, RESULT and branch`);
  assert.equal(exports.slowCalls.value-calls,Number(slow),`${name}: exact-success selection`);
  assert.deepEqual(new Uint8Array(memory.buffer),before,`${name}: outlined leaves RAM/CPU/FP intact`);
  if(expected)assert.deepEqual(outlined.slice(0,5),expected,`${name}: literal terminal`);
  if(completed!==undefined)assert.equal(outlined[8],completed,`${name}: caller continuation`);
  assert.equal(outlined[7],initial[11],`${name}: RESULT is never validator-owned`);
  controls++;
}

for(const payload of [0,1,0x7fffffff,0x80000000,0xffffffff])
  check(`success/${payload}`,{words:packet.with(5,payload),expected:[0,0x91,0xa2,0xb3,0xc4],completed:1,slow:false});
check('prior terminal reason preserved',{args:initial.with(4,13),expected:[13,0x91,0xa2,0xb3,0xc4],completed:0,slow:false});
for(const status of [1,2,3,4,5,7,9,11,12,0xffffffff])
  check(`transport/${status}`,{args:initial.with(1,status),expected:[7,3,0xa2,0xb3,0xc4],completed:0});
for(const [field,value]of [[0,0],[1,0x10007],[1,0x10001],[2,39],[3,1],[4,3],[4,0xffffffff],[6,1],[7,1],[8,1],[9,0],[9,1],[9,2],[9,8]])
  check(`malformed/${field}/${value}`,{words:packet.with(field,value),expected:[7,2,0xa2,0xb3,0xc4],completed:0});
for(const detail of [1,2])for(const address of [0xfffffffe,0xffffffff,0,1])
  check(`wrapped legacy fault/${detail}/${address}`,{words:packet.with(4,1).with(5,0).with(6,detail).with(7,address).with(8,1),expected:[5,detail,address,1,4],completed:0});
for(const [field,value]of [[5,1],[6,0],[6,3],[7,2],[7,0xfffffffd],[8,0],[8,2],[9,1]]) {
  const fault=packet.with(4,1).with(5,0).with(6,1).with(7,0).with(8,1);
  check(`malformed legacy fault/${field}/${value}`,{words:fault.with(field,value),expected:[7,2,0xa2,0xb3,0xc4],completed:0});
}
for(const detail of [0,1,2,3])
  check(`rejected/${detail}`,{words:packet.with(4,2).with(5,0).with(6,detail),expected:[7,detail===1?1:detail===2?3:2,0xa2,0xb3,0xc4],completed:0});
const typed=[0x484d3352,0x1000f,40,0,9,0,4,0,1,4];
for(const detail of [4,5])
  check(`typed PF/${detail}`,{words:typed.with(6,detail),args:initial.with(1,23),expected:[23,detail,0,1,4],completed:0});
for(let field=0;field<typed.length;field++)
  check(`malformed typed/${field}`,{words:typed.with(field,typed[field]^0x55),args:initial.with(1,23),expected:[7,2,0xa2,0xb3,0xc4],completed:0});
check('typed packet requires transport23',{words:typed,expected:[7,2,0xa2,0xb3,0xc4],completed:0});
check('transport23 requires typed packet',{args:initial.with(1,23),expected:[7,2,0xa2,0xb3,0xc4],completed:0});
check('read32 rejects SMC success',{args:initial.with(1,11),expected:[7,3,0xa2,0xb3,0xc4],completed:0});

// the real read caller checks live context before it interprets any returned packet.
for(const [base,descriptor,reason]of [[0,0x020fffff,0],[0x8000,0x020fffff,3],[0x8000,0x020000ff,11]]) {
  check(`read precheck/${base}/${descriptor}`,{mode:3,base,descriptor,precheck:true,slow:false,expected:reason?[reason,0,0,0,0]:[0,0x91,0xa2,0xb3,0xc4],completed:reason?0:1});
  if(reason)check(`context before malformed packet/${reason}`,{words:packet.with(0,0),mode:3,base,descriptor,precheck:true,slow:false,expected:[reason,0,0,0,0],completed:0});
}
check('rejected transport still checks live context',{args:initial.with(1,7),mode:3,base:0x8000,expected:[3,0,0,0,0],completed:0});
check('SMC retains rejected-context exemption',{args:initial.with(1,11),mode:3,base:0x8000,expected:[7,3,0xa2,0xb3,0xc4],completed:0});
assert.ok(controls>=60);
console.log('read32 inline/outlined controls passed');
