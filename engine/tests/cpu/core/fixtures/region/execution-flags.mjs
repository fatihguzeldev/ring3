import assert from 'node:assert/strict';

const arithmetic=0x8d5;
const little=value=>Array.from({length:4},(_,i)=>(value>>>i*8)&255);
const hex=bytes=>Buffer.from(bytes).toString('hex');

// Unbounded signed arithmetic determines overflow; unsigned arithmetic determines
// carry/borrow. This oracle does not use the emitter's XOR/sign-bit equations.
function calculate(kind,bits,left,right,initial) {
  const modulus=1n<<BigInt(bits),mask=modulus-1n,sign=modulus>>1n;
  const a=BigInt(left)&mask,b=BigInt(right)&mask,carry=BigInt(initial&1);
  const signed=value=>value>=sign?value-modulus:value;
  let exact,result,cf=false,af=false,logical=false;
  if(kind==='add'||kind==='adc'||kind==='inc'){
    const rhs=kind==='inc'?1n:b,input=kind==='adc'?carry:0n;
    exact=signed(a)+signed(rhs)+input;result=(a+rhs+input)&mask;
    cf=kind==='inc'?Boolean(initial&1):a+rhs+input>=modulus;
    af=(a&15n)+(rhs&15n)+input>=16n;
  }else if(kind==='sub'||kind==='cmp'||kind==='sbb'||kind==='dec'||kind==='neg'){
    const lhs=kind==='neg'?0n:a,rhs=kind==='neg'?a:kind==='dec'?1n:b,input=kind==='sbb'?carry:0n;
    exact=signed(lhs)-signed(rhs)-input;result=(lhs-rhs-input)&mask;
    cf=kind==='dec'?Boolean(initial&1):lhs<rhs+input;af=(lhs&15n)<(rhs&15n)+input;
  }else{
    logical=true;
    result=kind==='and'||kind==='test'?a&b:kind==='or'?a|b:a^b;
  }
  let low=Number(result&255n),ones=0;for(let i=0;i<8;i++){ones+=low&1;low>>>=1;}
  const of=!logical&&(exact < -sign||exact>=sign);
  // Ring3's accepted logical FLAGS policy clears undefined AF along with CF/OF.
  const flags=((initial&~arithmetic)|2|Number(cf)|(ones%2===0?4:0)|(!logical&&af?16:0)|
    (result===0n?64:0)|(result>=sign?128:0)|(of?2048:0))>>>0;
  const writes=!['cmp','test'].includes(kind),value=writes?Number((BigInt(left)&~mask)|result)>>>0:left>>>0;
  return{flags,value};
}

function conditions(flags) {
  const c=Boolean(flags&1),p=Boolean(flags&4),z=Boolean(flags&64),s=Boolean(flags&128),o=Boolean(flags&2048);
  return[o,!o,c,!c,z,!z,c||z,!c&&!z,s,!s,p,!p,s!==o,s===o,z||s!==o,!z&&s===o];
}

function opcode(kind,bits) {
  if(kind==='inc'||kind==='dec')return bits===8?[0xfe,kind==='inc'?0xc0:0xc8]:
    [...(bits===16?[0x66]:[]),kind==='inc'?0x40:0x48];
  if(kind==='neg')return[...(bits===16?[0x66]:[]),bits===8?0xf6:0xf7,0xd8];
  const pair={add:[0x00,0x01],sub:[0x28,0x29],cmp:[0x38,0x39],and:[0x20,0x21],or:[0x08,0x09],
    xor:[0x30,0x31],test:[0x84,0x85],adc:[0x10,0x11],sbb:[0x18,0x19]}[kind];
  assert(pair);return[...(bits===16?[0x66]:[]),pair[bits===8?0:1],0xc8];
}

function vectors(bits) {
  const sign=2**(bits-1),mask=2**bits-1;
  return[
    ['add-signed-overflow','add',sign-1,1,0],['add-carry','add',mask,1,0],
    ['sub-borrow','sub',0,1,0],['cmp-signed-overflow','cmp',sign,1,0],
    ['and-zero','and',mask,0,0],['or-sign','or',0,sign,0],['xor-zero','xor',mask,mask,0],
    ['test-sign','test',sign,sign,0],['adc-overflow','adc',sign-1,0,1],['adc-carry','adc',mask,0,1],
    ['sbb-overflow','sbb',sign,0,1],['sbb-borrow','sbb',0,0,1],['neg-minimum','neg',sign,0,0],
    ['inc-carry-preserved','inc',sign-1,0,1],['dec-carry-preserved','dec',sign,0,1],
  ];
}

export function runFlagsOracleCases(fixture) {
  assert.equal(calculate('cmp',32,0x80000000,1,2).flags,0x816);
  assert.equal(calculate('cmp',32,1,0x80000000,2).flags,0x883);
  assert.equal(calculate('add',32,0x7fffffff,1,2).flags,0x896);
  assert.equal(calculate('inc',8,0,0,3).flags,3);
  for(const bits of[8,16,32])for(const[name,kind,lowLeft,lowRight,carry]of vectors(bits)){
    const left=bits===32?lowLeft:(0x6a5a0000|lowLeft)>>>0;
    const right=bits===32?lowRight:(0x13570000|lowRight)>>>0;
    const initialFlags=0x402|carry,wanted=calculate(kind,bits,left,right,initialFlags),truth=conditions(wanted.flags);
    const write=opcode(kind,bits),code=[...write];
    for(let cc=0;cc<16;cc++)code.push(0x0f,0x90+cc,0x43,0x20+cc);
    for(let cc=0;cc<16;cc++){
      // Every Jcc gets the same writer immediately before it, not an unknown
      // incoming recipe after a previous conditional block. MOVs preserve FLAGS.
      code.push(carry?0xf9:0xf8,0xb8,...little(left),0xb9,...little(right),...write,
        0x0f,0x80+cc,6,0,0,0,0xc6,0x43,0x40+cc,0,0xeb,4,0xc6,0x43,0x40+cc,1);
    }
    code.push(0xff,0xe7);assert(code.length<4096,'the complete independent FLAGS matrix stays in one code page');
    fixture(`execution-all-setcc-jcc-${bits}-${name}`,{
      segments:[[0x1000,hex(code)]],registers:[left,right,0,0x3000,0x4800,0,0,0x2000],
      initialFlags,compileInstructions:256,compileBlocks:128,
    },t=>{
      const markers=hex(truth.map(Number));t.writeExpected(0x3020,markers);t.writeExpected(0x3040,markers);
      const before=t.version(),run=t.check({pc:0x2000,retired:122,budget:122,flags:wanted.flags,regs:{0:wanted.value},
        fp:t.startingFp});
      assert.equal(run.activations.length,1);assert.equal(t.result.modules.length,1);assert.equal(run.helperCalls,0);
      assert.equal(t.version(),before+32n,'all32 condition markers commit once each');
      t.result.flagOracle={kind,bits,left,right,initialFlags,...wanted,conditions:truth.map(Number)};
      t.result.semanticComplete=true;
    });
  }
}
