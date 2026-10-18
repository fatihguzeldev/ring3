import { mkdir, readFile, realpath, writeFile } from "node:fs/promises";
import { dirname } from "node:path";
import { fileURLToPath } from "node:url";

const source = new URL("./categories.json", import.meta.url);
const rustTarget = new URL("../rust/src/catalog/generated.rs", import.meta.url);
const typescriptTarget = new URL("../typescript/src/catalog/generated.ts", import.meta.url);

function rustSymbol(key) {
  return key.replace(/([a-z0-9])([A-Z])/g, "$1_$2").toUpperCase();
}

export function validateCatalog(catalog) {
  if (
    !catalog ||
    typeof catalog !== "object" ||
    Array.isArray(catalog) ||
    !Number.isInteger(catalog.version) ||
    catalog.version < 1 ||
    catalog.version > 0xffff_ffff ||
    !Array.isArray(catalog.categories) ||
    catalog.categories.length === 0 ||
    catalog.categories.length > 1024 ||
    Object.keys(catalog).some((key) => !["version", "categories"].includes(key))
  ) {
    throw new Error("invalid category catalog");
  }
  const ids = new Map();
  const keys = new Set();
  const symbols = new Set();
  for (const category of catalog.categories) {
    if (
      !category ||
      typeof category !== "object" ||
      Array.isArray(category) ||
      Object.keys(category).sort().join(",") !== "description,id,key,label,parent,selectable"
    ) {
      throw new Error("invalid category descriptor");
    }
    const { id, key, label, description, parent, selectable } = category;
    if (
      typeof id !== "string" ||
      id.length > 128 ||
      !/^[a-z][a-z0-9-]*(?:\.[a-z][a-z0-9-]*)*$/.test(id)
    )
      throw new Error("invalid category id");
    if (ids.has(id)) throw new Error(`duplicate category id: ${id}`);
    if (typeof key !== "string" || !/^[a-z][a-zA-Z0-9]*$/.test(key) || key.length > 128)
      throw new Error("invalid category key");
    if (keys.has(key)) throw new Error(`duplicate category key: ${key}`);
    const symbol = rustSymbol(key);
    if (symbols.has(symbol)) throw new Error(`duplicate Rust category symbol: ${symbol}`);
    for (const [value, maximum] of [
      [label, 128],
      [description, 256],
    ]) {
      if (
        typeof value !== "string" ||
        value.trim().length === 0 ||
        value.length > maximum ||
        /[\x00-\x1f\x7f-\x9f]/.test(value) ||
        [...value].some((character) => {
          const code = character.codePointAt(0);
          return code >= 0xd800 && code <= 0xdfff;
        })
      )
        throw new Error("invalid category description");
    }
    if ((parent !== null && typeof parent !== "string") || typeof selectable !== "boolean")
      throw new Error("invalid category ancestry");
    ids.set(id, category);
    keys.add(key);
    symbols.add(symbol);
  }
  for (const category of catalog.categories) {
    const visited = new Set([category.id]);
    let parent = category.parent;
    while (parent !== null) {
      if (!ids.has(parent)) throw new Error(`unknown category parent: ${parent}`);
      if (visited.has(parent)) throw new Error(`category ancestry cycle: ${parent}`);
      visited.add(parent);
      parent = ids.get(parent).parent;
    }
  }
  for (const category of catalog.categories) {
    const separator = category.id.lastIndexOf(".");
    const expectedParent = separator < 0 ? null : category.id.slice(0, separator);
    if (category.parent !== expectedParent)
      throw new Error(`category parent must match its dotted id: ${category.id}`);
  }
  if (ids.get("observability")?.selectable !== false || ids.get("observability")?.parent !== null) {
    throw new Error("recording control category must be nonselectable and have no parent");
  }
  return [...catalog.categories].sort((left, right) =>
    left.id < right.id ? -1 : left.id > right.id ? 1 : 0,
  );
}

export function renderCatalog(catalog, sourceText) {
  const entries = validateCatalog(catalog);
  const byId = new Map(entries.map((entry) => [entry.id, entry]));
  let fingerprint = 0xcbf29ce484222325n;
  const sourceBytes = new TextEncoder().encode(
    sourceText ?? JSON.stringify({ version: catalog.version, categories: entries }),
  );
  for (const byte of sourceBytes)
    fingerprint = BigInt.asUintN(64, (fingerprint ^ BigInt(byte)) * 0x100000001b3n);
  const header =
    "// generated from observability/catalog/categories.json; run node observability/catalog/generate.mjs.\n";
  const rust = `${header}use super::{Category, CategoryDescriptor};\n\npub const CATALOG_VERSION: u32 = ${catalog.version};\npub(super) const CATALOG_SOURCE_FINGERPRINT: u64 = 0x${fingerprint.toString(16).padStart(16, "0")};\n\npub mod categories {\n    use super::Category;\n${entries.map((entry) => `    pub const ${rustSymbol(entry.key)}: Category = Category::new_unchecked(${JSON.stringify(entry.id)});`).join("\n")}\n}\n\npub const CATEGORIES: &[CategoryDescriptor] = &[\n${entries.map((entry) => `    CategoryDescriptor {\n        id: ${JSON.stringify(entry.id)},\n        key: ${JSON.stringify(entry.key)},\n        label: ${JSON.stringify(entry.label)},\n        description: ${JSON.stringify(entry.description)},\n        parent: ${entry.parent === null ? "None" : `Some(${JSON.stringify(entry.parent)})`},\n        selectable: ${entry.selectable},\n    },`).join("\n")}\n];\n`;
  const typescript = `${header}import type { Category, CategoryDescriptor } from "./types.js";\n\nexport const catalogVersion: number = ${catalog.version};\n\nexport const categories: Readonly<{\n${entries.map((entry) => `  readonly ${entry.key}: Category;`).join("\n")}\n}> = Object.freeze({\n${entries.map((entry) => `  ${entry.key}: ${JSON.stringify(entry.id)} as Category,`).join("\n")}\n});\n\nexport const categoryCatalog: readonly CategoryDescriptor[] = Object.freeze([\n${entries.map((entry) => `  Object.freeze({\n    id: categories.${entry.key},\n    label: ${JSON.stringify(entry.label)},\n    description: ${JSON.stringify(entry.description)},\n    parent: ${entry.parent === null ? "null" : `categories.${byId.get(entry.parent).key}`},\n    selectable: ${entry.selectable},\n  }),`).join("\n")}\n]);\n`;
  return { rust, typescript };
}

async function generate(check) {
  const sourceText = await readFile(source, "utf8");
  const catalog = JSON.parse(sourceText);
  const output = renderCatalog(catalog, sourceText);
  for (const [target, content] of [
    [rustTarget, output.rust],
    [typescriptTarget, output.typescript],
  ]) {
    if (check) {
      const actual = await readFile(target, "utf8").catch(() => "");
      if (actual !== content) throw new Error(`generated catalog drift: ${fileURLToPath(target)}`);
    } else {
      await mkdir(dirname(fileURLToPath(target)), { recursive: true });
      await writeFile(target, content);
    }
  }
}

if (process.argv[1] && (await realpath(process.argv[1])) === fileURLToPath(import.meta.url)) {
  if (process.argv.slice(2).some((argument) => argument !== "--check"))
    throw new Error("usage: node generate.mjs [--check]");
  await generate(process.argv.includes("--check"));
}
