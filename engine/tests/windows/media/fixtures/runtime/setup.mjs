import assert from 'node:assert/strict';

function config(magic, values) {
  const bytes = Buffer.alloc(64); bytes.write(magic); bytes.writeUInt16LE(1, 4); bytes.writeUInt16LE(1, 6); bytes.writeUInt32LE(64, 8);
  values.forEach((value, i) => bytes.writeUInt32LE(value, 16 + i * 4)); return bytes;
}

export function configure({api, request, metadata}) {
  const now = 100_000_000n;
  assert.equal(api.configure_cpu_clock(1_000_000_000, 0, Number(now), 0), 0);
  assert.equal(api.configure_media(48000), 0); const mediaEpoch = metadata(1).epoch;
  assert.equal(api.configure_synchronization(64, 128, 256, 16), 0);
  assert.ok(metadata(16).epoch > 0n);
  assert.equal(api.configure_module_limits(16, 0x100000, 16, 65536, 0x100000), 0);
  assert.equal(api.register_kernel32_module(0x70000000), 0);
  assert.equal(api.configure_user32(16, 16, 64, 16, 0x30000000, 16), 0);
  assert.equal(api.register_user32_module(0x72000000), 0);
  const sound = config('R3CS', [0x10000000, 128, 0x20000000, 64, 128, 16, 256, 0x100000, 1]);
  const invalid = Buffer.from(sound); invalid.writeUInt32LE(2, 48); request(invalid);
  assert.equal(api.configure_dsound(invalid.length), 7, 'capture availability is a strict boolean');
  request(sound); assert.equal(api.configure_dsound(sound.length), 0);
  assert.equal(api.register_dsound_module(0x73000000), 0);
  const input = config('R3CI', [0x11000000, 64, 0x21000000, 16, 128, 32, 256]);
  request(input); assert.equal(api.configure_dinput(input.length), 0);
  const controllerName = Buffer.from('authored controller', 'utf16le');
  const controller = Buffer.alloc(80 + controllerName.length);
  controller.write('R3ID'); controller.writeUInt16LE(1, 4); controller.writeUInt16LE(1, 6); controller.writeUInt32LE(controller.length, 8);
  [0, 2, 1, 4, 2, controllerName.length / 2].forEach((value, i) => controller.writeUInt32LE(value, 16 + i * 4));
  controller.fill(1, 40, 56); controller.fill(2, 56, 72); controllerName.copy(controller, 80);
  const malformedController = Buffer.from(controller); malformedController[72] = 1; request(malformedController);
  assert.equal(api.register_input_controller(controller.length), 7, 'reserved descriptor bits are rejected');
  request(controller); assert.equal(api.register_input_controller(controller.length), 0);
  request(controller); assert.equal(api.register_input_controller(controller.length), 7, 'identity cannot be registered twice');
  const state = Buffer.alloc(192); state.write('R3IS'); state.writeUInt16LE(1, 4); state.writeUInt16LE(1, 6); state.writeUInt32LE(192, 8);
  state.writeUInt32LE(1, 20); state.writeBigUInt64LE(now, 24); state.writeInt32LE(10000, 32); state.writeInt32LE(-2000, 64);
  for (let i = 0; i < 4; i++) state.writeUInt32LE(i ? 0xffffffff : 9000, 160 + i * 4);
  state[176] = 8; const invalidState = Buffer.from(state); invalidState.writeInt32LE(1, 40); request(invalidState);
  assert.equal(api.submit_controller_input(Number(mediaEpoch), 0, 192), 7, 'disabled controller objects remain neutral');
  request(state); assert.equal(api.submit_controller_input(Number(mediaEpoch), 0, 192), 0);
  assert.equal(api.configure_controller_haptics(Number(mediaEpoch & 0xffffffffn),Number(mediaEpoch >> 32n),0,2,8),0);
  const files=Buffer.alloc(80);files.write('R3FC');files.writeUInt16LE(1,4);files.writeUInt16LE(1,6);files.writeUInt32LE(80,8);
  [64,64,8,8,8,16*1024*1024,64,8,8,16,8,16*1024*1024,0x40000000,0x1000].forEach((value,i)=>files.writeUInt32LE(value,16+i*4));
  request(files);assert.equal(api.configure_files(80),0);
  assert.equal(api.register_dinput_module(0x74000000), 0);
  assert.equal(api.register_dinput8_module(0x75000000), 0);
  assert.equal(api.configure_ole32(0x12000000, 64, 64, 16), 0);
  assert.equal(api.register_ole32_module(0x76000000), 0);
  assert.equal(api.register_winmm_module(0x77000000), 0);
  return {now, mediaEpoch};
}
