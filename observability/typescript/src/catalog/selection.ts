import { categoryCatalog } from "./generated.js";
import type { Category, CategoryDescriptor } from "./types.js";

const descriptors: ReadonlyMap<string, CategoryDescriptor> = new Map(
  categoryCatalog.map((category) => [category.id, category]),
);

export function requireCategory(category: Category): CategoryDescriptor {
  const descriptor = descriptors.get(category);
  if (descriptor === undefined)
    throw new TypeError(`unregistered observation category: ${String(category)}`);
  return descriptor;
}

export function resolveCategorySelection(
  ids: readonly string[] | undefined,
): readonly Category[] | undefined {
  if (ids === undefined) return undefined;
  if (ids.length > categoryCatalog.length) throw new RangeError("too many category selections");
  const seen = new Set<string>();
  return Object.freeze(
    ids.map((id) => {
      const descriptor = descriptors.get(id);
      if (descriptor === undefined || !descriptor.selectable)
        throw new TypeError(`unknown or nonselectable observation category: ${String(id)}`);
      if (seen.has(id)) throw new TypeError(`duplicate observation category: ${id}`);
      seen.add(id);
      return descriptor.id;
    }),
  );
}

export function encodeCategorySelection(selection: readonly Category[] | undefined): string {
  const categories = resolveCategorySelection(selection);
  return categories === undefined ? "" : categories.length === 0 ? "-" : categories.join("\n");
}

export function matchesCategory(
  category: Category,
  selection: readonly Category[] | undefined,
): boolean {
  let descriptor = requireCategory(category);
  if (!descriptor.selectable || selection === undefined) return true;
  for (;;) {
    if (selection.includes(descriptor.id)) return true;
    if (descriptor.parent === null) return false;
    descriptor = requireCategory(descriptor.parent);
  }
}
