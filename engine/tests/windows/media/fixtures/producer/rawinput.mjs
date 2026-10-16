export const rawInputSymbols = [
  'RegisterRawInputDevices', 'GetRegisteredRawInputDevices', 'GetRawInputData',
  'GetRawInputBuffer', 'GetRawInputDeviceList', 'GetRawInputDeviceInfoW',
  'DefRawInputProc', 'DefWindowProcA',
];

export function rawInput(b) {
  const {allocate, data, external, ref} = b;
  allocate('rawCount', [2]);
  allocate('rawSize', [32]);
  allocate('rawHandle');
  allocate('rawDevices', Array(4).fill(0));
  allocate('rawRegistered', Array(6).fill(0));
  allocate('rawInfo', [32, 0, 0, 0, 0, 0, 0, 0]);
  allocate('rawData', Array(16).fill(0xcccccccc));
  const a = b.addresses;
  const rows = data('rawRegistration', [0x60001, 0x30, 0, 0x20001, 0x30, 0]);
  b.copy(rows + 8, a.window);
  b.copy(rows + 20, a.window);
  const remove = data('rawRemove', [0x60001, 1, 0, 0x20001, 1, 0]);
  external('RegisterRawInputDevices', 0x140001, [rows, 2, 12], 1);
  external('GetRegisteredRawInputDevices', 0x140002, [a.rawRegistered, a.rawCount, 12], 2, undefined, 'raw-registered');
  external('GetRawInputDeviceList', 0x140005, [0, a.rawCount, 8], 0, undefined, 'raw-device-count');
  external('GetRawInputDeviceList', 0x140005, [a.rawDevices, a.rawCount, 8], 2, undefined, 'raw-device-list');
  external('GetRawInputDeviceInfoW', 0x140007, [0x59000002, 0x2000000b, a.rawInfo, a.rawSize], 32, undefined, 'raw-device-info');
  external('PeekMessageA', 0x20015, [a.message, ref('window'), 0xff, 0xff, 1], 1, undefined, 'raw-input');
  b.copy(a.rawHandle, a.message + 12);
  external('GetRawInputData', 0x140003, [ref('rawHandle'), 0x10000005, a.rawData, a.rawSize, 16], 16, undefined, 'raw-header');
  external('GetRawInputData', 0x140003, [ref('rawHandle'), 0x10000003, 0, a.rawSize, 16], 0, undefined, 'raw-required');
  external('GetRawInputData', 0x140003, [ref('rawHandle'), 0x10000003, a.rawData, a.rawSize, 16], 32, undefined, 'raw-keyboard');
  external('DefWindowProcA', 0x2001f, [ref('window'), 0xff, 0, ref('rawHandle')], 0);
  external('GetRawInputData', 0x140003, [ref('rawHandle'), 0x10000003, a.rawData, a.rawSize, 16], 0xffffffff);
  b.write(a.rawSize, 64);
  external('GetRawInputBuffer', 0x140004, [a.rawData, a.rawSize, 16], 1, undefined, 'raw-mouse');
  external('PeekMessageA', 0x20015, [a.message, ref('window'), 0x100, 0x100, 1], 0);
  external('GetAsyncKeyState', 0x2002b, [0x44], {mask: 0xffff, value: 0x8001});
  external('GetKeyState', 0x2002c, [0x44], 0);
  external('GetRawInputBuffer', 0x140004, [a.rawData, a.rawSize, 16], 1, undefined, 'raw-release');
  external('GetRawInputBuffer', 0x140004, [a.rawData, a.rawSize, 16], 0);
  external('DefRawInputProc', 0x140008, [0xffffffff, 0xffffffff, 16], 0);
  external('DefRawInputProc', 0x140008, [0, 0, 24], 0xffffffff);
  external('RegisterRawInputDevices', 0x140001, [remove, 2, 12], 1);
  external('GetRegisteredRawInputDevices', 0x140002, [a.rawRegistered, a.rawCount, 12], 0);
}
