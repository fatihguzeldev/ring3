import assert from 'node:assert/strict';

export function configure(host) {
  const {ok, request, take} = host;
  ok('configure_cpu_clock', 1_000_000_000, 0, 0, 0);
  ok('configure_execution', 128, 16 * 1024 * 1024, 0xffffffff);
  ok('configure_synchronization', 64, 128, 256, 32);
  ok('configure_module_limits', 16, 4 * 1024 * 1024, 16, 65536, 1024 * 1024);
  ok('register_kernel32_module', 0x70000000);
  ok('configure_ole32', 0x68000000, 64, 64, 16);
  ok('register_ole32_module', 0x76000000);
  ok('configure_registry', 256, 1024, 64, 1024 * 1024, 256, 0);
  ok('register_advapi32_module', 0x77000000);
  const config = Buffer.alloc(80);
  config.write('R3FC'); config.writeUInt16LE(1, 4); config.writeUInt16LE(1, 6); config.writeUInt32LE(80, 8);
  [512, 128, 2, 256, 16, 16 * 1024 * 1024, 256, 16, 16, 32, 16, 1024 * 1024, 0x30000000, 64].forEach((value, index) => config.writeUInt32LE(value, 16 + index * 4));
  request(config); ok('configure_files', 80);
  assert.equal(take(64).subarray(0, 4).toString(), 'R3FS');
  return take(64).readBigUInt64LE(16);
}
