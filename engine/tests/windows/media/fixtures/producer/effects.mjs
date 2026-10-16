export function effects(b) {
  const {allocate,data,ref,external}=b;
  for (const name of ['fxBuffer','fxObject','fxResult','fxReadback']) allocate(name, [0,0]);
  const format=data('fxFormat',[0x10001,48000,48000,0x80001,0]);
  const descriptor=data('fxBufferDesc',[36,0x10200,7200,0,format,0,0,0,0]);
  const classId=data('fxGargleClass',[0xdafd8210,0x4b915711,0x5bf7e39f,0xbf79e27a]);
  const interfaceId=data('fxGargleIid',[0xd616f352,0x11ced622,0x2000c5aa,0xa3990baf]);
  const description=data('fxDescription',[32,2,0xdafd8210,0x4b915711,0x5bf7e39f,0xbf79e27a,0,0]);
  const parameters=data('fxParameters',[100,1]);
  external('CoInitializeEx',0xd0002,[0,0]);
  b.com('CreateEffectBuffer','sound',0x70200,3,[descriptor,b.addresses.fxBuffer,0]);
  b.com('InstallGargle','fxBuffer',0x70400,21,[1,description,b.addresses.fxResult]);
  b.com('GetGargleObject','fxBuffer',0x70400,23,[classId,0,interfaceId,b.addresses.fxObject]);
  b.com('SetGargleParameters','fxObject',0x71200,3,[parameters]);
  b.com('GetGargleParameters','fxObject',0x71200,4,[b.addresses.fxReadback]);
  b.com('LockEffectBuffer','fxBuffer',0x70400,11,[0,4,b.addresses.pointer1,b.addresses.length1,b.addresses.pointer2,b.addresses.length2,0]);
  b.writeLocked(b.addresses.pointer1,[0xc0c0c0c0]);
  b.com('UnlockEffectBuffer','fxBuffer',0x70400,19,[ref('pointer1'),ref('length1'),ref('pointer2'),ref('length2')]);
  b.com('PlayGargle','fxBuffer',0x70400,12,[0,0,1],0,'fx-pcm');
  b.com('StopGargle','fxBuffer',0x70400,18);
  b.com('RemoveEffects','fxBuffer',0x70400,21,[0,0,0]);
  b.com('DetachedGargleParameters','fxObject',0x71200,4,[b.addresses.fxReadback]);
  b.com('ReleaseGargle','fxObject',0x71200,2);
  b.com('ReleaseEffectBuffer','fxBuffer',0x70400,2);
  external('CoUninitialize',0xd0003,[]);
}
