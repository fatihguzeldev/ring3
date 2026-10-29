import assert from 'node:assert/strict';
import {test} from 'node:test';
import {selectedFolder} from './folder.js';
import {Engine} from '../runtime/engine.js';

function file(path, bytes = [1,2,3,4]) {
  const result = new File([new Uint8Array(bytes)],path.split('/').at(-1));
  Object.defineProperty(result,'webkitRelativePath',{value:path});
  result.arrayBuffer = () => assert.fail('folder selection must not read complete files');
  return result;
}
test('folder selection preserves relative directories and executable choices without reading payloads',() => {
  const input = [file('game/data/texture.bin'),file('game/bin/game.exe'),file('game/setup.exe')];
  const game = selectedFolder(input);
  assert.equal(game.root,'game');
  assert.deepEqual(game.executables.map(item => item.path),['C:\\bin\\game.exe','C:\\setup.exe']);
  assert.deepEqual(game.manifest.files.filter(item => item.isDirectory).map(item => item.path),['C:\\bin','C:\\data']);
  const texture = game.manifest.files.find(item => item.path === 'C:\\data\\texture.bin');
  assert.equal(game.localFiles.get(BigInt(texture.id)),input[0]);
  assert.equal(game.manifest.executable,game.selectedExecutable);
});
test('cancelled or executable-free selection does not fabricate an executable',() => {
  assert.equal(selectedFolder([]),null);
  const game = selectedFolder([file('game/readme.txt')]);
  assert.equal(game.configured,false);
  assert.equal(game.selectedExecutable,'');
});
test('ambiguous Windows paths and parent traversal fail before publication',() => {
  assert.throws(() => selectedFolder([file('game/a.exe'),file('game/A.EXE')]),/duplicate Windows path/);
  assert.throws(() => selectedFolder([file('game/a.exe'),file('other/b.exe')]),/invalid selected folder path/);
  assert.throws(() => selectedFolder([file('game/../a.exe')]),/invalid selected folder path/);
});
test('Worker byte source reads only the requested File slice and rejects invalid ranges',async () => {
  const game = selectedFolder([file('game/game.exe',[9,8,7,6])]);
  const h = new Engine({},null,'test');
  h.localFiles = structuredClone(game.localFiles);
  assert.deepEqual([...await h.bytes('',1n,1n,2)],[8,7]);
  await assert.rejects(h.bytes('',1n,3n,2),/invalid local file span/);
  await assert.rejects(h.bytes('',99n,0n,1),/invalid local file span/);
});
