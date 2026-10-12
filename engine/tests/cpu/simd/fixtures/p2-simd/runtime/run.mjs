import assert from 'node:assert/strict';
import {readFileSync,writeFileSync} from 'node:fs';
import {join} from 'node:path';
import {createHash} from 'node:crypto';
import {readEngine} from '../../../../../support/wasm/engine.mjs';
import {packedStringCases} from './packed-strings.mjs';
import {cryptoCases} from './crypto.mjs';
const [enginePath,output,root]=process.argv.slice(2),engine=readEngine(enginePath),initial=readFileSync(join(output,'initial-arena.bin'));
const SIZE=initial.length,SV=4364,FP=4236,SG=4524,T=140,DATA=0x5010;
const hash=b=>createHash('sha256').update(b).digest('hex'),hex=s=>Buffer.from(s,'hex');
const dwords=v=>{const b=Buffer.alloc(v.length*4);v.forEach((n,i)=>b.writeUInt32LE(n>>>0,i*4));return b;};
const words=v=>{const b=Buffer.alloc(v.length*2);v.forEach((n,i)=>b.writeUInt16LE(n&0xffff,i*2));return b;};
const qwords=v=>{const b=Buffer.alloc(v.length*8);v.forEach((n,i)=>b.writeBigUInt64LE(n,i*8));return b;};
const fill=n=>Buffer.alloc(16,n),zero=fill(0),parents=()=>[0x10203040,0x21314151,0x32425262,0x43536373,0x7080,0x65758595,0x768696a6,0x8797a7b7];
const record=(magic,version,size,values)=>{const b=Buffer.alloc(size);b.write(magic);b.writeUInt16LE(version,4);b.writeUInt16LE(1,6);b.writeUInt32LE(size,8);values.forEach((n,i)=>b.writeUInt32LE(n>>>0,16+i*4));return b;};
const state=(r,pc,flags)=>record('R3ST',1,56,[...r,pc,flags]);
const exit=(reason,retired,detail=0,address=0,access=0,length=0)=>record('R3EX',6,40,[reason,retired,detail,address,access,length]);
const packet=(length,status=0,detail=0,address=0,access=0)=>record('R3MH',7,40,[status,0,detail,address,access,length]);
const savedImage=Buffer.alloc(512);savedImage.writeUInt16LE(0x37f);savedImage.writeUInt32LE(0x1f80,24);savedImage.writeUInt32LE(0xffff,28);savedImage.fill(0x31,160,176);savedImage.fill(0x42,176,192);savedImage.fill(0x66,464);
const restoredImage=Buffer.alloc(512);restoredImage.writeUInt16LE(0x37f);restoredImage.writeUInt16LE(0x1800,2);restoredImage[4]=0x18;restoredImage.writeUInt32LE(0x5f80,24);restoredImage.writeUInt32LE(0xffff,28);
const restoredX87=record('R3FP',1,128,[]);restoredX87.writeUInt16LE(0x37f,16);restoredX87.writeUInt16LE(0x1800,18);restoredX87.writeUInt16LE(0xfd3f,20);
for(let physical=0;physical<8;physical++){const raw=Buffer.alloc(10);if(physical!==4){raw.writeBigUInt64LE(0x8000000000000000n+BigInt(physical));raw.writeUInt16LE(0x3fff,8);}raw.copy(restoredX87,40+physical*10);raw.copy(restoredImage,32+((physical+5)&7)*16);restoredImage.fill(0x30+physical,160+physical*16,176+physical*16);}
const cases=[
 {name:'movlps-store-raw-bits-and-canaries',code:'0f1300',a:hex('341200000000f07f1122334455667788'),write:hex('341200000000f07f6666666666666666'),writeLength:8},
 {name:'paddb-wrap',code:'660ffcc1',direct:true,a:fill(255),b:fill(1),out:zero},
 {name:'paddsb-saturate',code:'660fecc1',direct:true,a:fill(127),b:fill(1),out:fill(127)},
 {name:'pmaddwd-overflow',code:'660ff5c1',direct:true,a:hex('00800080008000800080008000800080'),b:hex('00800080008000800080008000800080'),out:dwords([0x80000000,0x80000000,0x80000000,0x80000000])},
 {name:'pshufd-self-alias',code:'660f70c01b',direct:true,a:dwords([1,2,3,4]),out:dwords([4,3,2,1])},
 {name:'pandn-operand-order',code:'660fdfc1',direct:true,a:fill(0xf0),b:fill(0x33),out:fill(3)},
 {name:'punpckhbw-source-alias',code:'660f68c0',direct:true,a:Buffer.from(Array.from({length:16},(_,i)=>i)),out:hex('080809090a0a0b0b0c0c0d0d0e0e0f0f')},
 {name:'pmovmskb-high-bit-order',code:'660fd7c0',direct:true,a:hex('80008000800080008000800080008000'),gpr:0x5555},
 {name:'pextrw-zero-extends',code:'660fc5c003',direct:true,a:hex('010203040506ffff090a0b0c0d0e0f10'),gpr:0xffff},
 {name:'psrlq-full64-count',code:'660fd3c1',direct:true,a:fill(255),b:qwords([0x100000000n,0n]),out:zero},
 {name:'addss-nearest-even',code:'f30f58c1',a:dwords([0x3f800000,1,2,3]),b:dwords([0x33800000,4,5,6]),out:dwords([0x3f800000,1,2,3]),mxcsr:0x1f80,afterMxcsr:0x1fa0},
 {name:'addss-round-up',code:'f30f58c1',a:dwords([0x3f800000,1,2,3]),b:dwords([0x33800000,4,5,6]),out:dwords([0x3f800001,1,2,3]),mxcsr:0x5f80,afterMxcsr:0x5fa0},
 {name:'addpd-two-exact-lanes',code:'660f58c1',a:qwords([0x3ff0000000000000n,0x4000000000000000n]),b:qwords([0x4000000000000000n,0x4008000000000000n]),out:qwords([0x4008000000000000n,0x4014000000000000n])},
 {name:'sqrtss-integer-radical',code:'f30f51c1',a:dwords([0,1,2,3]),b:dwords([0x40000000,0,0,0]),out:dwords([0x3fb504f3,1,2,3]),afterMxcsr:0x1fa0},
 {name:'snan-source1-priority',code:'f30f58c1',a:dwords([0x7f801234,1,2,3]),b:dwords([0x7fc05678,4,5,6]),out:dwords([0x7fc01234,1,2,3]),afterMxcsr:0x1f81},
 {name:'divide-unmasked-stops',code:'f30f5ec1',a:dwords([0x3f800000,1,2,3]),b:zero,mxcsr:0x1d80,afterMxcsr:0x1d84,reason:13,retired:0,detail:0x1d84},
 {name:'overflow-unmasked-suppresses-own-precision',code:'f30f59c1',a:dwords([0x7f7fffff,1,2,3]),b:dwords([0x40000000,0,0,0]),mxcsr:0x1b80,afterMxcsr:0x1b88,reason:13,retired:0,detail:0x1b88},
 {name:'exact-tiny-unmasked-underflow',code:'f30f59c1',a:dwords([0x00800000,1,2,3]),b:dwords([0x3f000000,0,0,0]),mxcsr:0x1780,afterMxcsr:0x1790,reason:13,retired:0,detail:0x1790},
 {name:'ftz-ignored-when-underflow-unmasked',code:'f30f59c1',a:dwords([0x00800000,1,2,3]),b:dwords([0x3f000000,0,0,0]),mxcsr:0x9780,afterMxcsr:0x9790,reason:13,retired:0,detail:0x9790},
 {name:'tiny-before-rounding-to-normal-unmasked',code:'f30f59c1',a:dwords([0x00800000,1,2,3]),b:dwords([0x3f7fffff,0,0,0]),mxcsr:0x1780,afterMxcsr:0x1790,reason:13,retired:0,detail:0x1790},
 {name:'tiny-before-rounding-to-normal-masked',code:'f30f59c1',a:dwords([0x00800000,1,2,3]),b:dwords([0x3f7fffff,0,0,0]),out:dwords([0x00800000,1,2,3]),afterMxcsr:0x1fb0},
 {name:'tiny-before-rounding-to-normal-ftz',code:'f30f59c1',a:dwords([0x00800000,1,2,3]),b:dwords([0x3f7fffff,0,0,0]),out:dwords([0,1,2,3]),mxcsr:0x9f80,afterMxcsr:0x9fb0},
 {name:'double-tiny-before-rounding-to-normal',code:'f20f59c1',a:qwords([0x0010000000000000n,0x123456789abcdef0n]),b:qwords([0x3fefffffffffffffn,0n]),mxcsr:0x1780,afterMxcsr:0x1790,reason:13,retired:0,detail:0x1790},
 {name:'cvtsd2ss-tiny-before-rounding-to-normal',code:'f20f5ac1',a:dwords([1,2,3,4]),b:qwords([0x380fffffe0000000n,0n]),mxcsr:0x1780,afterMxcsr:0x1790,reason:13,retired:0,detail:0x1790},
 {name:'packed-overflow-retains-other-lane-precision',code:'0f59c1',a:dwords([0x7f7fffff,0x3f800001,0,0]),b:dwords([0x40000000,0x3f800001,0,0]),mxcsr:0x1b80,afterMxcsr:0x1ba8,reason:13,retired:0,detail:0x1ba8},
 {name:'packed-invalid-retains-other-lane-denormal',code:'0f59c1',a:dwords([0x7f800000,1,0,0]),b:dwords([0,0x3f800000,0,0]),mxcsr:0x1f00,afterMxcsr:0x1f03,reason:13,retired:0,detail:0x1f03},
 {name:'minss-qnan-masked-invalid-source2',code:'f30f5dc1',a:dwords([0x7fc01234,1,2,3]),b:zero,out:dwords([0,1,2,3]),afterMxcsr:0x1f81},
 {name:'minss-qnan-unmasked-invalid',code:'f30f5dc1',a:dwords([0x7fc01234,1,2,3]),b:zero,mxcsr:0x1f00,afterMxcsr:0x1f01,reason:13,retired:0,detail:0x1f01},
 {name:'ucomiss-unordered-flags',code:'0f2ec1',a:dwords([0x7fc01234,1,2,3]),b:zero,flags:0x447},
 {name:'cvttps2dq-indefinite',code:'f30f5bc1',a:zero,b:dwords([0x7fc01234,0x3fc00000,0xbfc00000,0x4f000000]),out:dwords([0x80000000,1,0xffffffff,0x80000000]),afterMxcsr:0x1fa1},
 {name:'movdqu-unaligned-read',code:'f30f6f00',direct:true,a:zero,memory:fill(0x35),address:DATA+1,out:fill(0x35)},
 {name:'movaps-alignment-before-unmapped',code:'0f2800',direct:true,a:fill(0x19),address:0x6001,reason:11,retired:0,detail:0},
 {name:'movdqu-read-unmapped',code:'f30f6f00',direct:true,a:fill(0x19),address:0x6000,reason:5,retired:0,detail:1,fault:0x6000,access:1,length:16},
 {name:'movdqu-unaligned-write',code:'f30f7f00',direct:true,a:fill(0x5a),address:DATA+1,write:fill(0x5a)},
 {name:'movntq-unaligned-writable-store',code:'0fe700',direct:true,mmx:true,a:hex('0123456789abcdef'),address:DATA+1,write:hex('0123456789abcdef'),writeLength:8},
 {name:'movntps-still-requires-alignment',code:'0f2b00',direct:true,a:fill(0x5a),address:DATA+1,reason:11,retired:0,unchangedMemory:fill(0x66)},
 {name:'movntdq-still-requires-alignment',code:'660fe700',direct:true,a:fill(0x5a),address:DATA+1,reason:11,retired:0,unchangedMemory:fill(0x66)},
 {name:'maskmovdqu-sparse',code:'660ff7c1',a:Buffer.from(Array.from({length:16},(_,i)=>i+1)),b:hex('80008000000000800000000000000080'),masked:0x8085,address:DATA},
 {name:'maskmovdqu-zero-null-segment-no-access',code:'660ff7c1',a:fill(0x51),b:zero,masked:0,address:0xffffffff,nullDs:true},
 {name:'maskmovdqu-selected-null-segment-gp',code:'660ff7c1',a:fill(0x51),b:fill(0x80),address:DATA,nullDs:true,reason:11,retired:0,compiledGp:true},
 {name:'mmx-paddb-wrap',code:'0ffcc1',direct:true,mmx:true,a:fill(255),b:fill(1),out:zero},
 {name:'pending-x87-stops-mmx-before-helper',code:'0ffcc1',direct:true,mmx:true,pendingX87:true,a:fill(255),b:fill(1),reason:12,retired:0,detail:0x8081},
 {name:'sse-independent-of-pending-x87',code:'660ffcc1',direct:true,pendingX87:true,a:fill(255),b:fill(1),out:zero},
 {name:'emms-preserves-raw80',code:'0f77',empty:true},
 {name:'ldmxcsr-reserved-gp',code:'0fae10',address:DATA,memory:dwords([0x10000,0,0,0]),reason:11,retired:0},
 {name:'stmxcsr-exact-dword',code:'0fae18',address:DATA,mxcsr:0x5f80,write:dwords([0x5f80]),writeLength:4},
 {name:'fxsave-write-only-preserves-software-tail',code:'0fae00',address:DATA,a:fill(0x31),b:fill(0x42),seedMemory:Buffer.alloc(512,0x66),write:savedImage,writeLength:512,writeOnly:true},
 {name:'fxrstor-physical-tags-and-xmm',code:'0fae08',address:DATA,memory:restoredImage,afterX87:restoredX87,afterXmm:restoredImage.subarray(160,288),afterMxcsr:0x5f80},
 {name:'mmx-psrlq-variable-count-nonzero',code:'0fd3c1',direct:true,mmx:true,a:hex('1122334455667788'),b:qwords([8n,0n]),out:hex('2233445566778800')},
 {name:'addsd-exact-even-carry-preserves-controls-and-upper',code:'f20f58c1',a:qwords([0x3ff0000000000001n,0x0123456789abcdefn]),b:qwords([0x3ff0000000000001n,0x7ff0000000000001n]),out:qwords([0x4000000000000001n,0x0123456789abcdefn]),mxcsr:0xdfe5},
 {name:'subss-exact-negative-preserves-unmasked-controls-and-upper',code:'f30f5cc1',a:dwords([0x3f800000,0x7fc01234,0x87654321,0x12345678]),b:dwords([0x3fc00000,0x7f801234,0x11223344,0x55667788]),out:dwords([0xbf000000,0x7fc01234,0x87654321,0x12345678]),mxcsr:0x8040},
 {name:'fxrstor-split-read-fault-preserves-staged-transfer',code:'0fae08',address:0x5f00,a:fill(0x31),b:fill(0x42),reason:5,retired:0,detail:1,fault:0x6000,access:1,length:512,stagedTransfer:Buffer.from(Array.from({length:512},(_,i)=>(i*29+0x63)%255+1))},
 {name:'psllw-full-immediate-ff-zeroes',code:'660f71f0ff',direct:true,a:fill(0xa5),out:zero},
 {name:'pslld-count-equal-width-zeroes',code:'660ff2c1',direct:true,a:dwords([1,0x80000000,0xffffffff,0x12345678]),b:qwords([32n,0n]),out:zero},
 {name:'psllq-count-63-keeps-last-bit',code:'660ff3c1',direct:true,a:qwords([1n,3n]),b:qwords([63n,0n]),out:qwords([0x8000000000000000n,0x8000000000000000n])},
 {name:'psrlw-unsigned-count-sign-bit-zeroes',code:'660fd1c1',direct:true,a:fill(255),b:qwords([0x8000000000000000n,0n]),out:zero},
 {name:'psrld-immediate-ordinary-logical',code:'660f72d004',direct:true,a:dwords([0xf1234567,0x80000000,1,0xffffffff]),out:dwords([0x0f123456,0x08000000,0,0x0fffffff])},
 {name:'psraw-oversized-count-clamps-per-lane-sign',code:'660fe1c1',direct:true,a:hex('0180ff7fffff010000800000feff0200'),b:qwords([33n,0n]),out:hex('ffff0000ffff0000ffff0000ffff0000')},
 {name:'psrad-full-immediate-ff-clamps-per-lane-sign',code:'660f72e0ff',direct:true,a:dwords([0x80000001,0x7fffffff,0xffffffff,1]),out:dwords([0xffffffff,0,0xffffffff,0])},
 {name:'pslld-count-destination-alias-ignores-upper64',code:'660ff2c0',direct:true,a:qwords([1n,0x80000000ffffffffn]),out:dwords([2,0,0xfffffffe,0])},
 {name:'psraw-memory-count-ignores-upper64',code:'660fe100',direct:true,a:hex('0180ff7fffff010000800000feff0200'),memory:qwords([1n,0xffffffffffffffffn]),address:DATA,readAlignment:16,out:hex('00c0ff3fffff000000c00000ffff0100')},
 {name:'psllq-memory-count-unmapped-preserves-destination',code:'660ff300',direct:true,a:qwords([0x1122334455667788n,0x8877665544332211n]),address:0x6000,reason:5,retired:0,detail:1,fault:0x6000,access:1,length:16,readAlignment:16,stagedTransfer:fill(0x97)},
 {name:'mmx-psrlw-memory-count-reads-full8-before-oversized-shortcut',code:'0fd100',direct:true,mmx:true,a:hex('1122334455667788'),address:0x5ffc,seedMemory:dwords([64]),reason:5,retired:0,detail:1,fault:0x6000,access:1,length:8,readAlignment:1,stagedTransfer:Buffer.alloc(8,0xb3)},
 {name:'pending-x87-stops-mmx-shift-before-count-read',code:'0ff300',direct:true,mmx:true,pendingX87:true,a:hex('1122334455667788'),address:0x5ffc,seedMemory:dwords([64]),reason:12,retired:0,detail:0x8081},
 {name:'packsswb-xmm-signed-endpoints-and-source-order',code:'660f63c1',direct:true,a:hex('00807fff80ffffff000001007f008000'),b:hex('ff7fff000001018081ff7e00feff0200'),out:hex('808080ff00017f7f7f7f7f80817efe02'),mxcsr:0xdfe5},
 {name:'packsswb-mmx-signed-endpoints-and-source-order',code:'0f63c1',direct:true,mmx:true,a:hex('00807fff80ff7f00'),b:hex('8000ffff0000ff7f'),out:hex('8080807f7fff007f')},
 {name:'packssdw-xmm-signed-endpoints-and-source-order',code:'660f6bc1',direct:true,a:dwords([0x80000000,0xffff7fff,0xffff8000,0xffffffff]),b:dwords([0,0x7fff,0x8000,0x7fffffff]),out:hex('008000800080ffff0000ff7fff7fff7f')},
 {name:'packssdw-mmx-signed-endpoints-and-source-order',code:'0f6bc1',direct:true,mmx:true,a:dwords([0xffff7fff,0x7fff]),b:dwords([0xffff8000,0x8000]),out:hex('0080ff7f0080ff7f')},
 {name:'packuswb-xmm-signed-input-endpoints-and-source-order',code:'660f67c1',direct:true,a:hex('0080ffff00000100fe00ff000001ff7f'),b:hex('fffe00ff7fff80ff02007f0080000101'),out:hex('00000001feffffff00000000027f80ff')},
 {name:'packuswb-mmx-signed-input-endpoints-and-source-order',code:'0f67c1',direct:true,mmx:true,a:hex('ffff0000ff000001'),b:hex('00800100fe00ff7f'),out:hex('0000ffff0001feff'),mxcsr:0x8040},
 {name:'packsswb-xmm-self-alias-captures-original-lanes',code:'660f63c0',direct:true,a:hex('00807fff80ffffff000001007f008000'),out:hex('808080ff00017f7f808080ff00017f7f')},
 {name:'packssdw-mmx-self-alias-keeps-both-halves',code:'0f6bc0',direct:true,mmx:true,a:dwords([0xffff7fff,0x7fff]),out:hex('0080ff7f0080ff7f')},
 {name:'packssdw-xmm-aligned-memory-reads-full16',code:'660f6b00',direct:true,a:dwords([0x80000000,0xffff7fff,0xffff8000,0xffffffff]),memory:dwords([0,0x7fff,0x8000,0x7fffffff]),address:DATA,readAlignment:16,expectedHelpers:1,out:hex('008000800080ffff0000ff7fff7fff7f')},
 {name:'packuswb-mmx-unaligned-memory-reads-full8',code:'0f6700',direct:true,mmx:true,a:hex('ffff0000ff000001'),memory:hex('00800100fe00ff7f'),address:DATA+1,readAlignment:1,expectedHelpers:1,out:hex('0000ffff0001feff')},
 {name:'packsswb-xmm-alignment-precedes-unmapped-memory',code:'660f6300',direct:true,a:hex('00807fff80ffffff000001007f008000'),address:0x6001,reason:11,retired:0,detail:0,expectedHelpers:1},
 {name:'packssdw-xmm-unmapped-full16-preserves-destination',code:'660f6b00',direct:true,a:dwords([0x80000000,0xffff7fff,0xffff8000,0xffffffff]),address:0x6000,reason:5,retired:0,detail:1,fault:0x6000,access:1,length:16,readAlignment:16,expectedHelpers:1,stagedTransfer:fill(0x97)},
 {name:'packsswb-mmx-split-full8-preserves-destination-and-transfer',code:'0f6300',direct:true,mmx:true,a:hex('00807fff80ff7f00'),address:0x5ffc,seedMemory:dwords([0x12345678]),reason:5,retired:0,detail:1,fault:0x6000,access:1,length:8,readAlignment:1,expectedHelpers:1,stagedTransfer:Buffer.alloc(8,0xb3)},
 {name:'pending-x87-stops-mmx-pack-before-memory-read',code:'0f6700',direct:true,mmx:true,pendingX87:true,a:hex('ffff0000ff000001'),address:0x5ffc,seedMemory:dwords([0x12345678]),reason:12,retired:0,detail:0x8081,expectedHelpers:0},
 {name:'pmulhw-xmm-signed-high-words-and-lane-order',code:'660fe5c1',direct:true,a:hex('0080ffffff7f0180010000000040ffff'),b:hex('00800100ffff0200ffffffff0400ffff'),out:hex('0040ffffffffffffffff000001000000'),mxcsr:0xdfe5,expectedHelpers:0},
 {name:'pmulhuw-xmm-unsigned-high-words-and-lane-order',code:'660fe4c1',direct:true,a:hex('0080ffffff7f0180010000000040ffff'),b:hex('00800100ffff0200ffffffff0400ffff'),out:hex('00400000fe7f0100000000000100feff'),mxcsr:0x8040,expectedHelpers:0},
 {name:'pmulhw-mmx-signed-high-words',code:'0fe5c1',direct:true,mmx:true,a:hex('0080ffffff7f0180'),b:hex('00800100ffff0200'),out:hex('0040ffffffffffff'),expectedHelpers:0},
 {name:'pmulhuw-mmx-unsigned-high-words',code:'0fe4c1',direct:true,mmx:true,a:hex('0080ffffff7f0180'),b:hex('00800100ffff0200'),out:hex('00400000fe7f0100'),expectedHelpers:0},
 {name:'pmulhw-xmm-self-alias-captures-original-lanes',code:'660fe5c0',direct:true,a:hex('0080ffffff7f0180010000000040ffff'),out:hex('00400000ff3fff3f0000000000100000'),expectedHelpers:0},
 {name:'pmulhuw-mmx-self-alias-captures-original-lanes',code:'0fe4c0',direct:true,mmx:true,a:hex('0080ffffff7f0180'),out:hex('0040feffff3f0140'),expectedHelpers:0},
 {name:'pmulhw-xmm-aligned-memory-reads-full16',code:'660fe500',direct:true,a:hex('0080ffffff7f0180010000000040ffff'),memory:hex('00800100ffff0200ffffffff0400ffff'),address:DATA,readAlignment:16,expectedHelpers:1,out:hex('0040ffffffffffffffff000001000000')},
 {name:'pmulhuw-mmx-unaligned-memory-reads-full8',code:'0fe400',direct:true,mmx:true,a:hex('0080ffffff7f0180'),memory:hex('00800100ffff0200'),address:DATA+1,readAlignment:1,expectedHelpers:1,out:hex('00400000fe7f0100')},
 {name:'pmulhw-xmm-alignment-precedes-unmapped-memory',code:'660fe500',direct:true,a:hex('0080ffffff7f0180010000000040ffff'),address:0x6001,reason:11,retired:0,detail:0,expectedHelpers:1},
 {name:'pmulhuw-xmm-unmapped-full16-preserves-destination',code:'660fe400',direct:true,a:hex('0080ffffff7f0180010000000040ffff'),address:0x6000,reason:5,retired:0,detail:1,fault:0x6000,access:1,length:16,readAlignment:16,expectedHelpers:1,stagedTransfer:fill(0x97)},
 {name:'pmulhw-mmx-split-full8-preserves-destination-and-transfer',code:'0fe500',direct:true,mmx:true,a:hex('0080ffffff7f0180'),address:0x5ffc,seedMemory:dwords([0x12345678]),reason:5,retired:0,detail:1,fault:0x6000,access:1,length:8,readAlignment:1,expectedHelpers:1,stagedTransfer:Buffer.alloc(8,0xb3)},
 {name:'pending-x87-stops-mmx-multiply-high-before-memory-read',code:'0fe400',direct:true,mmx:true,pendingX87:true,a:hex('0080ffffff7f0180'),address:0x5ffc,seedMemory:dwords([0x12345678]),reason:12,retired:0,detail:0x8081,expectedHelpers:0},
 {name:'pmuludq-xmm-full-unsigned-products-ignore-odd-dwords',code:'660ff4c1',direct:true,a:hex('ffffffff7856341200000080ffffffff'),b:hex('ffffffff214365870200000000000000'),out:hex('01000000feffffff0000000001000000'),mxcsr:0xdfe5,expectedHelpers:0},
 {name:'pmuludq-mmx-full-unsigned-product-ignores-odd-dword',code:'0ff4c1',direct:true,mmx:true,a:hex('ffffffff78563412'),b:hex('ffffffff21436587'),out:hex('01000000feffffff'),expectedHelpers:0},
 {name:'psadbw-xmm-unsigned-independent-eight-byte-sums',code:'660ff6c1',direct:true,a:hex('00ff807f0102fa100001020304050607'),b:hex('ff007f8002010520fffefdfcfbfaf9f8'),out:hex('0703000000000000c007000000000000'),mxcsr:0x8040,expectedHelpers:0},
 {name:'psadbw-mmx-maximum-unsigned-byte-sum',code:'0ff6c1',direct:true,mmx:true,a:hex('0000000000000000'),b:hex('ffffffffffffffff'),out:hex('f807000000000000'),expectedHelpers:0},
 {name:'pmuludq-xmm-self-alias-captures-original-even-dwords',code:'660ff4c0',direct:true,a:hex('ffffffff7856341200000080ffffffff'),out:hex('01000000feffffff0000000000000040'),expectedHelpers:0},
 {name:'psadbw-mmx-self-alias-zeroes-complete-result',code:'0ff6c0',direct:true,mmx:true,a:hex('00ff807f0102fa10'),out:hex('0000000000000000'),expectedHelpers:0},
 {name:'psadbw-xmm-aligned-memory-reads-full16',code:'660ff600',direct:true,a:hex('00ff807f0102fa100001020304050607'),memory:hex('ff007f8002010520fffefdfcfbfaf9f8'),address:DATA,readAlignment:16,expectedHelpers:1,out:hex('0703000000000000c007000000000000')},
 {name:'pmuludq-mmx-unaligned-memory-reads-full8',code:'0ff400',direct:true,mmx:true,a:hex('ffffffff78563412'),memory:hex('ffffffff21436587'),address:DATA+1,readAlignment:1,expectedHelpers:1,out:hex('01000000feffffff')},
 {name:'pmuludq-xmm-alignment-precedes-unmapped-memory',code:'660ff400',direct:true,a:hex('ffffffff7856341200000080ffffffff'),address:0x6001,reason:11,retired:0,detail:0,expectedHelpers:1},
 {name:'psadbw-xmm-unmapped-full16-preserves-destination',code:'660ff600',direct:true,a:hex('00ff807f0102fa100001020304050607'),address:0x6000,reason:5,retired:0,detail:1,fault:0x6000,access:1,length:16,readAlignment:16,expectedHelpers:1,stagedTransfer:fill(0x97)},
 {name:'pmuludq-mmx-split-full8-preserves-destination-and-transfer',code:'0ff400',direct:true,mmx:true,a:hex('ffffffff78563412'),address:0x5ffc,seedMemory:dwords([0xffffffff]),reason:5,retired:0,detail:1,fault:0x6000,access:1,length:8,readAlignment:1,expectedHelpers:1,stagedTransfer:Buffer.alloc(8,0xb3)},
 {name:'pending-x87-stops-mmx-sad-before-memory-read',code:'0ff600',direct:true,mmx:true,pendingX87:true,a:hex('00ff807f0102fa10'),address:0x5ffc,seedMemory:dwords([0x12345678]),reason:12,retired:0,detail:0x8081,expectedHelpers:0},
 {name:'pslldq-zero-preserves-all-bytes',code:'660f73f800',direct:true,a:hex('0102030405060708090a0b0c0d0e0f10'),out:hex('0102030405060708090a0b0c0d0e0f10'),expectedHelpers:0},
 {name:'pslldq-one-preserves-little-endian-byte-order',code:'660f73f801',direct:true,a:hex('0102030405060708090a0b0c0d0e0f10'),out:hex('000102030405060708090a0b0c0d0e0f'),expectedHelpers:0},
 {name:'pslldq-fifteen-retains-only-low-byte',code:'660f73f80f',direct:true,a:hex('0102030405060708090a0b0c0d0e0f10'),out:hex('00000000000000000000000000000001'),expectedHelpers:0},
 {name:'pslldq-sixteen-clears-every-byte',code:'660f73f810',direct:true,a:hex('0102030405060708090a0b0c0d0e0f10'),out:hex('00000000000000000000000000000000'),expectedHelpers:0},
 {name:'psrldq-one-preserves-little-endian-byte-order',code:'660f73d801',direct:true,a:hex('0102030405060708090a0b0c0d0e0f10'),out:hex('02030405060708090a0b0c0d0e0f1000'),expectedHelpers:0},
 {name:'psrldq-fifteen-retains-only-high-byte',code:'660f73d80f',direct:true,a:hex('0102030405060708090a0b0c0d0e0f10'),out:hex('10000000000000000000000000000000'),expectedHelpers:0},
 {name:'psrldq-full-u8-count-clears-every-byte',code:'660f73d8ff',direct:true,a:hex('0102030405060708090a0b0c0d0e0f10'),out:hex('00000000000000000000000000000000'),expectedHelpers:0},
 {name:'pinsrw-xmm-masks-selector-and-gpr-low16',code:'660fc4c1ff',direct:true,a:hex('0102030405060708090a0b0c0d0e0f10'),out:hex('0102030405060708090a0b0c0d0e5141'),expectedHelpers:0},
 {name:'pinsrw-mmx-masks-selector-and-gpr-low16',code:'0fc4c1ff',direct:true,mmx:true,a:hex('0102030405060708'),out:hex('0102030405065141'),expectedHelpers:0},
 {name:'pinsrw-xmm-last-mapped-word-reads-only2',code:'660fc400fa',direct:true,a:hex('0102030405060708090a0b0c0d0e0f10'),memory:hex('efbe'),address:0x5ffe,readAlignment:1,out:hex('01020304efbe0708090a0b0c0d0e0f10'),expectedHelpers:1},
 {name:'pinsrw-mmx-odd-memory-word-reads-only2',code:'0fc400fe',direct:true,mmx:true,a:hex('0102030405060708'),memory:hex('efbe'),address:DATA+1,readAlignment:1,out:hex('01020304efbe0708'),expectedHelpers:1},
 {name:'pinsrw-xmm-split-word-preserves-destination-and-transfer',code:'660fc400ff',direct:true,a:hex('0102030405060708090a0b0c0d0e0f10'),address:0x5fff,seedMemory:hex('ef'),reason:5,retired:0,detail:1,fault:0x6000,access:1,length:2,readAlignment:1,stagedTransfer:hex('a7b3'),expectedHelpers:1},
 {name:'pending-x87-stops-mmx-insert-before-word-read',code:'0fc400ff',direct:true,mmx:true,pendingX87:true,a:hex('0102030405060708'),address:0x5fff,seedMemory:hex('ef'),reason:12,retired:0,detail:0x8081,expectedHelpers:0},
];
const ssse3Cases=[
 {name:'pshufb-control-high-bits-and-zero',code:'660f3800c1',a:hex('000102030405060708090a0b0c0d0e0f'),b:hex('0f0e8d0c1b2a39485766758493a2b1c0'),out:hex('0f0e000c0b0a09080706050000000000')},
 {name:'phaddw-wrap',code:'660f3801c1',a:words([32767,1,-32768,-1,10,20,-10,-20]),b:words([30000,30000,-30000,-30000,1,2,3,4]),out:words([-32768,32767,30,-30,-5536,5536,3,7])},
 {name:'phaddd-wrap',code:'660f3802c1',a:dwords([0x7fffffff,1,0xffffffff,2]),b:dwords([0x80000000,0x80000000,10,20]),out:dwords([0x80000000,1,0,30])},
 {name:'phaddsw-saturate',code:'660f3803c1',a:words([32767,1,-32768,-1,10,20,-10,-20]),b:words([30000,30000,-30000,-30000,1,2,3,4]),out:words([32767,-32768,30,-30,32767,-32768,3,7])},
 {name:'pmaddubsw-mixed-signedness-saturate',code:'660f3804c1',a:hex('ffffffffffffffff0102030405060708'),b:hex('7f7f808001ff7f800102fd0405fa0708'),out:words([32767,-32768,0,-255,5,7,-11,113])},
 {name:'phsubw-wrap',code:'660f3805c1',a:words([-32768,1,32767,-1,10,20,-10,-20]),b:words([30000,-30000,-30000,30000,1,2,3,4]),out:words([32767,-32768,-10,10,-5536,5536,-1,-1])},
 {name:'phsubd-wrap',code:'660f3806c1',a:dwords([0x80000000,1,0x7fffffff,0xffffffff]),b:dwords([0x80000000,0x80000000,10,20]),out:dwords([0x7fffffff,0x80000000,0,0xfffffff6])},
 {name:'phsubsw-saturate',code:'660f3807c1',a:words([-32768,1,32767,-1,10,20,-10,-20]),b:words([30000,-30000,-30000,30000,1,2,3,4]),out:words([-32768,32767,-10,10,32767,-32768,-1,-1])},
 {name:'psignb-zero-and-minimum',code:'660f3808c1',a:hex('80017fff1122334480017fff11223344'),b:hex('ff00017fff00017fff00017fff00017f'),out:hex('80007fffef00334480007fffef003344')},
 {name:'psignw-zero-and-minimum',code:'660f3809c1',a:words([-32768,1,32767,-1,11,22,33,44]),b:words([-1,0,1,32767,-32768,0,1,-1]),out:words([-32768,0,32767,-1,-11,0,33,-44])},
 {name:'psignd-zero-and-minimum',code:'660f380ac1',a:dwords([0x80000000,1,0x7fffffff,0xffffffff]),b:dwords([0xffffffff,0,1,0x80000000]),out:dwords([0x80000000,0,0x7fffffff,1])},
 {name:'pmulhrsw-round-without-saturation',code:'660f380bc1',a:words([-32768,16384,-16384,1,-1,32767,3,-3]),b:words([-32768,16384,16384,16384,16384,32767,16384,16384]),out:words([-32768,8192,-8192,1,0,32766,2,-1])},
 {name:'pabsb-minimum-wrap',code:'660f381cc1',a:fill(0x55),b:hex('80ff007f817e827d837c847b857a8679'),out:hex('8001007f7f7e7e7d7d7c7c7b7b7a7a79')},
 {name:'pabsw-minimum-wrap',code:'660f381dc1',a:fill(0x55),b:words([-32768,-1,0,32767,-2,2,-30000,30000]),out:words([-32768,1,0,32767,2,2,30000,30000])},
 {name:'pabsd-minimum-wrap',code:'660f381ec1',a:fill(0x55),b:dwords([0x80000000,0xffffffff,0,0x7fffffff]),out:dwords([0x80000000,1,0,0x7fffffff])},
];
for(const row of ssse3Cases){cases.push({...row,direct:true});if(!row.name.startsWith('phadd')&&!row.name.startsWith('phsub')&&!row.name.startsWith('pshufb'))cases.push({...row,name:'mmx-'+row.name,code:row.code.slice(2),direct:true,mmx:true});}
for(const size of[8,16])for(const count of[0,7,8,15,16,31,32,255]){
 const a=hex('000102030405060708090a0b0c0d0e0f'),b=hex('101112131415161718191a1b1c1d1e1f'),out=Buffer.alloc(16);
 Buffer.concat([b.subarray(0,size),a.subarray(0,size)]).subarray(count,count+size).copy(out);
 cases.push({name:`palignr-${size}-count-${count}`,code:(size===16?'66':'')+'0f3a0fc1'+count.toString(16).padStart(2,'0'),direct:true,mmx:size===8,a,b,out});
}
cases.push(
 {name:'mmx-pshufb-control-low3',code:'0f3800c1',direct:true,mmx:true,a:hex('0001020304050607'),b:hex('0f0e8d0c1b2a3948'),out:hex('0706000403020100')},
 {name:'mmx-phaddw-half-concatenation',code:'0f3801c1',direct:true,mmx:true,a:words([1,2,3,4]),b:words([10,20,30,40]),out:words([3,7,30,70])},
 {name:'pshufb-aligned-memory-control',code:'660f380000',direct:true,a:hex('000102030405060708090a0b0c0d0e0f'),memory:hex('0f0e0d0c0b0a09080706050403020100'),out:hex('0f0e0d0c0b0a09080706050403020100'),expectedHelpers:1},
 {name:'ssse3-unaligned-memory-gp',code:'660f380100',direct:true,a:fill(0x55),address:DATA+1,reason:11,retired:0,expectedHelpers:1},
 {name:'ssse3-mmx-split-memory-fault',code:'0f380800',direct:true,mmx:true,a:fill(0x55),address:0x5ffc,seedMemory:hex('01020304'),stagedTransfer:hex('a7b3c4d5e6f70819'),reason:5,retired:0,detail:1,fault:0x6000,access:1,length:8,readAlignment:1,expectedHelpers:1},
);
cases.push(
 {name:'sse3-lddqu-unaligned-bits',code:'f20ff000',direct:true,a:fill(0x55),memory:hex('341200000000f07f1122334455667788'),address:DATA+1,readAlignment:1,out:hex('341200000000f07f1122334455667788'),expectedHelpers:1},
 {name:'sse3-movddup-snan-raw',code:'f20f12c1',direct:true,a:fill(0x55),b:qwords([0x7ff0000000001234n,0x8765432101234567n]),out:qwords([0x7ff0000000001234n,0x7ff0000000001234n])},
 {name:'sse3-movsldup-snan-raw',code:'f30f12c1',direct:true,a:fill(0x55),b:dwords([0x7f800001,0x12345678,0x7f800002,0x87654321]),out:dwords([0x7f800001,0x7f800001,0x7f800002,0x7f800002])},
 {name:'sse3-movshdup-snan-raw',code:'f30f16c1',direct:true,a:fill(0x55),b:dwords([0x12345678,0x7f800001,0x87654321,0x7f800002]),out:dwords([0x7f800001,0x7f800001,0x7f800002,0x7f800002])},
 {name:'sse3-movddup-last8-mapped-bytes',code:'f20f1200',direct:true,a:fill(0x55),memory:qwords([0x7ff0000000001234n]),address:0x5ff8,readAlignment:1,out:qwords([0x7ff0000000001234n,0x7ff0000000001234n]),expectedHelpers:1},
 {name:'sse3-movddup-split8-preserves-state',code:'f20f1200',direct:true,a:fill(0x55),address:0x5ff9,seedMemory:hex('01020304050607'),stagedTransfer:hex('a7b3c4d5e6f70819'),reason:5,retired:0,detail:1,fault:0x6000,access:1,length:8,readAlignment:1,expectedHelpers:1},
 {name:'sse3-addsubps-lane-polarity',code:'f20fd0c1',a:dwords([0x3f800000,0x40000000,0x40400000,0x40800000]),b:dwords([0x40a00000,0x40c00000,0x40e00000,0x41000000]),out:dwords([0xc0800000,0x41000000,0xc0800000,0x41400000])},
 {name:'sse3-addsubpd-lane-polarity',code:'660fd0c1',a:qwords([0x3ff0000000000000n,0x4000000000000000n]),b:qwords([0x4008000000000000n,0x4010000000000000n]),out:qwords([0xc000000000000000n,0x4018000000000000n])},
 {name:'sse3-haddps-input-pairs',code:'f20f7cc1',a:dwords([0x3f800000,0x40000000,0x40400000,0x40800000]),b:dwords([0x40a00000,0x40c00000,0x40e00000,0x41000000]),out:dwords([0x40400000,0x40e00000,0x41300000,0x41700000])},
 {name:'sse3-haddpd-input-pairs',code:'660f7cc1',a:qwords([0x3ff0000000000000n,0x4000000000000000n]),b:qwords([0x4008000000000000n,0x4010000000000000n]),out:qwords([0x4008000000000000n,0x401c000000000000n])},
 {name:'sse3-hsubps-input-pairs',code:'f20f7dc1',a:dwords([0x3f800000,0x40000000,0x40400000,0x40800000]),b:dwords([0x40a00000,0x40c00000,0x40e00000,0x41000000]),out:dwords([0xbf800000,0xbf800000,0xbf800000,0xbf800000])},
 {name:'sse3-hsubpd-input-pairs',code:'660f7dc1',a:qwords([0x3ff0000000000000n,0x4000000000000000n]),b:qwords([0x4008000000000000n,0x4010000000000000n]),out:qwords([0xbff0000000000000n,0xbff0000000000000n])},
 {name:'sse3-haddps-round-up-precision',code:'f20f7cc1',a:dwords([0x3f800000,0x33800000,0x40000000,0x40400000]),b:dwords([0x40800000,0x40a00000,0x40c00000,0x40e00000]),out:dwords([0x3f800001,0x40a00000,0x41100000,0x41500000]),mxcsr:0x5f80,afterMxcsr:0x5fa0},
 {name:'sse3-hsubpd-signed-zero',code:'660f7dc1',a:qwords([0n,0x8000000000000000n]),b:qwords([0x8000000000000000n,0n]),out:qwords([0n,0x8000000000000000n])},
 {name:'sse3-addsubps-unmasked-invalid-transaction',code:'f20fd0c1',a:dwords([0x7f800000,0x3f800000,0x40000000,0x40400000]),b:dwords([0x7f800000,0x3f800000,0x40000000,0x40400000]),mxcsr:0x1f00,afterMxcsr:0x1f01,reason:13,retired:0,detail:0x1f01},
 {name:'sse3-haddpd-unmasked-invalid-transaction',code:'660f7cc1',a:qwords([0x7ff0000000000000n,0xfff0000000000000n]),b:zero,mxcsr:0x1f00,afterMxcsr:0x1f01,reason:13,retired:0,detail:0x1f01},
 {name:'sse3-haddps-self-alias',code:'f20f7cc0',a:dwords([0x3f800000,0x40000000,0x40400000,0x40800000]),out:dwords([0x40400000,0x40e00000,0x40400000,0x40e00000])},
 {name:'sse3-addsubps-daz',code:'f20fd0c1',a:dwords([1,0,0,0]),b:zero,out:zero,mxcsr:0x1fc0},
 {name:'sse3-haddps-aligned-source-memory',code:'f20f7c00',a:dwords([0x3f800000,0x40000000,0x40400000,0x40800000]),memory:dwords([0x40a00000,0x40c00000,0x40e00000,0x41000000]),readAlignment:16,out:dwords([0x40400000,0x40e00000,0x41300000,0x41700000]),address:DATA},
 {name:'sse3-haddps-unaligned-before-floating-exception',code:'f20f7c00',a:fill(0xff),address:DATA+1,mxcsr:0x1f00,reason:11,retired:0,expectedHelpers:1},
);
cases.push(
 {name:'sse4-pmuldq-signed-even-lanes',code:'660f3828c1',direct:true,a:dwords([0xfffffffe,0xdeadbeef,0x80000000,0x12345678]),b:dwords([0xfffffffd,0x87654321,0xffffffff,0x7fffffff]),out:qwords([6n,0x80000000n])},
 {name:'sse4-pmuldq-self-alias',code:'660f3828c0',direct:true,a:dwords([0xfffffffe,7,0x80000000,8]),out:qwords([4n,0x4000000000000000n])},
 {name:'sse4-pmulld-low32-wrap',code:'660f3840c1',direct:true,a:dwords([0xffffffff,0x80000000,0x7fffffff,0x10000]),b:dwords([0xffffffff,2,2,0x10000]),out:dwords([1,0,0xfffffffe,0])},
 {name:'sse4-pcmpeqq-complete-lanes',code:'660f3829c1',direct:true,a:qwords([0xffffffffffffffffn,0n]),b:qwords([0xffffffffffffffffn,1n]),out:qwords([0xffffffffffffffffn,0n])},
 {name:'sse4-pcmpgtq-signed-boundaries',code:'660f3837c1',direct:true,a:qwords([0x8000000000000000n,0x7fffffffffffffffn]),b:qwords([0xffffffffffffffffn,0x8000000000000000n]),out:qwords([0n,0xffffffffffffffffn])},
 {name:'sse4-pminsb-signed-boundaries',code:'660f3838c1',direct:true,a:fill(0x80),b:fill(0x7f),out:fill(0x80)},
 {name:'sse4-pmaxsb-signed-boundaries',code:'660f383cc1',direct:true,a:fill(0x80),b:fill(0x7f),out:fill(0x7f)},
 {name:'sse4-pminsd-signed-boundaries',code:'660f3839c1',direct:true,a:dwords([0x80000000,0x7fffffff,0xffffffff,0]),b:dwords([0x7fffffff,0x80000000,0,0xffffffff]),out:dwords([0x80000000,0x80000000,0xffffffff,0xffffffff])},
 {name:'sse4-pmaxsd-signed-boundaries',code:'660f383dc1',direct:true,a:dwords([0x80000000,0x7fffffff,0xffffffff,0]),b:dwords([0x7fffffff,0x80000000,0,0xffffffff]),out:dwords([0x7fffffff,0x7fffffff,0,0])},
 {name:'sse4-pminuw-unsigned-boundaries',code:'660f383ac1',direct:true,a:words([0xffff,0,0x8000,0x7fff,10,11,12,13]),b:words([1,1,0x7fff,0x8000,10,12,11,13]),out:words([1,0,0x7fff,0x7fff,10,11,11,13])},
 {name:'sse4-pmaxuw-unsigned-boundaries',code:'660f383ec1',direct:true,a:words([0xffff,0,0x8000,0x7fff,10,11,12,13]),b:words([1,1,0x7fff,0x8000,10,12,11,13]),out:words([0xffff,1,0x8000,0x8000,10,12,12,13])},
 {name:'sse4-pminud-unsigned-boundaries',code:'660f383bc1',direct:true,a:dwords([0x80000000,0x7fffffff,0xffffffff,0]),b:dwords([0x7fffffff,0x80000000,0,0xffffffff]),out:dwords([0x7fffffff,0x7fffffff,0,0])},
 {name:'sse4-pmaxud-unsigned-boundaries',code:'660f383fc1',direct:true,a:dwords([0x80000000,0x7fffffff,0xffffffff,0]),b:dwords([0x7fffffff,0x80000000,0,0xffffffff]),out:dwords([0x80000000,0x80000000,0xffffffff,0xffffffff])},
 {name:'sse4-packusdw-signed-source-saturation',code:'660f382bc1',direct:true,a:dwords([0xffffffff,0,65535,65536]),b:dwords([0x80000000,0x7fffffff,42,0xffffffd6]),out:words([0,0,65535,65535,0,65535,42,0])},
 {name:'sse4-movntdqa-raw-aligned-load',code:'660f382a00',direct:true,a:fill(0x55),memory:hex('341200000000f07f1122334455667788'),address:DATA,readAlignment:16,out:hex('341200000000f07f1122334455667788'),expectedHelpers:1},
 {name:'sse4-movntdqa-unaligned-gp',code:'660f382a00',direct:true,a:fill(0x55),address:DATA+1,reason:11,retired:0,expectedHelpers:1},
 {name:'sse4-pmulld-aligned-source-memory',code:'660f384000',direct:true,a:dwords([0xffffffff,0x80000000,0x7fffffff,0x10000]),memory:dwords([0xffffffff,2,2,0x10000]),address:DATA,readAlignment:16,out:dwords([1,0,0xfffffffe,0]),expectedHelpers:1},
 {name:'sse4-packusdw-memory-before-writeback',code:'660f382b00',direct:true,a:fill(0x55),address:0x6000,stagedTransfer:fill(0x77),reason:5,retired:0,detail:1,fault:0x6000,access:1,length:16,readAlignment:16,expectedHelpers:1},
);
cases.push(
 {name:'sse4-pmovsxbw-low-lanes',code:'660f3820c1',direct:true,a:fill(0xa5),b:hex('807fff8001fe81ffaabbccddeeff6677'),out:hex('80ff7f00ffff80ff0100feff81ffffff'),mxcsr:0xdfe5,expectedHelpers:0},
 {name:'sse4-pmovsxbw-exact-last-mapped-source',code:'660f382000',direct:true,a:fill(0xa5),memory:hex('807fff8001fe81ff'),address:0x5ff8,readAlignment:1,out:hex('80ff7f00ffff80ff0100feff81ffffff'),expectedHelpers:1},
 {name:'sse4-pmovsxbd-low-lanes',code:'660f3821c1',direct:true,a:fill(0xa5),b:hex('807fff8001fe81ffaabbccddeeff6677'),out:hex('80ffffff7f000000ffffffff80ffffff'),mxcsr:0xdfe5,expectedHelpers:0},
 {name:'sse4-pmovsxbd-exact-last-mapped-source',code:'660f382100',direct:true,a:fill(0xa5),memory:hex('807fff80'),address:0x5ffc,readAlignment:1,out:hex('80ffffff7f000000ffffffff80ffffff'),expectedHelpers:1},
 {name:'sse4-pmovsxbq-low-lanes',code:'660f3822c1',direct:true,a:fill(0xa5),b:hex('807fff8001fe81ffaabbccddeeff6677'),out:hex('80ffffffffffffff7f00000000000000'),mxcsr:0xdfe5,expectedHelpers:0},
 {name:'sse4-pmovsxbq-exact-last-mapped-source',code:'660f382200',direct:true,a:fill(0xa5),memory:hex('807f'),address:0x5ffe,readAlignment:1,out:hex('80ffffffffffffff7f00000000000000'),expectedHelpers:1},
 {name:'sse4-pmovsxwd-low-lanes',code:'660f3823c1',direct:true,a:fill(0xa5),b:hex('807fff8001fe81ffaabbccddeeff6677'),out:hex('807f0000ff80ffff01feffff81ffffff'),mxcsr:0xdfe5,expectedHelpers:0},
 {name:'sse4-pmovsxwd-exact-last-mapped-source',code:'660f382300',direct:true,a:fill(0xa5),memory:hex('807fff8001fe81ff'),address:0x5ff8,readAlignment:1,out:hex('807f0000ff80ffff01feffff81ffffff'),expectedHelpers:1},
 {name:'sse4-pmovsxwq-low-lanes',code:'660f3824c1',direct:true,a:fill(0xa5),b:hex('807fff8001fe81ffaabbccddeeff6677'),out:hex('807f000000000000ff80ffffffffffff'),mxcsr:0xdfe5,expectedHelpers:0},
 {name:'sse4-pmovsxwq-exact-last-mapped-source',code:'660f382400',direct:true,a:fill(0xa5),memory:hex('807fff80'),address:0x5ffc,readAlignment:1,out:hex('807f000000000000ff80ffffffffffff'),expectedHelpers:1},
 {name:'sse4-pmovsxdq-low-lanes',code:'660f3825c1',direct:true,a:fill(0xa5),b:hex('807fff8001fe81ffaabbccddeeff6677'),out:hex('807fff80ffffffff01fe81ffffffffff'),mxcsr:0xdfe5,expectedHelpers:0},
 {name:'sse4-pmovsxdq-exact-last-mapped-source',code:'660f382500',direct:true,a:fill(0xa5),memory:hex('807fff8001fe81ff'),address:0x5ff8,readAlignment:1,out:hex('807fff80ffffffff01fe81ffffffffff'),expectedHelpers:1},
 {name:'sse4-pmovzxbw-low-lanes',code:'660f3830c1',direct:true,a:fill(0xa5),b:hex('807fff8001fe81ffaabbccddeeff6677'),out:hex('80007f00ff0080000100fe008100ff00'),mxcsr:0xdfe5,expectedHelpers:0},
 {name:'sse4-pmovzxbw-exact-last-mapped-source',code:'660f383000',direct:true,a:fill(0xa5),memory:hex('807fff8001fe81ff'),address:0x5ff8,readAlignment:1,out:hex('80007f00ff0080000100fe008100ff00'),expectedHelpers:1},
 {name:'sse4-pmovzxbd-low-lanes',code:'660f3831c1',direct:true,a:fill(0xa5),b:hex('807fff8001fe81ffaabbccddeeff6677'),out:hex('800000007f000000ff00000080000000'),mxcsr:0xdfe5,expectedHelpers:0},
 {name:'sse4-pmovzxbd-exact-last-mapped-source',code:'660f383100',direct:true,a:fill(0xa5),memory:hex('807fff80'),address:0x5ffc,readAlignment:1,out:hex('800000007f000000ff00000080000000'),expectedHelpers:1},
 {name:'sse4-pmovzxbq-low-lanes',code:'660f3832c1',direct:true,a:fill(0xa5),b:hex('807fff8001fe81ffaabbccddeeff6677'),out:hex('80000000000000007f00000000000000'),mxcsr:0xdfe5,expectedHelpers:0},
 {name:'sse4-pmovzxbq-exact-last-mapped-source',code:'660f383200',direct:true,a:fill(0xa5),memory:hex('807f'),address:0x5ffe,readAlignment:1,out:hex('80000000000000007f00000000000000'),expectedHelpers:1},
 {name:'sse4-pmovzxwd-low-lanes',code:'660f3833c1',direct:true,a:fill(0xa5),b:hex('807fff8001fe81ffaabbccddeeff6677'),out:hex('807f0000ff80000001fe000081ff0000'),mxcsr:0xdfe5,expectedHelpers:0},
 {name:'sse4-pmovzxwd-exact-last-mapped-source',code:'660f383300',direct:true,a:fill(0xa5),memory:hex('807fff8001fe81ff'),address:0x5ff8,readAlignment:1,out:hex('807f0000ff80000001fe000081ff0000'),expectedHelpers:1},
 {name:'sse4-pmovzxwq-low-lanes',code:'660f3834c1',direct:true,a:fill(0xa5),b:hex('807fff8001fe81ffaabbccddeeff6677'),out:hex('807f000000000000ff80000000000000'),mxcsr:0xdfe5,expectedHelpers:0},
 {name:'sse4-pmovzxwq-exact-last-mapped-source',code:'660f383400',direct:true,a:fill(0xa5),memory:hex('807fff80'),address:0x5ffc,readAlignment:1,out:hex('807f000000000000ff80000000000000'),expectedHelpers:1},
 {name:'sse4-pmovzxdq-low-lanes',code:'660f3835c1',direct:true,a:fill(0xa5),b:hex('807fff8001fe81ffaabbccddeeff6677'),out:hex('807fff800000000001fe81ff00000000'),mxcsr:0xdfe5,expectedHelpers:0},
 {name:'sse4-pmovzxdq-exact-last-mapped-source',code:'660f383500',direct:true,a:fill(0xa5),memory:hex('807fff8001fe81ff'),address:0x5ff8,readAlignment:1,out:hex('807fff800000000001fe81ff00000000'),expectedHelpers:1},
 {name:'sse4-blendps-immediate-masks-high-bits-raw-snan',code:'660f3a0cc1f5',direct:true,a:dwords([1,2,3,4]),b:dwords([0x7f800001,0x80000000,0x7fc01234,0x3f800000]),out:dwords([0x7f800001,2,0x7fc01234,4]),mxcsr:0x1f00},
 {name:'sse4-blendpd-immediate-raw-signaling-bits',code:'660f3a0dc1fe',direct:true,a:qwords([0x3ff0000000000000n,0x4000000000000000n]),b:qwords([0x7ff0000000000001n,0x8000000000000000n]),out:qwords([0x3ff0000000000000n,0x8000000000000000n]),mxcsr:0x1f00},
 {name:'sse4-pblendw-alternating-words',code:'660f3a0ec1aa',direct:true,a:words([1,2,3,4,5,6,7,8]),b:words([11,12,13,14,15,16,17,18]),out:words([1,12,3,14,5,16,7,18])},
 {name:'sse4-pblendvb-destination-is-implicit-mask',code:'660f3810c1',direct:true,a:hex('80018001800180018001800180018001'),b:fill(0x55),out:hex('55015501550155015501550155015501')},
 {name:'sse4-blendvps-tests-only-sign-bit',code:'660f3814c1',direct:true,a:dwords([0x80000000,0x7fffffff,0xffffffff,0]),b:dwords([0x7f800001,0x80000000,0x7fc01234,0x3f800000]),out:dwords([0x7f800001,0x7fffffff,0x7fc01234,0]),mxcsr:0x1f00},
 {name:'sse4-blendvpd-tests-only-sign-bit',code:'660f3815c1',direct:true,a:qwords([0x8000000000000000n,0x7fffffffffffffffn]),b:qwords([0x7ff0000000000001n,0x3ff0000000000000n]),out:qwords([0x7ff0000000000001n,0x7fffffffffffffffn]),mxcsr:0x1f00},
 {name:'sse4-blendvps-independent-implicit-xmm0',code:'660f3814d1',direct:true,initialXmm:Buffer.concat([dwords([0x80000000,0x7fffffff,0xffffffff,0]),dwords([0x7f800001,0x80000000,0x7fc01234,0x3f800000]),dwords([1,2,3,4]),Buffer.alloc(80)]),afterXmm:Buffer.concat([dwords([0x80000000,0x7fffffff,0xffffffff,0]),dwords([0x7f800001,0x80000000,0x7fc01234,0x3f800000]),dwords([0x7f800001,2,0x7fc01234,4]),Buffer.alloc(80)]),mxcsr:0x1f00},
 {name:'sse4-pblendvb-aligned-memory-mask-snapshot',code:'660f381000',direct:true,a:hex('80018001800180018001800180018001'),memory:fill(0x55),address:DATA,readAlignment:16,out:hex('55015501550155015501550155015501'),expectedHelpers:1},
 {name:'sse4-blendps-alignment-precedes-unmapped-read',code:'660f3a0c000f',direct:true,a:fill(0x19),address:0x6001,reason:11,retired:0,expectedHelpers:1},
 {name:'sse4-insertps-selects-then-zeroes-destination-lanes',code:'660f3a21c1b6',direct:true,a:dwords([1,2,3,4]),b:dwords([10,20,30,40]),out:dwords([1,0,0,30])},
 {name:'sse4-insertps-self-alias-snapshots-source',code:'660f3a21c031',direct:true,a:dwords([1,2,3,4]),out:dwords([0,2,3,1])},
 {name:'sse4-insertps-m32-ignores-source-selector-raw-snan',code:'660f3a2100c0',direct:true,a:dwords([1,2,3,4]),memory:dwords([0x7f801234]),address:0x5ffc,readAlignment:1,out:dwords([0x7f801234,2,3,4]),mxcsr:0x1f00,expectedHelpers:1},
 {name:'sse4-pinsrb-masks-selector-and-gpr-low8',code:'660f3a20c1ff',direct:true,a:hex('0102030405060708090a0b0c0d0e0f10'),out:hex('0102030405060708090a0b0c0d0e0f51')},
 {name:'sse4-pinsrd-masks-selector-and-gpr-low32',code:'660f3a22c1ff',direct:true,a:dwords([1,2,3,4]),out:dwords([1,2,3,0x21314151])},
 {name:'sse4-pinsrb-last-mapped-byte-reads-only1',code:'660f3a2000ff',direct:true,a:fill(0),memory:hex('ef'),address:0x5fff,readAlignment:1,out:hex('000000000000000000000000000000ef'),expectedHelpers:1},
 {name:'sse4-pinsrd-last-mapped-dword-reads-only4',code:'660f3a2200ff',direct:true,a:dwords([1,2,3,4]),memory:dwords([0x7f801234]),address:0x5ffc,readAlignment:1,out:dwords([1,2,3,0x7f801234]),expectedHelpers:1},
 {name:'sse4-pextrb-zero-extends-low8',code:'660f3a14c0ff',direct:true,a:hex('000000000000000000000000000000ef'),gpr:0xef},
 {name:'sse4-pextrw-new-encoding-zero-extends-low16',code:'660f3a15c0ff',direct:true,a:hex('0000000000000000000000000000ffff'),gpr:0xffff},
 {name:'sse4-pextrd-keeps-full32',code:'660f3a16c0ff',direct:true,a:dwords([1,2,3,0xffffffff]),gpr:0xffffffff},
 {name:'sse4-extractps-preserves-signaling-nan-bits',code:'660f3a17c0ff',direct:true,a:dwords([1,2,3,0x7f801234]),gpr:0x7f801234,mxcsr:0x1f00},
 {name:'sse4-pextrb-unaligned-store-only1-and-canaries',code:'660f3a1400ff',direct:true,a:hex('00000000000000000000000000000078'),address:DATA+1,write:hex('78666666666666666666666666666666'),writeLength:1,mxcsr:0x1f00},
 {name:'sse4-pextrw-unaligned-store-only2-and-canaries',code:'660f3a1500ff',direct:true,a:hex('00000000000000000000000000007856'),address:DATA+1,write:hex('78566666666666666666666666666666'),writeLength:2,mxcsr:0x1f00},
 {name:'sse4-pextrd-unaligned-store-only4-and-canaries',code:'660f3a1600ff',direct:true,a:hex('00000000000000000000000078563412'),address:DATA+1,write:hex('78563412666666666666666666666666'),writeLength:4,mxcsr:0x1f00},
 {name:'sse4-extractps-unaligned-store-only4-and-canaries',code:'660f3a1700ff',direct:true,a:hex('0000000000000000000000003412807f'),address:DATA+1,write:hex('3412807f666666666666666666666666'),writeLength:4,mxcsr:0x1f00},
 {name:'sse4-pmovsxbq-split-source-preserves-state-and-transfer',code:'660f382200',direct:true,a:fill(0x19),address:0x5fff,seedMemory:hex('80'),reason:5,retired:0,detail:1,fault:0x6000,access:1,length:2,readAlignment:1,stagedTransfer:hex('97b3'),expectedHelpers:1},
 {name:'sse4-pmovsxbw-split-source-preserves-state-and-transfer',code:'660f382000',direct:true,a:fill(0x19),address:0x5ff9,seedMemory:hex('807fff8001fe81'),reason:5,retired:0,detail:1,fault:0x6000,access:1,length:8,readAlignment:1,stagedTransfer:Buffer.alloc(8,0x97),expectedHelpers:1},
 {name:'sse4-pinsrb-unmapped-byte-preserves-state-and-transfer',code:'660f3a2000ff',direct:true,a:fill(0x19),address:0x6000,reason:5,retired:0,detail:1,fault:0x6000,access:1,length:1,readAlignment:1,stagedTransfer:hex('97'),expectedHelpers:1},
 {name:'sse4-insertps-zero-mask-still-reads-unmapped-source',code:'660f3a21000f',direct:true,a:fill(0x19),address:0x6000,reason:5,retired:0,detail:1,fault:0x6000,access:1,length:4,readAlignment:1,stagedTransfer:Buffer.alloc(4,0x97),expectedHelpers:1},
 {name:'sse4-pextrw-split-store-is-effect-neutral',code:'660f3a1500ff',direct:true,a:hex('0000000000000000000000000000efbe'),address:0x5fff,seedMemory:hex('66'),reason:5,retired:0,detail:1,fault:0x6000,access:2,length:2,unchangedMemory:hex('66')},
);
cases.push(
 {name:'sse4-roundps-nearest-even-and-negative-zero',code:'660f3a08c100',a:fill(0x19),b:dwords([0x3fc00000,0xbfc00000,0x40200000,0xbf000000]),out:dwords([0x40000000,0xc0000000,0x40000000,0x80000000]),afterMxcsr:0x1fa0},
 {name:'sse4-roundps-toward-negative',code:'660f3a08c101',a:fill(0x19),b:dwords([0x3fc00000,0xbfc00000,0x40200000,0xbf000000]),out:dwords([0x3f800000,0xc0000000,0x40000000,0xbf800000]),afterMxcsr:0x1fa0},
 {name:'sse4-roundps-toward-positive',code:'660f3a08c102',a:fill(0x19),b:dwords([0x3fc00000,0xbfc00000,0x40200000,0xbf000000]),out:dwords([0x40000000,0xbf800000,0x40400000,0x80000000]),afterMxcsr:0x1fa0},
 {name:'sse4-roundps-toward-zero',code:'660f3a08c103',a:fill(0x19),b:dwords([0x3fc00000,0xbfc00000,0x40200000,0xbf000000]),out:dwords([0x3f800000,0xbf800000,0x40000000,0x80000000]),afterMxcsr:0x1fa0},
 {name:'sse4-roundps-uses-mxcsr-mode-and-ignores-reserved-immediate',code:'660f3a08c1f4',a:fill(0x19),b:dwords([0x3fc00000,0xbfc00000,0x40200000,0xbf000000]),out:dwords([0x40000000,0xbf800000,0x40400000,0x80000000]),mxcsr:0x5f80,afterMxcsr:0x5fa0},
 {name:'sse4-roundpd-packed-nearest-even',code:'660f3a09c100',a:fill(0x19),b:qwords([0xbff8000000000000n,0x4004000000000000n]),out:qwords([0xc000000000000000n,0x4000000000000000n]),afterMxcsr:0x1fa0},
 {name:'sse4-roundss-preserves-upper-destination-ignores-upper-snan',code:'660f3a0ac100',a:dwords([1,2,3,4]),b:dwords([0x3fc00000,0x7f800001,0,0]),out:dwords([0x40000000,2,3,4]),afterMxcsr:0x1fa0},
 {name:'sse4-roundsd-preserves-upper-destination-ignores-upper-snan',code:'660f3a0bc101',a:qwords([1n,0x123456789abcdef0n]),b:qwords([0xbff8000000000000n,0x7ff0000000000001n]),out:qwords([0xc000000000000000n,0x123456789abcdef0n]),afterMxcsr:0x1fa0},
 {name:'sse4-roundps-unmasked-precision-preserves-destination',code:'660f3a08c100',a:fill(0x19),b:dwords([0x3fc00000,0,0,0]),mxcsr:0x0f80,afterMxcsr:0x0fa0,reason:13,retired:0,detail:0x0fa0},
 {name:'sse4-roundps-suppresses-unmasked-precision',code:'660f3a08c108',a:fill(0x19),b:dwords([0x3fc00000,0,0,0]),out:dwords([0x40000000,0,0,0]),mxcsr:0x0f80},
 {name:'sse4-roundps-snan-quiets-payload-even-with-precision-suppressed',code:'660f3a08c108',a:fill(0x19),b:dwords([0x7f801234,0x7fc05678,0x7f800000,0x80000000]),out:dwords([0x7fc01234,0x7fc05678,0x7f800000,0x80000000]),afterMxcsr:0x1f81},
 {name:'sse4-roundpd-snan-quiets-full-payload',code:'660f3a09c108',a:fill(0x19),b:qwords([0x7ff0000000001234n,0x8000000000000000n]),out:qwords([0x7ff8000000001234n,0x8000000000000000n]),afterMxcsr:0x1f81},
 {name:'sse4-roundps-unmasked-invalid-suppresses-other-lane-precision',code:'660f3a08c100',a:fill(0x19),b:dwords([0x7f801234,0x3fc00000,0,0]),mxcsr:0x1f00,afterMxcsr:0x1f01,reason:13,retired:0,detail:0x1f01},
 {name:'sse4-roundps-denormal-never-signals-denormal-exception',code:'660f3a08c102',a:fill(0x19),b:dwords([1,0x80000001,0,0x80000000]),out:dwords([0x3f800000,0x80000000,0,0x80000000]),mxcsr:0x1e80,afterMxcsr:0x1ea0},
 {name:'sse4-roundps-daz-preserves-signed-zero-without-precision',code:'660f3a08c102',a:fill(0x19),b:dwords([1,0x80000001,0,0x80000000]),out:dwords([0,0x80000000,0,0x80000000]),mxcsr:0x1ec0},
 {name:'sse4-roundss-last-mapped-dword',code:'660f3a0a0000',a:dwords([1,2,3,4]),memory:dwords([0x3fc00000]),address:0x5ffc,readAlignment:1,out:dwords([0x40000000,2,3,4]),afterMxcsr:0x1fa0},
 {name:'sse4-roundsd-unaligned-memory-qword',code:'660f3a0b0008',a:qwords([1n,0x123456789abcdef0n]),memory:qwords([0xbff8000000000000n]),address:DATA+1,readAlignment:1,out:qwords([0xc000000000000000n,0x123456789abcdef0n])},
 {name:'sse4-roundpd-aligned-memory-packed',code:'660f3a090008',a:fill(0x19),memory:qwords([0xbff8000000000000n,0x4004000000000000n]),address:DATA,readAlignment:16,out:qwords([0xc000000000000000n,0x4000000000000000n])},
 {name:'sse4-dpps-pairwise-rounding-differs-from-sequential-sum',code:'660f3a40c1ff',a:dwords([0x4b800000,0x3f800000,0x3f800000,0xcb800000]),b:dwords([0x3f800000,0x3f800000,0x3f800000,0x3f800000]),out:dwords([0x3f800000,0x3f800000,0x3f800000,0x3f800000]),afterMxcsr:0x1fa0},
 {name:'sse4-dpps-masked-inputs-do-not-raise-invalid',code:'660f3a40c111',a:dwords([0x3f800000,0x7f800001,0x7f800000,0x40800000]),b:dwords([0x40a00000,0,0,0x41000000]),out:dwords([0x40a00000,0,0,0]),mxcsr:0x1f00},
 {name:'sse4-dpps-zero-output-mask-still-computes-invalid',code:'660f3a40c1f0',a:dwords([0x7f801234,0,0,0]),b:dwords([0x3f800000,0,0,0]),out:zero,afterMxcsr:0x1f81},
 {name:'sse4-dpps-unmasked-invalid-retains-masked-denormal-stage-flags',code:'660f3a40c1ff',a:dwords([0x7f801234,1,2,3]),b:dwords([0x3f800000,0,0,0]),mxcsr:0x1f00,afterMxcsr:0x1f03,reason:13,retired:0,detail:0x1f03},
 {name:'sse4-dpps-isolated-unmasked-invalid-preserves-destination',code:'660f3a40c1ff',a:dwords([0x7f801234,0,0,0]),b:dwords([0x3f800000,0,0,0]),mxcsr:0x1f00,afterMxcsr:0x1f01,reason:13,retired:0,detail:0x1f01},
 {name:'sse4-dpps-first-unmasked-add-stage-precedes-later-invalid',code:'660f3a40c1ff',a:dwords([0x4b800000,0x3f800000,0x7f800000,0xff800000]),b:dwords([0x3f800000,0x3f800000,0x3f800000,0x3f800000]),mxcsr:0x0f00,afterMxcsr:0x0f20,reason:13,retired:0,detail:0x0f20},
 {name:'sse4-dpps-earlier-masked-precision-survives-later-invalid',code:'660f3a40c1ff',a:dwords([0x4b800000,0x3f800000,0x7f800000,0xff800000]),b:dwords([0x3f800000,0x3f800000,0x3f800000,0x3f800000]),mxcsr:0x1f00,afterMxcsr:0x1f21,reason:13,retired:0,detail:0x1f21},
 {name:'sse4-dppd-two-exact-products',code:'660f3a41c133',a:qwords([0x3ff0000000000000n,0x4000000000000000n]),b:qwords([0x4008000000000000n,0x4010000000000000n]),out:qwords([0x4026000000000000n,0x4026000000000000n])},
 {name:'sse4-dppd-unmasked-product-stage-precedes-invalid-sum',code:'660f3a41c133',a:qwords([0x7fefffffffffffffn,0xfff0000000000000n]),b:qwords([0x4000000000000000n,0x3ff0000000000000n]),mxcsr:0x0f00,afterMxcsr:0x0f28,reason:13,retired:0,detail:0x0f28},
 {name:'sse4-dppd-masks-input-and-ignores-reserved-immediate-bits',code:'660f3a41c1d6',a:qwords([0x3ff0000000000000n,0x7ff0000000000001n]),b:qwords([0x4008000000000000n,0n]),out:qwords([0n,0x4008000000000000n]),mxcsr:0x1f00},
 {name:'sse4-dppd-aligned-memory-exact',code:'660f3a410033',a:qwords([0x3ff0000000000000n,0x4000000000000000n]),memory:qwords([0x4008000000000000n,0x4010000000000000n]),address:DATA,readAlignment:16,out:qwords([0x4026000000000000n,0x4026000000000000n])},
 {name:'sse4-mpsadbw-block-offset-0',code:'660f3a42c100',direct:true,a:hex('000102030405060708090a0b0c0d0e0f'),b:hex('6465666768696a6b6c6d6e6f70717273'),out:words([400,396,392,388,384,380,376,372])},
 {name:'sse4-mpsadbw-block-offset-1',code:'660f3a42c101',direct:true,a:hex('000102030405060708090a0b0c0d0e0f'),b:hex('6465666768696a6b6c6d6e6f70717273'),out:words([416,412,408,404,400,396,392,388])},
 {name:'sse4-mpsadbw-block-offset-2',code:'660f3a42c102',direct:true,a:hex('000102030405060708090a0b0c0d0e0f'),b:hex('6465666768696a6b6c6d6e6f70717273'),out:words([432,428,424,420,416,412,408,404])},
 {name:'sse4-mpsadbw-block-offset-3',code:'660f3a42c103',direct:true,a:hex('000102030405060708090a0b0c0d0e0f'),b:hex('6465666768696a6b6c6d6e6f70717273'),out:words([448,444,440,436,432,428,424,420])},
 {name:'sse4-mpsadbw-block-offset-4',code:'660f3a42c104',direct:true,a:hex('000102030405060708090a0b0c0d0e0f'),b:hex('6465666768696a6b6c6d6e6f70717273'),out:words([384,380,376,372,368,364,360,356])},
 {name:'sse4-mpsadbw-block-offset-5',code:'660f3a42c105',direct:true,a:hex('000102030405060708090a0b0c0d0e0f'),b:hex('6465666768696a6b6c6d6e6f70717273'),out:words([400,396,392,388,384,380,376,372])},
 {name:'sse4-mpsadbw-block-offset-6',code:'660f3a42c106',direct:true,a:hex('000102030405060708090a0b0c0d0e0f'),b:hex('6465666768696a6b6c6d6e6f70717273'),out:words([416,412,408,404,400,396,392,388])},
 {name:'sse4-mpsadbw-block-offset-7',code:'660f3a42c107',direct:true,a:hex('000102030405060708090a0b0c0d0e0f'),b:hex('6465666768696a6b6c6d6e6f70717273'),out:words([432,428,424,420,416,412,408,404])},
 {name:'sse4-mpsadbw-ignores-high-immediate-bits',code:'660f3a42c1ff',direct:true,a:hex('000102030405060708090a0b0c0d0e0f'),b:hex('6465666768696a6b6c6d6e6f70717273'),out:words([432,428,424,420,416,412,408,404])},
 {name:'sse4-mpsadbw-maximum-unsigned-four-byte-sum',code:'660f3a42c100',direct:true,a:zero,b:fill(255),out:words([1020,1020,1020,1020,1020,1020,1020,1020])},
 {name:'sse4-mpsadbw-self-alias-captures-both-blocks',code:'660f3a42c007',direct:true,a:hex('000102030405060708090a0b0c0d0e0f'),out:words([32,28,24,20,16,12,8,4])},
 {name:'sse4-mpsadbw-aligned-memory-full16',code:'660f3a420000',direct:true,a:hex('000102030405060708090a0b0c0d0e0f'),memory:hex('6465666768696a6b6c6d6e6f70717273'),address:DATA,readAlignment:16,out:words([400,396,392,388,384,380,376,372]),expectedHelpers:1},
 {name:'sse4-phminposuw-lowest-index-wins-ties',code:'660f3841c1',direct:true,a:fill(0x19),b:words([65535,7,5,5,6,9,5,8]),out:words([5,2,0,0,0,0,0,0])},
 {name:'sse4-phminposuw-all-unsigned-maxima',code:'660f3841c1',direct:true,a:fill(0x19),b:fill(255),out:words([65535,0,0,0,0,0,0,0])},
 {name:'sse4-phminposuw-last-lane-and-self-alias',code:'660f3841c0',direct:true,a:words([32768,65535,3,4,5,6,7,0]),out:words([0,7,0,0,0,0,0,0])},
 {name:'sse4-phminposuw-aligned-memory-full16',code:'660f384100',direct:true,a:fill(0x19),memory:words([65535,7,5,5,6,9,5,8]),address:DATA,readAlignment:16,out:words([5,2,0,0,0,0,0,0]),expectedHelpers:1},
 {name:'sse4-ptest-zero-sets-carry-and-zero',code:'660f3817c1',direct:true,a:zero,b:zero,flags:0x443},
 {name:'sse4-ptest-disjoint-sets-zero-only',code:'660f3817c1',direct:true,a:zero,b:fill(255),flags:0x442},
 {name:'sse4-ptest-contained-sets-carry-only',code:'660f3817c1',direct:true,a:fill(255),b:fill(255),flags:0x403},
 {name:'sse4-ptest-mixed-clears-both-and-preserves-df',code:'660f3817c1',direct:true,a:fill(0x0f),b:fill(0x33),flags:0x402,mxcsr:0xdfe5},
 {name:'sse4-ptest-self-alias-preserves-vector',code:'660f3817c0',direct:true,a:fill(0x19),flags:0x403},
 {name:'sse4-ptest-aligned-memory-full16',code:'660f381700',direct:true,a:fill(0x0f),memory:fill(0x33),address:DATA,readAlignment:16,flags:0x402,expectedHelpers:1},
 {name:'sse4-ptest-alignment-before-flags',code:'660f381700',direct:true,a:fill(0x19),address:0x6001,reason:11,retired:0,expectedHelpers:1},
 {name:'sse4-ptest-unmapped-read-preserves-flags-and-vector',code:'660f381700',direct:true,a:fill(0x19),address:0x6000,reason:5,retired:0,detail:1,fault:0x6000,access:1,length:16,readAlignment:16,stagedTransfer:fill(0x97),expectedHelpers:1},
 {name:'sse4-mpsadbw-unmapped-read-preserves-destination',code:'660f3a420007',direct:true,a:fill(0x19),address:0x6000,reason:5,retired:0,detail:1,fault:0x6000,access:1,length:16,readAlignment:16,stagedTransfer:fill(0x97),expectedHelpers:1},
 {name:'sse4-roundps-alignment-precedes-numeric-effects',code:'660f3a080000',a:fill(0x19),address:0x6001,reason:11,retired:0},
 {name:'sse4-roundss-split-dword-preserves-state-and-transfer',code:'660f3a0a0000',a:fill(0x19),address:0x5ffe,seedMemory:hex('0000'),reason:5,retired:0,detail:1,fault:0x6000,access:1,length:4,readAlignment:1,stagedTransfer:Buffer.alloc(4,0x97),expectedHelpers:1},
 {name:'sse4-dpps-zero-input-output-masks-still-read-full-source',code:'660f3a400000',a:fill(0x19),address:0x6000,reason:5,retired:0,detail:1,fault:0x6000,access:1,length:16,readAlignment:16,stagedTransfer:fill(0x97),expectedHelpers:1},
);
const sourcePaths=['engine/src/cpu/x86/simd.rs','engine/src/cpu/x86/simd/packed.rs','engine/src/cpu/x86/simd/float.rs', 'engine/src/math/mod.rs', 'engine/src/math/ieee.rs', 'engine/src/math/integer.rs', 'engine/src/lib.rs','engine/src/cpu/x86/decode/simd.rs','engine/src/cpu/dbt/wasm/simd.rs','engine/src/cpu/dbt/wasm/simd/direct.rs','engine/src/process/cpu/simd.rs', 'engine/src/process/cpu/mod.rs', 'engine/src/process/mod.rs','engine/src/process/memory/span.rs', 'engine/src/process/memory/mod.rs', 'engine/src/process/mod.rs','engine/src/abi/x86/simd.rs','engine/tests/cpu/simd/cases/native/simd_semantics.rs','engine/tests/cpu/simd/cases/native/simd_extensions.rs','engine/tests/cpu/simd/cases/native/simd_sse3.rs','engine/tests/cpu/simd/cases/native/simd_sse4.rs','engine/tests/cpu/simd/cases/native/simd_sse4_moves.rs','engine/tests/cpu/simd/cases/native/simd_sse4_arithmetic.rs', 'engine/tests/cpu/simd/native.rs', 'engine/tests/cpu/simd/native.rs','engine/tests/cpu/simd/cases/wasm/cpu_simd_wasm.rs', 'engine/tests/cpu/simd/wasm.rs', 'engine/tests/support/wasm.rs', 'engine/tests/cpu/simd/wasm.rs', 'engine/tests/support/wasm.rs','engine/tests/cpu/simd/fixtures/p2-simd/runtime/run.mjs'];
cases.push(...packedStringCases);
cases.push(...cryptoCases);
sourcePaths.push('engine/Cargo.toml','Cargo.lock','engine/src/math/aes.rs','engine/src/cpu/x86/simd/crypto.rs','engine/src/cpu/dbt/wasm/locals.rs','engine/tests/cpu/simd/cases/native/simd_crypto.rs','engine/tests/cpu/simd/fixtures/p2-simd/runtime/crypto.mjs');
sourcePaths.push('engine/src/cpu/x86/packed_strings.rs','engine/src/cpu/x86/decode/packed_strings.rs','engine/src/cpu/dbt/wasm/packed_strings.rs','engine/tests/cpu/simd/cases/native/packed_strings.rs','engine/tests/cpu/simd/fixtures/p2-simd/runtime/packed-strings.mjs','engine/src/cpu/x86/ir.rs','engine/src/cpu/x86/decode/lower.rs','engine/src/cpu/x86/decode/profile.rs','engine/src/cpu/dbt/region.rs','engine/src/cpu/dbt/wasm/emitter.rs','engine/src/cpu/dbt/wasm/integer/mod.rs');
const sourceHashes=()=>Object.fromEntries(sourcePaths.map(p=>[p,hash(readFileSync(join(root,p)))])),beforePins=sourceHashes();
const observations=[],helpers=[],modules=[],contexts=[],raw_protocol=[];let count=0;
const arena=c=>Buffer.from(new Uint8Array(c.memory.buffer,c.base,SIZE));
const input=(c,at,b)=>new Uint8Array(c.memory.buffer,c.base+at,b.length).set(b);
const host=(c,name,args)=>assert.equal(c.api[name](...args),0,name);
const upload=(c,address,b)=>{input(c,T,b);host(c,'upload',[address,b.length]);};
function open(owner,entries,batch){
 const instance=new WebAssembly.Instance(engine.module,{}),c={owner,entries,memory:instance.exports.memory,api:{},id:++count};
 for(const[name,fn]of Object.entries(instance.exports))if(name.startsWith('ring3_abi_v1_')&&typeof fn==='function')c.api[name.slice(13)]=fn;
 host(c,'open',[8,c.id,0x53494d44]);c.base=c.api.arena_ptr()>>>0;input(c,0,initial);
 for(const[page,bits]of[[0x1000,7],[0x5000,3],[0x7000,3]])host(c,'map',[page,1,bits]);
 const blocks=batch.map((row,i)=>({pc:0x1000+i*32,bytes:Buffer.concat([hex(row.code),hex('ebfe')])}));for(const b of blocks)upload(c,b.pc,b.bytes);
 host(c,'protect',[0x1000,1,5]);input(c,T,dwords(blocks.flatMap(b=>entries?[b.pc]:[b.pc,b.bytes.length])));
 const compile=owner==='resident'?(entries?'compile_resident_entries':'compile_resident'):(entries?'compile_entries':'compile');
 const compiled=c.api[compile](...(entries?[blocks.length,0]:[blocks.length]));
 assert.equal(compiled,0,`${compile} ${batch.map(row=>`${row.name}:${row.code}`).join(', ')}`);
 const a=arena(c),binding=owner==='resident'?{lo:a.readUInt32LE(T+8),hi:a.readUInt32LE(T+12),pointer:a.readUInt32LE(T+16),length:a.readUInt32LE(T+20)}:{pointer:c.api.module_ptr()>>>0,length:c.api.module_len()>>>0};
 const bytes=Buffer.from(new Uint8Array(c.memory.buffer,binding.pointer,binding.length)),module=new WebAssembly.Module(bytes),ring3={};
 for(const im of WebAssembly.Module.imports(module)){if(im.kind!=='function')continue;ring3[im.name]=(...args)=>{
  if(c.raw&&im.name.startsWith('guard'))return 0;
  let result;
  if(c.raw&&(im.name==='simd_execute'||im.name==='simd_execute_resident')){
   const p=packet(0);p.writeUInt32LE(c.raw.value??0,20);input(c,100,p);input(c,T+512,dwords([c.raw.gpr??0xdeadbeef,c.raw.flags]));result=0;
  }else result=c.api[im.name](...args);
  if(!im.name.startsWith('guard'))helpers.push({context:c.id,name:im.name,args:args.map(v=>v>>>0),status:result,packet:arena(c).subarray(100,140).toString('hex'),...(c.raw?{nonforwarded:im.name==='simd_execute'||im.name==='simd_execute_resident'}:{})});return result;
 };}
 c.run=new WebAssembly.Instance(module,{env:{memory:c.memory},ring3}).exports.run;
 writeFileSync(join(output,`module-${c.id}.wasm`),bytes);modules.push({context:c.id,owner,entries,sha256:hash(bytes),bytes:bytes.length,imports:WebAssembly.Module.imports(module)});contexts.push({context:c.id,owner,entries});return[c,blocks];
}
function test(c,block,row){
 input(c,0,initial);const r=parents();r[0]=row.address??DATA;r[7]=row.address??DATA;if(row.registers)for(const[index,value]of Object.entries(row.registers))r[Number(index)]=value>>>0;input(c,0,state(r,block.pc,0xcd7));input(c,56,exit(1,0));
 input(c,SV+16,dwords([row.mxcsr??0x1f80]));
 if(row.pendingX87){input(c,FP+16,hex('7e030100'));}
 if(row.nullDs){input(c,SG+22,hex('0000'));}
 if(row.mmx){input(c,FP+40,(row.a??zero).subarray(0,8));input(c,FP+50,(row.b??zero).subarray(0,8));}else{input(c,SV+32,row.a??zero);input(c,SV+48,row.b??zero);}
 if(row.initialXmm)input(c,SV+32,row.initialXmm);
 upload(c,0x5000,fill(0x66));upload(c,DATA,fill(0x66));upload(c,DATA+16,fill(0x66));if(row.memory)upload(c,row.address??DATA,row.memory);
 if(row.seedMemory)upload(c,row.address??DATA,row.seedMemory);
 if(row.writeOnly)host(c,'protect',[0x5000,1,2]);
 if(row.stagedTransfer)input(c,T,row.stagedTransfer);
 const expected=arena(c),reason=row.reason??1,retired=row.retired??1;
 if(row.gpr!==undefined)r[0]=row.gpr;
 if(row.afterRegisters)for(const[index,value]of Object.entries(row.afterRegisters))r[Number(index)]=value>>>0;
 expected.set(state(r,retired?block.pc+hex(row.code).length:block.pc,row.flags??0xcd7));expected.set(exit(reason,retired,row.detail??0,row.fault??0,row.access??0,row.length??0),56);
 if(row.out){if(row.mmx){expected.set(row.out.subarray(0,8),FP+40);expected.fill(255,FP+48,FP+50);}else expected.set(row.out,SV+32);}
 if(row.mmx&&retired){expected.writeUInt16LE(0,FP+20);expected.writeUInt16LE(expected.readUInt16LE(FP+18)&~0x3800,FP+18);}
 if(row.empty)expected.writeUInt16LE(0xffff,FP+20);
 if(row.afterX87)expected.set(row.afterX87,FP);
 if(row.afterXmm)expected.set(row.afterXmm,SV+32);
 if(row.pendingX87&&reason===12)expected.writeUInt16LE(0x8081,FP+18);
 expected.writeUInt32LE(row.afterMxcsr??row.mxcsr??0x1f80,SV+16);
 const h=helpers.length,status=c.run(c.base,c.base+56,1,c.base+96);assert.equal(status,0,row.name);
 const events=helpers.slice(h),last=events.at(-1),directRegister=row.direct&&!row.memory&&!row.write&&reason===1;
 if(row.expectedHelpers!==undefined){assert.equal(events.length,row.expectedHelpers,row.name+' exact helper count');if(row.expectedHelpers===1&&row.expectedExecutionHelpers!==1)assert.equal(events[0].name,'cpu_read',row.name+' only the checked memory read crosses the host');}
 if(row.expectedExecutionHelpers!==undefined)assert.equal(events.filter(event=>event.name==='simd_execute'||event.name==='simd_execute_resident').length,row.expectedExecutionHelpers,row.name+' exact numeric helper count');
 if(directRegister||reason===12)assert.equal(events.length,0,row.name+' has no host execution helper');else assert.ok(last,row.name+' helper');
 // Scratch bytes are validated from the ordered canonical calls below; only those spans may differ.
 const actual=arena(c);
 if(row.stagedTransfer){
  assert.equal(events.length,1,row.name+' only the failed read crosses the host');
  assert.equal(events[0].name,'cpu_read');
  assert.deepEqual(events[0].args,[row.address,row.stagedTransfer.length,row.readAlignment??16]);
  assert.equal(events[0].packet,packet(row.stagedTransfer.length,reason===11?3:1,row.detail,row.fault,row.access).toString('hex'));
  assert.deepEqual(actual.subarray(T,T+row.stagedTransfer.length),row.stagedTransfer,row.name+' failed read preserves every staged transfer byte');
 }
 expected.set(actual.subarray(100,140),100);expected.set(actual.subarray(T,T+512),T);expected.set(actual.subarray(T+512,T+520),T+512);
 assert.deepEqual(actual,expected,row.name+' complete arena');
 if(reason===1&&last){assert.equal(last.packet,packet(row.write?row.writeLength??16:row.masked!==undefined?16:row.direct&&row.memory?row.memory.length:0).toString('hex'));if(!row.direct)assert.equal(actual.readUInt32LE(T+516),row.flags??0xcd7);}
 else if(last){const p=hex(last.packet);assert.equal(p.readUInt32LE(16),row.compiledGp?0:reason===13?5:reason===11?3:1);assert.equal(p.readUInt32LE(24),row.detail??0);}
 if(row.masked!==undefined){assert.equal(events.at(-1).args.at(-1),row.masked);assert.equal(actual.readUInt32LE(T+512),row.masked);}
 if(row.memory&&reason!==11){assert.equal(events[0].name,'cpu_read');if(row.readAlignment!==undefined)assert.deepEqual(events[0].args,[row.address,row.memory.length,row.readAlignment]);assert.deepEqual(actual.subarray(T,T+row.memory.length),row.memory);}
 if(row.writeOnly)host(c,'protect',[0x5000,1,3]);
 if(row.write||row.masked){const wanted=row.write??Buffer.from(fill(0x66).map((v,i)=>row.masked&(1<<i)?row.a[i]:v)),address=row.address??DATA;for(let i=0;i<wanted.length;i++){host(c,'read8',[address+i]);const p=arena(c);assert.equal(p.readUInt32LE(120),wanted[i],row.name+' memory byte'+i);}}
 if(row.unchangedMemory){for(let i=0;i<row.unchangedMemory.length;i++){host(c,'read8',[(row.address??DATA)+i]);assert.equal(arena(c).readUInt32LE(120),row.unchangedMemory[i],row.name+' preserved memory byte'+i);}}
 observations.push({context:c.id,name:row.name,status,retired,reason,cpu:actual.subarray(0,56).toString('hex'),exit:actual.subarray(56,96).toString('hex'),simd:actual.subarray(SV,SV+160).toString('hex'),x87:actual.subarray(FP,FP+128).toString('hex'),helper_calls:events.length});
}
for(const owner of['replacement','resident'])for(const entries of[false,true])for(let start=0;start<cases.length;start+=8){const batch=cases.slice(start,start+8),[c,blocks]=open(owner,entries,batch);for(let i=0;i<batch.length;i++)test(c,blocks[i],batch[i]);writeFileSync(join(output,`arena-${c.id}.bin`),arena(c));host(c,'close',[]);}
// Raw execution payloads are nonforwarded, with a permissive owner guard. They
// independently challenge publication after a well-formed V7 success packet.
const rawCases=[
 {name:'raw-reserved-bit-before-gpr',code:'f30f2cc1',flags:0xcd5},
 {name:'raw-preserved-df-before-memory-write',code:'0f1300',flags:0x8d7},
 {name:'raw-high-reserved-before-gpr',code:'f30f2cc1',flags:0x1cd7},
 {name:'raw-nonzero-helper-value-before-gpr',code:'f30f2cc1',flags:0xcd7,value:1},
 {name:'raw-invalid-compare-relation',code:'0f2ec1',flags:0x443},
 {name:'raw-valid-compare-flags',code:'0f2ec1',flags:0x403,valid:true},
];
for(const owner of['replacement','resident'])for(const entries of[false,true]){
 const[c,blocks]=open(owner,entries,rawCases);
 for(let i=0;i<rawCases.length;i++){
  const row=rawCases[i],block=blocks[i],r=parents();r[0]=DATA;
  input(c,0,initial);input(c,0,state(r,block.pc,0xcd7));input(c,56,exit(1,0));
  input(c,SV+32,dwords([0x3f800000,1,2,3]));input(c,SV+48,dwords([0x40000000,4,5,6]));
  upload(c,DATA,fill(0x66));const expected=arena(c),retired=row.valid?1:0,reason=row.valid?1:7;
  expected.set(state(r,block.pc+(row.valid?hex(row.code).length:0),row.valid?row.flags:0xcd7));expected.set(exit(reason,retired,row.valid?0:2),56);
  const raw=packet(0);raw.writeUInt32LE(row.value??0,20);expected.set(raw,100);expected.set(dwords([0xdeadbeef,row.flags]),T+512);
  const h=helpers.length;c.raw=row;const status=c.run(c.base,c.base+56,1,c.base+96);delete c.raw;
  assert.equal(status,0,row.name);const events=helpers.slice(h);assert.equal(events.length,1,row.name+' no read/write or extra helper');assert.equal(events[0].nonforwarded,true);
  assert.deepEqual(arena(c),expected,row.name+' complete neutral publication');
  const actual=arena(c);for(let offset=0;offset<16;offset++){host(c,'read8',[DATA+offset]);assert.equal(arena(c).readUInt32LE(120),0x66,row.name+' preserved RAM');}
  raw_protocol.push({context:c.id,owner,entries,name:row.name,status,reason,retired,cpu:actual.subarray(0,56).toString('hex'),exit:actual.subarray(56,96).toString('hex'),arena_hex:actual.toString('hex'),helper_calls:events.length});
 }
 writeFileSync(join(output,`arena-${c.id}.bin`),arena(c));host(c,'close',[]);
}
assert.deepEqual(sourceHashes(),beforePins);const result={status:'PASS',engine:{path:enginePath,sha256:engine.sha256,bytes:engine.bytes.length},arena_bytes:SIZE,counts:{contexts:contexts.length,modules:modules.length,runs:observations.length,raw_protocol_runs:raw_protocol.length,helpers:helpers.length},contexts,modules,observations,raw_protocol,helpers,source_sha256:beforePins,scope:'Authored instruction probes under four bound profiles; exact CPU/SIMD/x87 arena except explicitly observed scratch transfer overlay, literal semantic outputs and canonical packet checks. No original compiler/performance claim.'};
writeFileSync(join(output,'result.json'),JSON.stringify(result,null,2));console.log(JSON.stringify({status:'PASS',...result.counts,output}));
