const word = value => {const bytes=Buffer.alloc(4);bytes.writeUInt32LE(value);return [...bytes];};
export function captureEffects(b) {
  const {allocate,data,ref,external}=b;
  for(const name of ['duplexFx','aecCapture','aecRender','aecObject','aecMode','aecStatus','aecFirst']) allocate(name,[0,0,0]);
  const format=data('aecFormat',[0x10001,48000,96000,0x100002,0]);
  const aecClass=data('aecClass',[0xbf963d80,0x11d0c559,0xa0002b8a,0xc15a25c9]);
  const aecIid=data('aecIid',[0xad74143d,0x4ab7903d,0xd3286680,0x656d0363]);
  const effects=data('aecEffects',[
    48,2,0xbf963d80,0x11d0c559,0xa0002b8a,0xc15a25c9,0xcdebb919,0x488a379a,0x3cf56587,0x40de36fd,0,0,
    48,2,0xe07f903f,0x4e6062fd,0xa7dedd8c,0xb5656623,0x11c5c73b,0x4ba166e9,0x14e8baa0,0x2dd9eec6,0,0]);
  const capture=data('aecCaptureDesc',[28,0x200,960,0,format,2,effects]);
  const render=data('aecRenderDesc',[36,0,960,0,format,0,0,0,0]);
  external('DirectSoundFullDuplexCreate',0x6000a,[0,0,capture,render,ref('window'),1,b.addresses.duplexFx,b.addresses.aecCapture,b.addresses.aecRender,0]);
  b.com('GetAEC','aecCapture',0x70a00,12,[aecClass,0,aecIid,b.addresses.aecObject]);
  b.com('InitialAECMode','aecObject',0x71600,4,[b.addresses.aecMode]);
  b.com('LockAECReference','aecRender',0x70400,11,[0,960,b.addresses.pointer1,b.addresses.length1,b.addresses.pointer2,b.addresses.length2,0]);
  b.emit(0xfc,0x8b,0x3d,...word(b.addresses.pointer1),0xb8,...word(0x10001000),0xb9,...word(240),0xf3,0xab);
  b.com('UnlockAECReference','aecRender',0x70400,19,[ref('pointer1'),ref('length1'),ref('pointer2'),ref('length2')]);
  b.com('PlayAECReference','aecRender',0x70400,12,[0,0,1]);
  b.com('StartAEC','aecCapture',0x70a00,9,[1],0,'aec-benchmark');
  b.com('ProcessedAECMode','aecObject',0x71600,4,[b.addresses.aecMode]);
  b.com('AECStatus','aecObject',0x71600,5,[b.addresses.aecStatus]);
  b.com('LockProcessedAEC','aecCapture',0x70a00,8,[0,4,b.addresses.pointer1,b.addresses.length1,b.addresses.pointer2,b.addresses.length2,0]);
  b.readLocked(b.addresses.pointer1,b.addresses.aecFirst);
  b.com('UnlockProcessedAEC','aecCapture',0x70a00,11,[ref('pointer1'),ref('length1'),ref('pointer2'),ref('length2')]);
  b.com('StopAEC','aecCapture',0x70a00,10);
  b.com('StopAECReference','aecRender',0x70400,18);
  b.com('ResetAEC','aecObject',0x71600,6);
  b.com('ResetAECStatus','aecObject',0x71600,5,[b.addresses.aecStatus]);
  b.com('ReleaseAEC','aecObject',0x71600,2);
  b.com('ReleaseAECCapture','aecCapture',0x70a00,2);
  b.com('ReleaseAECRender','aecRender',0x70400,2);
  b.com('ReleaseAECDuplex','duplexFx',0x70b00,2);
}
