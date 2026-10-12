const hex=text=>Buffer.from(text,'hex'),zero=Buffer.alloc(16);
const qwords=values=>{const out=Buffer.alloc(16);values.forEach((value,index)=>out.writeBigUInt64LE(value,index*8));return out;};
const instruction=(map,opcode,modrm=0xc1,immediate)=>Buffer.from([0x66,0x0f,map,opcode,modrm,...(immediate===undefined?[]:[immediate])]).toString('hex');
const roundCases=[
 ['aesenc',0x38,0xdc,undefined,'00102030405060708090a0b0c0d0e0f0','d6aa74fdd2af72fadaa678f1d6ab76fe','89d810e8855ace682d1843d8cb128fe4'],
 ['aesenclast',0x38,0xdd,undefined,'bd6e7c3df2b5779e0b61216e8b10b689','13111d7fe3944a17f307a78b4d2b30c5','69c4e0d86a7b0430d8cdb78070b4c55a'],
 ['aesdec',0x38,0xde,undefined,'7ad5fda789ef4e272bca100b3d9ff59f','13aa29be9c8faff6f770f58000f7bf03','54d990a16ba09ab596bbf40ea111702f'],
 ['aesdeclast',0x38,0xdf,undefined,'6353e08c0960e104cd70b751bacad0e7','000102030405060708090a0b0c0d0e0f','00112233445566778899aabbccddeeff'],
 ['aesimc',0x38,0xdb,undefined,'a5'.repeat(16),'549932d1f08557681093ed9cbe2c974e','13aa29be9c8faff6f770f58000f7bf03'],
 ['aeskeygenassist',0x3a,0xdf,1,'a5'.repeat(16),'000102030405060708090a0b0c0d0e0f','f26b6fc56a6fc5f2fed7ab76d6ab76fe'],
];
export const cryptoCases=[];
for(const[name,map,opcode,immediate,a,b,out]of roundCases){
 cryptoCases.push({name:`crypto-${name}-literal-intermediate`,code:instruction(map,opcode,0xc1,immediate),a:hex(a),b:hex(b),out:hex(out),mxcsr:0x003f,expectedHelpers:1,expectedExecutionHelpers:1});
 cryptoCases.push({name:`crypto-${name}-aligned-full16-read`,code:instruction(map,opcode,0x00,immediate),a:hex(a),memory:hex(b),out:hex(out),address:0x5010,readAlignment:16,mxcsr:0xdfe5,expectedHelpers:2,expectedExecutionHelpers:1});
}
for(const[name,map,opcode,immediate]of roundCases){
 const value=name==='aesenc'||name==='aesenclast'?0x63:name==='aesdec'||name==='aesdeclast'?0x52:0;
 const out=Buffer.alloc(16,value);
 if(name==='aeskeygenassist'){out.fill(0x63);out[4]^=1;out[12]^=1;}
 cryptoCases.push({name:`crypto-${name}-self-alias-snapshots-both-inputs`,code:instruction(map,opcode,0xc0,immediate),a:zero,out,mxcsr:0xffff,expectedHelpers:1,expectedExecutionHelpers:1});
}
for(const immediate of[0,0x80,0xff]){
 const out=hex('f26b6fc56b6fc5f2fed7ab76d7ab76fe');out[4]^=immediate;out[12]^=immediate;
 cryptoCases.push({name:`crypto-key-assist-round-constant-${immediate}`,code:instruction(0x3a,0xdf,0xc1,immediate),a:Buffer.alloc(16,0x7f),b:hex('ffffffff04050607808080800c0d0e0f'),out,mxcsr:0x003f,expectedHelpers:1,expectedExecutionHelpers:1});
}
const left=qwords([1n<<63n,3n]),right=qwords([2n,0x8000000000000001n]);
const selectors=[[0,[0n,1n]],[1,[6n,0n]],[0x10,[0x8000000000000000n,0x4000000000000000n]],[0x11,[0x8000000000000003n,1n]]];
for(const[control,product]of selectors)for(const ignored of[0,0xee]){
 cryptoCases.push({name:`crypto-pclmul-halves-${control}-ignored-${ignored}`,code:instruction(0x3a,0x44,0xc1,control|ignored),direct:true,a:left,b:right,out:qwords(product),mxcsr:0x003f,expectedHelpers:0});
}
for(const[leftBit,rightBit]of[[0,63],[1,63],[31,32],[32,32],[32,63],[63,0],[63,1],[63,63]]){
 const product=1n<<BigInt(leftBit+rightBit);
 cryptoCases.push({name:`crypto-pclmul-basis-${leftBit}-${rightBit}`,code:instruction(0x3a,0x44,0xc1,0),direct:true,a:qwords([1n<<BigInt(leftBit),0n]),b:qwords([1n<<BigInt(rightBit),0n]),out:qwords([product&0xffffffffffffffffn,product>>64n]),expectedHelpers:0});
}
cryptoCases.push(
 {name:'crypto-pclmul-all-ones-self-alias',code:instruction(0x3a,0x44,0xc0,0x11),direct:true,a:Buffer.alloc(16,255),out:qwords([0x5555555555555555n,0x5555555555555555n]),expectedHelpers:0},
 {name:'crypto-pclmul-aligned-memory-selected-high-half',code:instruction(0x3a,0x44,0x00,0xff),direct:true,a:left,memory:right,out:qwords([0x8000000000000003n,1n]),address:0x5010,readAlignment:16,expectedHelpers:1},
);
for(const[name,map,opcode,immediate]of[...roundCases,['pclmul',0x3a,0x44,0]]){
 for(const[stage,address,reason,fault]of[['alignment-before-unmapped',0x6001,11,undefined],['unmapped-full16',0x6000,5,0x6000]]){
  cryptoCases.push({name:`crypto-${name}-${stage}-preserves-entire-state-and-transfer`,code:instruction(map,opcode,0x00,immediate),direct:name==='pclmul',a:Buffer.alloc(16,0x19),b:Buffer.alloc(16,0x35),address,reason,retired:0,detail:reason===5?1:0,...(reason===5?{fault,access:1,length:16}:{}),readAlignment:16,stagedTransfer:Buffer.alloc(16,0x97),expectedHelpers:1,expectedExecutionHelpers:0});
 }
}
