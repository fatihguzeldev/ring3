import assert from 'node:assert/strict';
export function checkDatafile(call, {guest, read, result}) {
  const a = guest.addresses;
  const fileBase = () => read(a.fileOne) & ~3;
  const imageBase = () => read(a.imageOnly) & ~3;
  const resourceRva = a.resourceDirectory - 0x400000;
  switch (call.hook) {
    case 'datafile-first': assert.equal(result & 3, 1); break;
    case 'datafile-second':
      assert.equal(result & 3, 1);
      assert.notEqual(result, read(a.fileOne));
      break;
    case 'datafile-image':
      assert.equal(result & 3, 2);
      assert.equal(read((result & ~3) + resourceRva + 0x350), 0xaaaaaaaa, 'resource mapping never assigns a static TLS index');
      break;
    case 'datafile-payload':
      assert.equal(result, fileBase() + 0x4200 + resourceRva - 0x6000 + 0x180);
      assert.equal(read(result), 0x44332211);
      break;
    case 'image-payload':
      assert.equal(result, imageBase() + resourceRva + 0x180);
      assert.equal(read(result), 0x44332211);
      break;
    case 'datafile-query':
    case 'image-query':
      assert.equal(read(a.virtualInfo + 4), call.hook === 'image-query' ? imageBase() : fileBase());
      assert.equal(read(a.virtualInfo + 16), 0x1000);
      assert.equal(read(a.virtualInfo + 20), 2);
      assert.equal(read(a.virtualInfo + 24), call.hook === 'image-query' ? 0x1000000 : 0x40000);
      break;
    case 'datafile-noaccess': assert.equal(read(a.oldProtection), 2); break;
    case 'datafile-readonly': assert.equal(read(a.oldProtection), 1); break;
  }
}
