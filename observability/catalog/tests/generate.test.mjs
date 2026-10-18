import assert from "node:assert/strict";
import test from "node:test";
import { mkdir, mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { execFile } from "node:child_process";
import { promisify } from "node:util";
import { validateCatalog, renderCatalog } from "../generate.mjs";

const catalog = JSON.parse(await readFile(new URL("../categories.json", import.meta.url), "utf8"));

test("canonical catalog produces deterministic Rust and TypeScript data", () => {
  assert.deepEqual(
    renderCatalog(catalog),
    renderCatalog({ ...catalog, categories: [...catalog.categories].reverse() }),
  );
  assert.match(renderCatalog(catalog).rust, /pub const ENGINE_CPU_DECODE: Category/);
  assert.match(renderCatalog(catalog).typescript, /engineCpuDecode/);
});

test("catalog rejects duplicate IDs, keys and generated Rust symbols", () => {
  const duplicate = structuredClone(catalog);
  duplicate.categories.push(duplicate.categories[0]);
  assert.throws(() => validateCatalog(duplicate), /duplicate category id/);
  const key = structuredClone(catalog);
  key.categories[1].key = key.categories[0].key;
  assert.throws(() => validateCatalog(key), /duplicate category key/);
  const symbol = structuredClone(catalog);
  symbol.categories[1].key = "engineCPU";
  symbol.categories[2].key = "engineCpu";
  assert.throws(() => validateCatalog(symbol), /duplicate Rust category symbol/);
});

test("catalog rejects malformed IDs, missing parents and ancestry cycles", () => {
  const invalid = structuredClone(catalog);
  invalid.categories[1].id = "engine..cpu";
  assert.throws(() => validateCatalog(invalid), /invalid category id/);
  const parent = structuredClone(catalog);
  parent.categories[1].parent = "missing";
  assert.throws(() => validateCatalog(parent), /unknown category parent/);
  const cycle = structuredClone(catalog);
  cycle.categories[0].parent = "engine.cpu";
  assert.throws(() => validateCatalog(cycle), /category ancestry cycle/);
  const inconsistent = structuredClone(catalog);
  inconsistent.categories[2].parent = "host";
  assert.throws(() => validateCatalog(inconsistent), /parent must match its dotted id/);
});

test("catalog descriptions retain valid Unicode and reject unpaired UTF-16 surrogates", () => {
  const unicode = structuredClone(catalog);
  unicode.categories[0].description = 'İşlem "açılış" ve \\veri';
  assert.match(renderCatalog(unicode).rust, /İşlem/);
  assert.match(renderCatalog(unicode).rust, /\\"açılış\\"/);
  unicode.categories[0].description = "bad \ud800";
  assert.throws(() => validateCatalog(unicode), /invalid category description/);
});

test("generator --check accepts identical output and rejects generated source drift", async () => {
  const temporary = await mkdtemp(join(tmpdir(), "ring3-catalog-"));
  const execute = promisify(execFile);
  try {
    const directory = join(temporary, "catalog");
    await mkdir(directory);
    await writeFile(
      join(directory, "generate.mjs"),
      await readFile(new URL("../generate.mjs", import.meta.url)),
    );
    await writeFile(
      join(directory, "categories.json"),
      await readFile(new URL("../categories.json", import.meta.url)),
    );
    await execute(process.execPath, [join(directory, "generate.mjs")]);
    await execute(process.execPath, [join(directory, "generate.mjs"), "--check"]);
    const generated = join(temporary, "typescript/src/catalog/generated.ts");
    await writeFile(generated, (await readFile(generated, "utf8")) + "\n");
    await assert.rejects(
      execute(process.execPath, [join(directory, "generate.mjs"), "--check"]),
      /generated catalog drift/,
    );
  } finally {
    await rm(temporary, { recursive: true, force: true });
  }
});
