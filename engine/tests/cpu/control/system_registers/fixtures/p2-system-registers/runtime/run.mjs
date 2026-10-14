import assert from 'node:assert/strict';
import {mkdirSync} from 'node:fs';
import {harness, PC, KEY_HIGH, SEG, BASES, record, registers, state, setSegments, exit, words, writeRAM} from './support.mjs';

const [enginePath, output, root] = process.argv.slice(2);
assert.ok(enginePath && output && root, 'explicit engine, fresh output, repository root');
mkdirSync(output, {recursive: true});
const h = await harness(enginePath, output, root);
// These are authored literals from the frozen query-bank contract, never decoded from production output.
const FULL = Buffer.from('5233535901000100240000001f0000007856341298badcfe452356344300530033000000', 'hex');
const GDT = '452378563412', IDT = '563498badcfe';
function bank(mask, {gdtrBase = 0, gdtrLimit = 0, idtrBase = 0, idtrLimit = 0, ldtr = 0, tr = 0, msw = 0} = {}) {
  const b = record('R3SY', 36); b.writeUInt32LE(mask, 12); b.writeUInt32LE(gdtrBase, 16); b.writeUInt32LE(idtrBase, 20);
  b.writeUInt16LE(gdtrLimit, 24); b.writeUInt16LE(idtrLimit, 26); b.writeUInt16LE(ldtr, 28); b.writeUInt16LE(tr, 30); b.writeUInt16LE(msw, 32); return b;
}
const CASES = [
  {id:'sldt32', hex:'0f00c0', eax:0x43},
  {id:'sldt16', hex:'660f00c0', eax:0xa1b20043},
  {id:'str32', hex:'0f00c8', eax:0x53},
  {id:'str16', hex:'660f00c8', eax:0xa1b20053},
  {id:'smsw32', hex:'0f01e0', eax:0xa1b20033},
  {id:'smsw16', hex:'660f01e0', eax:0xa1b20033},
  {id:'sgdt', hex:'0f0100', address:0x5000, bytes:GDT},
  {id:'sgdt66', hex:'660f0100', address:0x5000, bytes:GDT},
  {id:'sidt', hex:'0f0108', address:0x5000, bytes:IDT},
  {id:'sidt66', hex:'660f0108', address:0x5000, bytes:IDT},
  {id:'sldt-m32', hex:'0f0000', address:0x5000, bytes:'4300'},
  {id:'sldt-m16', hex:'660f0000', address:0x5000, bytes:'4300'},
  {id:'str-m32', hex:'0f0008', address:0x5000, bytes:'5300'},
  {id:'smsw-m32', hex:'0f0120', address:0x5000, bytes:'3300'},
  {id:'d16-b16-sgdt-address-default', hex:'0f0100', mode:'D16/B16', regs:{3:0xaaaa5000,6:0xbbbb0000}, address:0x5000, bytes:GDT},
  {id:'d16-b32-str32-via66', hex:'660f00c8', mode:'D16/B32', eax:0x53},
  {id:'d16-b16-smsw-address32-via67', hex:'670f0120', mode:'D16/B16', address:0x5000, bytes:'3300'},
  {id:'d32-b16-sgdt-address16-SS', hex:'670f0102', mode:'D32/B16', ssBase:0x2000, regs:{5:0xaaaa3000,6:0xbbbb0000}, address:0x5000, bytes:GDT},
  {id:'explicit-zero-GDTR-not-missing', hex:'0f0100', address:0x5000, bytes:'000000000000', bank:bank(1), sparse:true},
  {id:'explicit-null-LDTR-not-missing', hex:'0f00c0', eax:0, bank:bank(4), sparse:true},
  {id:'sgdt-late-readonly', hex:'0f0100', address:0x5ffd, bytes:GDT, fault:true, capturedPages:true},
  {id:'sidt-late-readonly', hex:'0f0108', address:0x5ffd, bytes:IDT, fault:true, capturedPages:true},
  {id:'sgdt-writeonly', hex:'0f0100', address:0x5000, bytes:GDT, writeOnly:true},
  {id:'sgdt-null-DS', hex:'0f0100', initialEax:0xffffffff, segmentFault:11, nullDS:true},
  // R3EX assigns #NP=14 and #SS=15. The frozen prose's named #SS contract controls this literal correction.
  {id:'sgdt-SS-wholespan-limit', hex:'0f014500', regs:{5:0x5000}, segmentFault:15, ssLimit:0x5003},
  {id:'sldt-word-late-readonly', hex:'0f0000', address:0x5fff, bytes:'4300', fault:true},
  {id:'sgdt-unaligned-AC-with-AM0', hex:'0f0100', address:0x5001, bytes:GDT, flags:0x40002, version:2},
  {id:'sgdt-self-modifying', hex:'0f0100', address:0x1000, bytes:GDT, smc:true},
  {id:'smsw-RF-TF', hex:'0f01e0', eax:0xa1b20033, flags:0x10102, version:3, expectedFlags:0x102, debug:true},
  {id:'sgdt-preinstruction-budget-RF', hex:'0f0100', address:0x5000, bytes:GDT, flags:0x10002, version:3, shadow:1, budget:0, preinstruction:true},
  {id:'sgdt-preinstruction-cancel-RF', hex:'0f0100', address:0x5000, bytes:GDT, flags:0x10002, version:3, shadow:1, cancel:1, preinstruction:true},
];
assert.equal(CASES.length, 31, 'frozen finite family inventory');
const DEFAULTS = [
  {id:'gdtr',hex:'0f0100',address:0x5000,bytes:'ff070000ffff'},
  {id:'idtr',hex:'0f0108',address:0x5000,bytes:'ff070010ffff'},
  {id:'ldtr',hex:'0f00c0',eax:0},
  {id:'tr',hex:'0f00c8',eax:0x28},
  {id:'msw',hex:'0f01e0',eax:0xa1b20033},
];
const suffix = (c, name) => c.resident ? ({cpu_write:'cpu_write_resident',cpu_store16:'cpu_store_resident16'}[name]) : name;
const binding = (c, material) => c.resident ? material.binding : [];
function helper(c, material, address, bytes, status = 0) {
  const six = bytes.length === 6, name = suffix(c, six ? 'cpu_write' : 'cpu_store16');
  return {name, args:[...binding(c, material), address, ...(six ? [6,1] : [bytes.readUInt16LE(0)])], status};
}
function packet(want, width, fault = false) {
  want.set(record('R3MH', 40, fault ? [9,0,7,0x6000,2,width] : [0,0,0,0,0,width], fault ? 15 : width === 6 ? 7 : 10), 100);
}
function result(input, spec, instructionLength, {pc = PC, fault = spec.fault, gates = false} = {}) {
  const want = Buffer.from(input), six = spec.bytes?.length === 12;
  if (!spec.preinstruction) {want.writeUInt32LE(0,12); want.writeUInt32LE((spec.expectedFlags ?? (want.readUInt32LE(52) & ~0x10000)) >>> 0,52);}
  if (!fault && !spec.segmentFault && !spec.preinstruction) {
    want.writeUInt32LE(pc + instructionLength,48);
    if (spec.eax !== undefined) want.writeUInt32LE(spec.eax,16);
  }
  const version = six || spec.segmentFault ? 6 : spec.bytes ? 6 : gates ? 3 : 1;
  if (spec.segmentFault) exit(want,spec.segmentFault,0,6);
  else if (fault) exit(want,23,0,12,[7,0x6000,2,spec.bytes.length/2]);
  else if (spec.debug) exit(want,16,1,7);
  else if (spec.preinstruction) exit(want,spec.cancel ? 2 : 1,0,version);
  else exit(want,spec.smc ? 6 : 1,1,version);
  if (spec.bytes && !spec.preinstruction && !spec.segmentFault) {
    if (six) want.set(Buffer.from(spec.bytes,'hex'),140);
    packet(want,spec.bytes.length/2,fault);
  }
  return want;
}
function seed(c, spec, pc = PC) {
  const regs = [...registers];
  if (spec.initialEax !== undefined) regs[0] = spec.initialEax;
  else if (spec.address !== undefined && !spec.regs) regs[0] = spec.address;
  for (const [index, value] of Object.entries(spec.regs ?? {})) regs[Number(index)] = value;
  return state(h.arena(c), {pc, regs, mode:spec.mode, ssBase:spec.ssBase, flags:spec.flags, version:spec.version, shadow:spec.shadow, cancel:spec.cancel});
}
function setup(resident, entries, label, pages = [[0x1000],[0x5000]], image = FULL) {
  const c = h.open(resident,entries,label,pages);
  if (image) h.configure(c,label+': strict supplied bank',image);
  return c;
}
function uploadCode(c, pc, hex, sentinel = false) {
  const bytes = Buffer.concat([Buffer.from(hex,'hex'),Buffer.from('ebfe','hex'),...(sentinel ? [Buffer.from([0xa5])] : [])]);
  h.upload(c,pc,bytes); return bytes.length - Number(sentinel);
}
function strict(c, label, pc, length, helpers = [], status = 0, extra = []) {
  return h.compile(c,label,[[pc,length],...extra],helpers,{expectedStatus:status});
}
function bootstrap(c, target, label) {
  h.upload(c,PC,Buffer.concat([Buffer.from([0xe9]),words([(target-(PC+5))>>>0])]));
  h.put(c,0,state(h.arena(c)));
  const m = strict(c,label+': terminal starter',PC,5);
  if (!m) {h.skip(c,label+': real NeedCode','starter compile'); return false;}
  const input = state(h.arena(c)), want = exit(Buffer.from(input),3,1,1); want.writeUInt32LE(target,48);
  return h.observe(c,label+': real branch reaches NeedCode',{input,want,budget:2}).passed;
}
let queryInvocations = 0;
try {
for (const resident of [false,true]) for (const entries of [false,true]) {
  for (const spec of CASES) {
    const pages = [[0x1000],[0x5000,spec.writeOnly ? 2 : 7],...(spec.fault ? [[0x6000,5]] : [])];
    const c = setup(resident,entries,spec.id,pages,null);
    if (spec.sparse) h.request(c,spec.id+': sparse TI1 separate authority','configure_segment_descriptor_raw',[7,0xffff,0x0040f200]);
    h.configure(c,spec.id+': explicit bank',spec.bank ?? FULL);
    const length = uploadCode(c,PC,spec.hex,spec.smc);
    const extra = [];
    if (spec.capturedPages) {
      h.protect(c,0x6000,7);
      for (const pc of [0x5100,0x6100]) {uploadCode(c,pc,'90'); extra.push([pc,3]);}
      h.protect(c,0x6000,5);
    }
    h.put(c,0,seed(c,spec));
    const imports = spec.bytes ? [suffix(c,spec.bytes.length === 12 ? 'cpu_write' : 'cpu_store16')] : [];
    // Segment denial still has the generated write import but must never invoke it.
    if (spec.segmentFault) imports.push(suffix(c,'cpu_write'));
    const material = strict(c,spec.id+': bound query',PC,length,imports,0,extra);
    if (!material) {h.skip(c,spec.id+': literal query semantics','configured query compile'); continue;}
    let input = seed(c,spec);
    if (spec.nullDS) setSegments(input,{legacy:true,nullDS:true});
    if (spec.ssLimit !== undefined) {
      setSegments(input,{ssLimit:spec.ssLimit}); input.writeUInt16LE(2,SEG+4); input.fill(0,BASES,BASES+40);
    }
    const want = result(input,spec,length-2), wantRAM = h.literalRAM(c);
    if (spec.bytes && !spec.fault && !spec.preinstruction) writeRAM(wantRAM,spec.address,Buffer.from(spec.bytes,'hex'));
    const helpers = spec.bytes && !spec.preinstruction ? [helper(c,material,spec.address,Buffer.from(spec.bytes,'hex'),spec.fault ? 23 : spec.smc ? 11 : 0)] : [];
    queryInvocations++;
    h.observe(c,spec.id+': literal query semantics',{input,want,wantRAM,budget:spec.budget ?? 1,helpers});
    if (spec.capturedPages) {
      const before = h.arena(c), stillCurrent = exit(Buffer.from(before),1,0,6);
      h.observe(c,spec.id+': failed write preserves both captured Execute-page versions',{input:before,want:stillCurrent,budget:0});
      h.protect(c,0x6000,7);
      // Protect legitimately changes mapping identity: recompile before checking repaired write semantics.
      h.put(c,0,seed(c,spec));
      const repaired = strict(c,spec.id+': repaired permission material',PC,length,imports);
      if (repaired) {
        const retry = seed(c,spec), retryWant = result(retry,spec,length-2,{fault:false}), retryRAM = h.literalRAM(c);
        writeRAM(retryRAM,spec.address,Buffer.from(spec.bytes,'hex'));
        h.observe(c,spec.id+': repaired exact six-byte store',{input:retry,want:retryWant,wantRAM:retryRAM,helpers:[helper(c,repaired,spec.address,Buffer.from(spec.bytes,'hex'))]});
        // Repaired material captures only the query source, restoring the frozen normal-budget oracle.
      } else h.skip(c,spec.id+': repaired write','permission repair compile');
    }
    if (spec.smc) {const after = h.arena(c); h.observe(c,spec.id+': stale held material stays neutral',{input:after,want:after,status:4});}
    h.host(c,'close');
  }
  for (const partial of [false,true]) {
    const c=setup(resident,entries,partial?'partial bank defaults':'application defaults',[[0x1000],[0x5000]],partial?bank(4):null);
    for (const spec of DEFAULTS) {
      const length=uploadCode(c,PC,spec.hex);h.put(c,0,seed(c,spec));
      const helpers=spec.bytes?[suffix(c,'cpu_write')]:[];
      const material=strict(c,spec.id+': complete default query bank',PC,length,helpers);
      if (material) {
        const input=seed(c,spec),want=result(input,spec,length-2),wantRAM=h.literalRAM(c);
        if(spec.bytes)writeRAM(wantRAM,spec.address,Buffer.from(spec.bytes,'hex'));
        h.observe(c,spec.id+': exact application default observation',{input,want,wantRAM,
          helpers:spec.bytes?[helper(c,material,spec.address,Buffer.from(spec.bytes,'hex'))]:[]});
      } else h.skip(c,spec.id+': default query execution','default query compile');
    }
    h.host(c,'close');
  }
  {
    const c=setup(resident,entries,'stopped default query',[[0x1000],[0x2000],[0x5000]],null);uploadCode(c,0x2000,'0f0100');
    if(bootstrap(c,0x2000,'default-bank stopped')) {
      const material=h.compile(c,'stopped full query uses application defaults',[],[suffix(c,'cpu_write')],{stopped:true});
      if(material) {
        const spec=DEFAULTS[0],input=seed(c,spec,0x2000),want=result(input,spec,3,{pc:0x2000}),wantRAM=h.literalRAM(c);
        writeRAM(wantRAM,0x5000,Buffer.from(spec.bytes,'hex'));
        h.observe(c,'stopped default SGDT exact store',{input,want,wantRAM,helpers:[helper(c,material,0x5000,Buffer.from(spec.bytes,'hex'))]});
      }else h.skip(c,'stopped default query execution','stopped query compile');
    }
    h.host(c,'close');
  }
  // fetch must complete before lowering; failed prepares do not freeze startup configuration.
  for (const [name,prefix,modrm,bytes] of [['sgdt','0f01','00',GDT],['sldt','0f00','c0',null]]) {
    const c=setup(resident,entries,'truncated '+name,[[0x1000],[0x5000]],null);
    h.upload(c,0x1ffe,Buffer.from(prefix,'hex')); h.put(c,0,state(h.arena(c),{pc:0x1ffe,regs:[0x5000,...registers.slice(1)]}));
    strict(c,'default '+name+' full-fetch refusal before lowering',0x1ffe,5,[],10);
    h.host(c,'map',[0x2000,1,7]); c.pages.push(0x2000); c.pages.sort((a,b)=>a-b); c.permissions.set(0x2000,7);c.expectedRAM.set(0x2000,Buffer.alloc(4096,0xa5));
    for (let at=0;at<4096;at+=512) h.upload(c,0x2000+at,Buffer.alloc(512,0xa5));
    h.upload(c,0x2000,Buffer.from(modrm+'ebfe','hex'));
    h.configure(c,'failed compile permits explicit bank configuration',FULL);
    const material=strict(c,'full-fetch repair admits supplied '+name,0x1ffe,5,bytes?[suffix(c,'cpu_write')]:[]);
    if (material) {
      const input=state(h.arena(c),{pc:0x1ffe,regs:[0x5000,...registers.slice(1)]}), spec=bytes?{address:0x5000,bytes}:{eax:0x43};
      const want=result(input,spec,3,{pc:0x1ffe}), wantRAM=h.literalRAM(c);
      if(bytes) writeRAM(wantRAM,0x5000,Buffer.from(bytes,'hex'));
      h.observe(c,'supplied '+name+' succeeds only after full-fetch repair',{input,want,wantRAM,helpers:bytes?[helper(c,material,0x5000,Buffer.from(bytes,'hex'))]:[]});
    } else h.skip(c,'repaired supplied query execution','full-fetch repair compile');
    h.host(c,'close');
  }
  for (const hex of ['66660f0100','f30f00c0']) {
    const c=setup(resident,entries,'prefix policy '+hex),length=uploadCode(c,PC,hex); h.put(c,0,state(h.arena(c)));
    strict(c,'unchanged unsupported prefix policy '+hex,PC,length,[],10); h.host(c,'close');
  }
  for (const consumed of [0,2]) {
    const target=consumed ? 0x1ffe : 0x2000, c=setup(resident,entries,'current CS before default query prefix '+consumed,[[0x1000],[0x5000]],null);
    if(consumed) h.upload(c,target,Buffer.from('0f01','hex'));
    if(bootstrap(c,target,'CS-before-query prefix '+consumed)) {
      const input=Buffer.from(h.arena(c)); setSegments(input,{csLimit:0x1fff}); h.put(c,0,input);
      const m=h.compile(c,'stopped captures real denied prefix '+consumed,[],[],{stopped:true});
      if(m) {
        const gpInput=h.arena(c), gp=exit(Buffer.from(gpInput),11,0,6);
        h.observe(c,'CS denial precedes default query '+consumed,{input:gpInput,want:gp});
        h.host(c,'map',[0x2000,1,7]);c.pages.push(0x2000);c.pages.sort((a,b)=>a-b);c.permissions.set(0x2000,7);c.expectedRAM.set(0x2000,Buffer.alloc(4096,0xa5));
        for(let at=0;at<4096;at+=512)h.upload(c,0x2000+at,Buffer.alloc(512,0xa5));
        h.upload(c,0x2000,Buffer.from(consumed?'00ebfe':'0f0100ebfe','hex'));
        const repaired=Buffer.from(h.arena(c));setSegments(repaired,{csLimit:0x2fff});
        const miss=exit(Buffer.from(repaired),3,0,1);h.observe(c,'repaired synthetic block requests real code '+consumed,{input:repaired,want:miss});
        const material=h.compile(c,'repaired full query uses default readback '+consumed,[],[suffix(c,'cpu_write')],{stopped:true});
        if(material) {
          const spec=DEFAULTS[0],input=Buffer.from(h.arena(c));input.writeUInt32LE(0x5000,16);
          const want=result(input,spec,3,{pc:target}),wantRAM=h.literalRAM(c);writeRAM(wantRAM,0x5000,Buffer.from(spec.bytes,'hex'));
          h.observe(c,'repaired CS admits default query '+consumed,{input,want,wantRAM,helpers:[helper(c,material,0x5000,Buffer.from(spec.bytes,'hex'))]});
        }else h.skip(c,'repaired default query '+consumed,'repaired stopped compile');
      } else h.skip(c,'CS-before-bank execution','stopped synthetic capture');
    }
    h.host(c,'close');
  }
  {
    const c=setup(resident,entries,'illegal LOCK independent of missing bank',[[0x1000],[0x2000]],null);
    uploadCode(c,0x2000,'f00f00c0');
    if(bootstrap(c,0x2000,'illegal LOCK stopped')) {
      const material=h.compile(c,'execution policy admits real illegal LOCK fault',[],[],{stopped:true});
      if(material){const input=h.arena(c),want=exit(Buffer.from(input),20,0,7);h.observe(c,'illegal LOCK produces UD without bank or operands',{input,want});}
      else h.skip(c,'illegal LOCK fault execution','stopped execution-policy compile');
    }
    h.host(c,'close');
  }
  // invalid request records leave the prepublication readback configuration reusable.
  {
    const c=setup(resident,entries,'configuration codec',[[0x1000]],null);
    const invalid=[['magic',b=>b[0]^=1],['version',b=>b.writeUInt16LE(2,4)],['profile',b=>b.writeUInt16LE(2,6)],['size',b=>b.writeUInt32LE(40,8)],
      ['reserved34',b=>b.writeUInt16LE(1,34)],['mask-zero',b=>b.writeUInt32LE(0,12)],['unknown-presence',b=>b.writeUInt32LE(0x3f,12)],
      ['absent-payload',b=>b.writeUInt32LE(0x1e,12)],['LDTR-TI1',b=>b.writeUInt16LE(7,28)],['TR-TI1',b=>b.writeUInt16LE(7,30)],
      ...[0,0x30,0x35,0x39,0x21,0x11,0x71,0xffff].map(value=>['CR0-'+value.toString(16),b=>b.writeUInt16LE(value,32)])];
    for(const [label,mutate]of invalid){const b=Buffer.from(FULL);mutate(b);h.configure(c,'strict codec rejects '+label,b,7);}
    h.configure(c,'valid request after every neutral codec refusal',FULL);
    h.configure(c,'partial image can replace readback before executable publication',bank(4));
    h.configure(c,'complete image can replace the partial readback before publication',FULL);
    const length=uploadCode(c,PC,'0f00c0');h.put(c,0,state(h.arena(c)));const material=strict(c,'codec refusals did not replace LDTR43',PC,length);
    if(material){const input=state(h.arena(c));h.observe(c,'configured bank retained after rejected requests',{input,want:result(input,{eax:0x43},3)});}
    h.configure(c,'configuration frozen after executable material',FULL,7);
    // Equal-content source writes stale material; query values must not publish on held re-entry.
    h.upload(c,PC,Buffer.from('0f00c0ebfe','hex'));const before=state(h.arena(c));
    if(material)h.observe(c,'physical equal-write staleness is neutral before query',{input:before,want:before,status:4});
    h.host(c,'close');
  }
  {
    const c=setup(resident,entries,'material freezes application defaults',[[0x1000]],null),length=uploadCode(c,PC,'90');h.put(c,0,state(h.arena(c)));
    const material=strict(c,'default-bank NOP publishes executable material',PC,length);
    h.configure(c,'application defaults cannot change after material',FULL,7);
    if(resident && material){h.request(c,'discard all unacknowledged resident material','discard_unacknowledged_resident',material.binding);h.configure(c,'discard does not reopen configuration lifetime',FULL,7);}
    h.host(c,'close');c.pages=[];c.permissions.clear();c.expectedRAM.clear();
    h.configure(c,'closed export refuses before request parsing',Buffer.alloc(36),5);
  }
  // Real gate capture, pending-call priority, callback authorization, RF/shadow transport, and retained readback authority.
  {
    const c=setup(resident,entries,'call and callback bank lifecycle',[[0x1000],[0x2000],[0x8000]]);
    h.upload(c,PC,Buffer.from('0f0b','hex'));h.upload(c,0x1100,Buffer.from('0f0b','hex'));uploadCode(c,0x2000,'0f00c0');h.upload(c,0x8f00,words([0x2000]));
    const regs=[...registers];regs[4]=0x8f00;h.put(c,0,state(h.arena(c),{regs,flags:0x10002,version:3,shadow:1}));
    const outer=h.compile(c,'outer gate material',resident?[[PC,2]]:[[PC,2],[0x1100,2],[0x2000,5]],[],{gates:resident?[[PC,7]]:[[PC,7],[0x1100,8]]});
    let callback=outer;
    if(resident && outer) callback=h.compile(c,'distinct resident callback query material',[[0x1100,2],[0x2000,5]],[],{gates:[[0x1100,8]]});
    if(outer && callback){
      const input=state(h.arena(c),{regs,flags:0x10002,version:3,shadow:1}),gate=exit(Buffer.from(input),8,0,3,[7,0,0,0]);
      h.observe(c,'real outer gate preserves RF and SS shadow',{input,want:gate,material:outer});
      let want=Buffer.from(h.arena(c));want.set(record('R3CF',112,[1,7,1,0,PC,0x8f00,0x2000,0]),140);
      h.request(c,'capture actual outer gate','capture'+(resident?'_resident':'')+'_call',resident?[...outer.binding,1,0]:[c.key,KEY_HIGH,outer.generation,1,0],{want});
      h.configure(c,'pending Busy precedes invalid configuration bytes',Buffer.alloc(36),12);
      want=Buffer.from(h.arena(c));want.writeUInt32LE(0x8efc,32);want.writeUInt32LE(0x2000,48);exit(want,3,0,3);
      const callbackFields=[2,1,1,0,0x2000,0x8efc,0x1100,8,0,0,...(resident?[...outer.binding.slice(2),...callback.binding.slice(2)]:[outer.generation])];
      want.set(record(resident?'R3RC':'R3CB',resident?72:64,callbackFields),140);
      const wantRAM=h.literalRAM(c);writeRAM(wantRAM,0x8efc,words([0x1100]));
      h.request(c,'begin callback transports exact RF/shadow/current records',resident?'begin_resident_callback':'begin_callback',
        resident?[...outer.binding,...callback.binding.slice(2),1,0x2000,0x1100,8,0]:[c.key,KEY_HIGH,outer.generation,1,0x2000,0x1100,8,0],{want,wantRAM});
      h.configure(c,'callback Busy precedes malformed or frozen bank',Buffer.alloc(36),12);
      if(resident)h.request(c,'authorize distinct resident callback','authorize_resident_callback',[...callback.binding,2]);
      const callbackInput=h.arena(c),callbackWant=result(callbackInput,{eax:0x43},3,{pc:0x2000,gates:true});
      h.observe(c,'real callback query retains supplied LDTR and consumes RF/shadow',{input:callbackInput,want:callbackWant,material:callback});
      want=Buffer.from(h.arena(c));want.set(gate.subarray(0,96),0);for(const [start,end]of [[4236,4364],[4364,4524],[4524,4588],[4612,4756],[4756,4796]])want.set(gate.subarray(start,end),start);
      h.request(c,'abort restores exact captured outer CPU context','abort_callback',[c.key,KEY_HIGH,2],{want});
      h.configure(c,'restored pending outer still wins Busy',FULL,12);
      want=Buffer.from(h.arena(c));want.writeUInt32LE(42,16);want.writeUInt32LE(0x8f04,32);want.writeUInt32LE(0x2000,48);exit(want,3,0,3);
      h.request(c,'complete outer call preserves configured authority',resident?'complete_resident_call':'complete_call',resident?[...outer.binding,1,42]:[c.key,KEY_HIGH,outer.generation,1,42],{want});
      const completed=h.arena(c),completedWant=result(completed,{eax:0x43},3,{pc:0x2000,gates:true});
      h.observe(c,'query after lifecycle completion uses same bank',{input:completed,want:completedWant,material:callback});
      h.configure(c,'completed lifecycle does not reopen frozen bank',FULL,7);
    }else h.skip(c,'real call/callback authority lifecycle','gate material');
    h.host(c,'close');
  }
}
assert.equal(queryInvocations,124,'31 literal queries across exactly four endpoint profiles');
} catch(error) {h.failure('fixture setup or admission exception',error);}
h.finish([
  'Complete application defaults and partial overrides are readback observations, without IDT/TSS table walking.',
  'All original query widths, addressing forms, store effects and complete CPU/numeric context checks remain exercised.',
  'Application page faults preserve exact denied address, write intent, packet and exit metadata; failed stores preserve every mapped RAM byte.',
  'Real generated helpers, entry epochs and detector events are checked independently, including RF/TF, budget, cancellation and callback lifetime.',
  'Write-only diagnostics temporarily add READ and restore the permission; no unchanged-mapping-version claim is made.',
]);
