import {builder} from '../../../../support/pe32/image.mjs';
const word=value=>{const result=Buffer.alloc(4);result.writeUInt32LE(value>>>0);return [...result];};
export function image() {
  const ids={AddVectoredExceptionHandler:0x120001,RemoveVectoredExceptionHandler:0x120002,
    AddVectoredContinueHandler:0x120003,RemoveVectoredContinueHandler:0x120004,
    RaiseException:0x120007,RtlCaptureContext:0x120009,RtlRaiseException:0x12000c,ExitProcess:0x10003};
  const b=builder([['kernel32.dll',Object.keys(ids).filter(name=>!name.startsWith('Rtl'))],
    ['ntdll.dll',['RtlCaptureContext','RtlRaiseException']]],{callbacks:[{words:1,result:0xffffffff,record:'veh'},{words:1,result:0xffffffff,record:'vch'}]});
  for(const name of ['veh','vch','handled','lastCode','divideResult'])b.allocate(name);
  b.allocate('context',Array(179).fill(0));
  const record=b.data('record',[0xe0001234,0,0,0,0,...Array(15).fill(0)]);
  const a=b.addresses;
  const call=(name,args,expected=0,hook,output)=>b.external(name,ids[name],args,expected,output,hook);
  call('AddVectoredExceptionHandler',[1,b.callbacks[0].entry],'handle',undefined,'veh');
  call('AddVectoredContinueHandler',[1,b.callbacks[1].entry],'handle',undefined,'vch');
  call('RaiseException',[0xe0004321,0,0,0],42,'raised');
  call('RtlRaiseException',[record],42,'rtl-raised');
  // Real guest divide fault, with the handler advancing the fault CONTEXT.Eip by two bytes.
  b.emit(0x31,0xd2,0xb8,...word(1),0x31,0xc9,0xf7,0xf1);b.save(a.divideResult);
  call('RtlCaptureContext',[a.context],42,'captured');
  call('RemoveVectoredContinueHandler',[b.ref('vch')],1);
  call('RemoveVectoredExceptionHandler',[b.ref('veh')],1);
  call('ExitProcess',[0],'exit');
  const guest=b.seal();
  const body=[
    0xff,0x05,...word(a.vehCount),
    0x8b,0x44,0x24,4, // EXCEPTION_POINTERS
    0x8b,0x50,4,      // CONTEXT
    0x8b,0x08,0x8b,0x09,0x89,0x0d,...word(a.lastCode),
    0x81,0xf9,...word(0xc0000094),0x75,7,
    0x83,0x82,...word(184),2,
    0xc7,0x82,...word(176),...word(42),
    0xb8,...word(0xffffffff),0xc2,4,0,
  ];
  guest.bytes.set(body,0x200+(b.callbacks[0].entry-guest.base-0x1000));
  guest.callbacks[0].length=body.length;
  return guest;
}
