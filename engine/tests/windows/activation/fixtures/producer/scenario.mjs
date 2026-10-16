import {builder} from '../../../../support/pe32/image.mjs';

export function image() {
  const b = builder([['kernel32.dll', [
    'CreateActCtxA', 'CreateActCtxW', 'AddRefActCtx', 'ReleaseActCtx',
    'ActivateActCtx', 'DeactivateActCtx', 'GetCurrentActCtx',
    'FindActCtxSectionStringA', 'FindActCtxSectionStringW', 'FindActCtxSectionGuid',
    'QueryActCtxW', 'QueryActCtxSettingsW', 'ZombifyActCtx', 'GetLastError', 'ExitProcess',
  ]]]);
  const {allocate, data, external, ref} = b;
  const wide = (name, text) => {
    const bytes = Buffer.from(text + '\0', 'utf16le');
    const padded = Buffer.alloc((bytes.length + 3) & ~3);
    bytes.copy(padded);
    return data(name, Array.from({length: padded.length / 4}, (_, index) => padded.readUInt32LE(index * 4)));
  };
  allocate('context'); allocate('ansiContext'); allocate('current');
  allocate('cookie'); allocate('nullCookie');
  allocate('find', [64, ...Array(15).fill(0)]);
  allocate('query', Array(256).fill(0xcccccccc));
  allocate('written'); allocate('settings', Array(8).fill(0));
  const specification = data('specification', [32, 0x88, 0, 0, 0, 2, 0, 0x400000]);
  const clsid = data('clsid', [0x12345678, 0xdef09abc, 0x04030201, 0x08070605]);
  const dll = wide('dll', 'private.dll');
  const window = wide('window', 'Ring3.Window');
  const setting = wide('setting', 'dpiAware');
  const progid = b.string('progid', 'Ring3.Activation.1');
  const assemblyIndex = data('assemblyIndex', [1]);
  const fileIndex = data('fileIndex', [1, 0]);
  const a = b.addresses;
  external('CreateActCtxA', 0x180001, [specification], 'handle', 'ansiContext');
  external('ReleaseActCtx', 0x180004, [ref('ansiContext')], 'void');
  external('CreateActCtxW', 0x180002, [specification], 'handle', 'context');
  external('GetCurrentActCtx', 0x180007, [a.current], 1, undefined, 'no-current');
  external('ActivateActCtx', 0x180005, [ref('context'), a.cookie], 1, undefined, 'cookie');
  external('GetCurrentActCtx', 0x180007, [a.current], 1, undefined, 'current');
  external('ReleaseActCtx', 0x180004, [ref('current')], 'void');
  external('FindActCtxSectionGuid', 0x18000a, [1, 0, 4, clsid, a.find], 1, undefined, 'com');
  external('FindActCtxSectionStringW', 0x180009, [0, 0, 2, dll, a.find], 1, undefined, 'dll');
  external('FindActCtxSectionStringA', 0x180008, [0, 0, 7, progid, a.find], 1, undefined, 'progid');
  external('FindActCtxSectionStringW', 0x180009, [0, 0, 3, window, a.find], 1, undefined, 'window');
  external('QueryActCtxW', 0x18000b, [0x80000000, ref('context'), 0, 1, a.query, 1024, a.written], 1, undefined, 'basic');
  external('QueryActCtxW', 0x18000b, [0, ref('context'), 0, 2, a.query, 16, a.written], 0, undefined, 'required');
  external('GetLastError', 0x10001, [], 122);
  external('QueryActCtxW', 0x18000b, [0, ref('context'), 0, 2, a.query, 1024, a.written], 1, undefined, 'detailed');
  external('QueryActCtxW', 0x18000b, [0, ref('context'), assemblyIndex, 3, a.query, 1024, a.written], 1, undefined, 'assembly');
  external('QueryActCtxW', 0x18000b, [0, ref('context'), fileIndex, 4, a.query, 1024, a.written], 1, undefined, 'file');
  external('QueryActCtxW', 0x18000b, [0, ref('context'), 0, 5, a.query, 1024, a.written], 1, undefined, 'runlevel');
  external('QueryActCtxW', 0x18000b, [0, ref('context'), 0, 6, a.query, 1024, a.written], 1, undefined, 'compatibility');
  external('QueryActCtxSettingsW', 0x18000c, [0, ref('context'), 0, setting, a.settings, 16, a.written], 1, undefined, 'setting');
  external('AddRefActCtx', 0x180003, [ref('context')], 'void');
  external('ReleaseActCtx', 0x180004, [ref('context')], 'void');
  external('ActivateActCtx', 0x180005, [0, a.nullCookie], 1, undefined, 'null-cookie');
  external('GetCurrentActCtx', 0x180007, [a.current], 1, undefined, 'no-current');
  external('FindActCtxSectionStringW', 0x180009, [0, 0, 2, dll, a.find], 0);
  external('GetLastError', 0x10001, [], 14007);
  external('DeactivateActCtx', 0x180006, [0, ref('nullCookie')], 1);
  external('ZombifyActCtx', 0x18000d, [ref('context')], 1);
  external('FindActCtxSectionStringW', 0x180009, [0, 0, 2, dll, a.find], 0);
  external('GetLastError', 0x10001, [], 14006);
  external('ReleaseActCtx', 0x180004, [ref('context')], 'void');
  external('ReleaseActCtx', 0x180004, [ref('context')], 'void');
  external('DeactivateActCtx', 0x180006, [0, ref('cookie')], 1);
  external('ActivateActCtx', 0x180005, [ref('context'), a.cookie], 0);
  external('GetLastError', 0x10001, [], 6);
  external('ExitProcess', 0x10003, [0], 'exit');

  const manifest = Buffer.from(`<assembly xmlns="urn:schemas-microsoft-com:asm.v1" manifestVersion="1.0">
    <assemblyIdentity name="Ring3.Activation" type="win32" version="1.2.3.4"/>
    <file name="private.dll"><comClass clsid="{12345678-9abc-def0-0102-030405060708}" threadingModel="Both" progid="Ring3.Activation.1"/><windowClass>Ring3.Window</windowClass></file>
    <trustInfo xmlns="urn:schemas-microsoft-com:asm.v3"><security><requestedPrivileges><requestedExecutionLevel level="asInvoker" uiAccess="false"/></requestedPrivileges></security></trustInfo>
    <application xmlns="urn:schemas-microsoft-com:asm.v3"><windowsSettings><dpiAware xmlns="http://schemas.microsoft.com/SMI/2005/WindowsSettings">true</dpiAware></windowsSettings></application>
    <compatibility xmlns="urn:schemas-microsoft-com:compatibility.v1"><application><supportedOS Id="{8e0f7a12-bfb3-4fe8-b9a5-48fd50a15a9a}"/></application></compatibility>
  </assembly>`);
  const resource = Buffer.alloc((0x100 + manifest.length + 3) & ~3);
  for (const offset of [14, 0x2e, 0x4e]) resource.writeUInt16LE(1, offset);
  for (const [offset, value] of [[16, 24], [20, 0x80000020], [0x30, 2], [0x34, 0x80000040], [0x50, 0x409], [0x54, 0x60]]) resource.writeUInt32LE(value, offset);
  const directory = data('resourceDirectory', Array(resource.length / 4).fill(0));
  const rva = directory - 0x400000;
  resource.writeUInt32LE(rva + 0x100, 0x60);
  resource.writeUInt32LE(manifest.length, 0x64);
  manifest.copy(resource, 0x100);
  const guest = b.seal();
  resource.copy(guest.bytes, 0x4200 + rva - 0x6000);
  guest.bytes.writeUInt32LE(rva, 0x98 + 112);
  guest.bytes.writeUInt32LE(resource.length, 0x98 + 116);
  return guest;
}
