import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import fs from 'node:fs';
import { mkdtemp, mkdir, readFile, readdir, rm, symlink, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import path, { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { runInNewContext } from 'node:vm';
import test from 'node:test';
import { applyBridge, prepareDistribution } from '../tools/prepare-backend.mjs';

async function fixture(t) {
  const directory = await mkdtemp(join(tmpdir(), 'ring3-distribution-'));
  t.after(() => rm(directory, { recursive: true, force: true }));
  const producer = join(directory, 'build.mjs');
  await writeFile(producer, `
    import { writeFileSync } from 'node:fs';
    import { join } from 'node:path';
    const [directory, version, mode] = process.argv.slice(2);
    writeFileSync(join(directory, 'index.html'), version);
    if (mode === 'fail') process.exit(2);
    writeFileSync(join(directory, 'ring3-runtime-build.json'), JSON.stringify({
      backend: 'bottleship-v86', cpuBuild: 'original pinned upstream artifact',
      upstreamRevision: 'test', artifacts: { 'v86.wasm': version }
    }));
    if (mode !== 'invalid') writeFileSync(join(directory, 'LICENSE'), 'notice');
  `);
  const build = (version, mode = 'success', duringBuild = () => {}) => prepareDistribution(directory,
    async staging => {
      await duringBuild(staging);
      execFileSync(process.execPath, [producer, staging, version, mode], { stdio: 'pipe' });
    }, async staging => {
      assert.equal(await readFile(join(staging, 'LICENSE'), 'utf8'), 'notice');
    });
  const current = () => readFile(join(directory, 'dist/index.html'), 'utf8');
  const previous = () => readFile(join(directory, 'dist.previous/index.html'), 'utf8');
  return { directory, build, current, previous };
}

test('repeated builds keep current and one previous distribution without touching legacy backups', async t => {
  const f = await fixture(t);
  const legacy = join(f.directory, 'dist.previous-legacy');
  await mkdir(legacy);
  await writeFile(join(legacy, 'user-file'), 'keep');
  await f.build('one');
  await f.build('two', 'success', async () => assert.equal(await f.current(), 'one'));
  await f.build('three', 'success', async () => {
    assert.equal(await f.current(), 'two');
    assert.equal(await f.previous(), 'one');
  });
  assert.equal(await f.current(), 'three');
  assert.equal(await f.previous(), 'two');
  assert.deepEqual((await readdir(f.directory)).filter(name => name.startsWith('dist')).sort(),
    ['dist', 'dist.previous', 'dist.previous-legacy']);
  assert.equal(await readFile(join(legacy, 'user-file'), 'utf8'), 'keep');
});

test('build or validation failure leaves both successful outputs intact and removes staging', async t => {
  const f = await fixture(t);
  await f.build('one');
  await f.build('two');
  for (const mode of ['fail', 'invalid']) {
    await assert.rejects(f.build('partial', mode));
    assert.equal(await f.current(), 'two');
    assert.equal(await f.previous(), 'one');
    assert.ok(!(await readdir(f.directory)).includes('dist.staging'));
  }
  await f.build('three');
  assert.equal(await f.current(), 'three');
  assert.equal(await f.previous(), 'two');
});

test('unknown staging or backup paths are preserved and stop the build', async t => {
  for (const name of ['dist.staging', 'dist.previous']) {
    const f = await fixture(t);
    await f.build('one');
    await mkdir(join(f.directory, name));
    await writeFile(join(f.directory, name, 'user-file'), 'keep');
    let called = false;
    await assert.rejects(f.build('two', 'success', () => { called = true; }));
    assert.equal(called, false);
    assert.equal(await f.current(), 'one');
    assert.equal(await readFile(join(f.directory, name, 'user-file'), 'utf8'), 'keep');
  }
});

test('a backup symlink is rejected without following or deleting it', async t => {
  const f = await fixture(t);
  await f.build('one');
  const userDirectory = join(f.directory, 'user-data');
  await mkdir(userDirectory);
  await writeFile(join(userDirectory, 'important'), 'keep');
  await symlink(userDirectory, join(f.directory, 'dist.previous'));
  await assert.rejects(f.build('two'));
  assert.equal(await f.current(), 'one');
  assert.equal(await readFile(join(userDirectory, 'important'), 'utf8'), 'keep');
});

test('the upstream public-copy patch honors the staged Vite output directory', async t => {
  const f = await fixture(t);
  const config = `// only ever wasted minutes/gigabytes on \`vite build\`. Copy everything else, skip apps/.
// (BYO games and agent/make-wgb loads stream off disk via serveWgbFromDisk — never in public/.)
function copyPublicDirExceptApps(): Plugin {
  return {
    name: "copy-public-dir-except-apps",
    apply: "build",
    closeBundle() {
      const publicDir = path.resolve(__dirname, "public");
      const outDir = path.resolve(__dirname, "dist");
      if (!fs.existsSync(publicDir)) return;
      fs.mkdirSync(outDir, { recursive: true });
      for (const entry of fs.readdirSync(publicDir, { withFileTypes: true })) {
        if (entry.name === "apps") continue;
        fs.cpSync(path.join(publicDir, entry.name), path.join(outDir, entry.name), { recursive: true });
      }
    },
  };
}
`;
  await writeFile(join(f.directory, 'vite.config.ts'), config);
  const patch = fileURLToPath(new URL('../backend/bridge.patch', import.meta.url));
  execFileSync('git', ['apply', '--include=vite.config.ts', patch], { cwd: f.directory });
  await mkdir(join(f.directory, 'public/apps'), { recursive: true });
  await mkdir(join(f.directory, 'dist'));
  await writeFile(join(f.directory, 'public/v86.wasm'), 'cpu');
  await writeFile(join(f.directory, 'public/apps/private.wgb'), 'authored fixture');
  await writeFile(join(f.directory, 'dist/index.html'), 'current');
  const source = (await readFile(join(f.directory, 'vite.config.ts'), 'utf8')).replace(': Plugin', '');
  const plugin = runInNewContext(source + '\ncopyPublicDirExceptApps()', { fs, path, __dirname: f.directory });
  plugin.configResolved({ root: f.directory, build: { outDir: 'dist.staging' } });
  plugin.closeBundle();
  assert.equal(await readFile(join(f.directory, 'dist/index.html'), 'utf8'), 'current');
  assert.deepEqual(await readdir(join(f.directory, 'dist')), ['index.html']);
  assert.deepEqual(await readdir(join(f.directory, 'dist.staging')), ['v86.wasm']);
});

const change = name => `diff --git a/${name} b/${name}
--- a/${name}
+++ b/${name}
@@ -1 +1 @@
-before
+after
`;

test('an existing bridge upgrades only its missing files and is idempotent', async t => {
  const f = await fixture(t);
  execFileSync('git', ['init', '--quiet', f.directory]);
  await writeFile(join(f.directory, 'old.txt'), 'before\n');
  await writeFile(join(f.directory, 'new.txt'), 'before\n');
  applyBridge(f.directory, change('old.txt'));
  applyBridge(f.directory, change('old.txt') + change('new.txt'));
  applyBridge(f.directory, change('old.txt') + change('new.txt'));
  assert.equal(await readFile(join(f.directory, 'old.txt'), 'utf8'), 'after\n');
  assert.equal(await readFile(join(f.directory, 'new.txt'), 'utf8'), 'after\n');
});

test('all bridge sections are checked before any missing section is applied', async t => {
  const f = await fixture(t);
  execFileSync('git', ['init', '--quiet', f.directory]);
  await writeFile(join(f.directory, 'missing.txt'), 'before\n');
  await writeFile(join(f.directory, 'conflict.txt'), 'user change\n');
  assert.throws(() => applyBridge(f.directory, change('missing.txt') + change('conflict.txt')));
  assert.equal(await readFile(join(f.directory, 'missing.txt'), 'utf8'), 'before\n');
  assert.equal(await readFile(join(f.directory, 'conflict.txt'), 'utf8'), 'user change\n');
});
