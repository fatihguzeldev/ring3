import {builder} from '../../../../support/pe32/image.mjs';
function data(b,name,bytes) {const padded=Buffer.alloc(Math.ceil(bytes.length/4)*4);bytes.copy(padded);return b.data(name,Array.from({length:padded.length/4},(_,i)=>padded.readUInt32LE(i*4)));}
const wide=(b,name,text)=>data(b,name,Buffer.from(text+'\0','utf16le'));
export function image(){
  const ids={GetACP:0x150001,GetOEMCP:0x150002,IsValidCodePage:0x150003,GetCPInfoExW:0x150006,MultiByteToWideChar:0x150007,WideCharToMultiByte:0x150008,LCMapStringW:0x15000a,CompareStringW:0x15000c,GetStringTypeW:0x15000f,GetLocaleInfoW:0x150013,GetSystemTime:0x15001d,SystemTimeToFileTime:0x15001f,FileTimeToSystemTime:0x150020,GetDateFormatW:0x15002a,GetTimeFormatW:0x15002c,GetCommandLineA:0xc0004,GetCommandLineW:0xc0005,GetEnvironmentVariableW:0xc0007,SetEnvironmentVariableW:0xc0009,ExpandEnvironmentStringsW:0xc000b,GetSystemTimeAsFileTime:0xc0014,ExitProcess:0x10003};
  const b=builder([['kernel32.dll',Object.keys(ids)]]);const call=(name,args,expected,hook,output)=>b.external(name,ids[name],args,expected,output,hook);
  for(const name of ['commandA','commandW','used','converted','clockLow'])b.allocate(name);
  b.allocate('buffer',Array(160).fill(0xcccccccc));b.allocate('calendar',Array(8).fill(0));b.allocate('filetime',Array(2).fill(0));
  const a=b.addresses;
  const utf8=data(b,'utf8',Buffer.from('é😀\0','utf8'));const invalid=data(b,'invalid',Buffer.from([0xff,0]));
  const substitute=wide(b,'substitute','Ā∞');const upper=wide(b,'upper','straße');const accented=wide(b,'accented','É');const plain=wide(b,'plain','e');
  const environment=wide(b,'environment','RING3_NLS');const value=wide(b,'value','€é');const expand=wide(b,'expand','[%RING3_NLS%]');
  const date=b.data('date',[0x000207e8,0x001d0000,0x0005000d,0x00000008]);
  call('GetACP',[],1252);call('GetOEMCP',[],437);call('IsValidCodePage',[65001],1);call('IsValidCodePage',[932],0);
  call('GetCPInfoExW',[65001,0,a.buffer],1,'cpinfo');
  call('MultiByteToWideChar',[65001,8,utf8,0xffffffff,0,0],4);
  call('MultiByteToWideChar',[65001,8,utf8,6,a.buffer,3],3,'utf16');
  call('WideCharToMultiByte',[65001,0x80,a.buffer,3,a.buffer+64,6,0,0],6,'utf8');
  call('MultiByteToWideChar',[65001,8,invalid,1,a.buffer,32],0,'invalid-utf8');
  call('WideCharToMultiByte',[1252,0,substitute,2,a.buffer,32,0,a.used],2,'bestfit');
  call('WideCharToMultiByte',[1252,0x400,substitute,2,a.buffer,32,0,a.used],2,'substitute');
  call('LCMapStringW',[0x409,0x200,upper,0xffffffff,a.buffer,32],8,'uppercase');
  call('CompareStringW',[0x409,3,accented,0xffffffff,plain,0xffffffff],2);
  call('GetStringTypeW',[1,accented,1,a.buffer],1,'classification');
  call('GetLocaleInfoW',[0x400,0x20001004,a.buffer,2],2,'locale-number');
  call('GetCommandLineA',[],'pointer','command-line','commandA');call('GetCommandLineW',[],'pointer','command-line-wide','commandW');
  call('SetEnvironmentVariableW',[environment,value],1);call('GetEnvironmentVariableW',[environment,a.buffer,32],2,'environment');call('ExpandEnvironmentStringsW',[expand,a.buffer,32],5,'expanded');
  call('GetSystemTimeAsFileTime',[a.filetime],0,'system-filetime');call('GetSystemTime',[a.calendar],0,'system-time');
  call('SystemTimeToFileTime',[date,a.filetime],1,'leap-filetime');call('FileTimeToSystemTime',[a.filetime,a.calendar],1,'leap-roundtrip');
  call('GetDateFormatW',[0x409,0,date,0,a.buffer,64],10,'date-format');call('GetTimeFormatW',[0x409,0,date,0,a.buffer,64],11,'time-format');
  call('ExitProcess',[0],'exit');return b.seal();
}
