import assert from "node:assert/strict";
import { test } from "node:test";
import {
  categoryCatalog,
  categories,
  resolveCategorySelection,
} from "../../../observability/typescript/dist/index.js";
import { CategorySelection } from "./selection.js";

test("all catalog categories are selected by default, with distinct all and none selections", () => {
  const selection = new CategorySelection(categoryCatalog);
  assert.equal(selection.ids(), undefined);
  for (const entry of categoryCatalog.filter((entry) => entry.selectable)) {
    assert.deepEqual(selection.state(entry.id), { checked: true, indeterminate: false });
  }
  selection.setAll(false);
  assert.deepEqual(selection.ids(), []);
  selection.setAll(true);
  assert.equal(selection.ids(), undefined);
});

test("a parent toggles descendants and becomes indeterminate after a child is deselected", () => {
  const selection = new CategorySelection(categoryCatalog);
  selection.toggle(categories.engineCpu, false);
  for (const entry of categoryCatalog.filter((entry) =>
    String(entry.id).startsWith("engine.cpu"),
  )) {
    assert.deepEqual(selection.state(entry.id), { checked: false, indeterminate: false });
  }
  selection.toggle(categories.engineCpuDecode, true);
  assert.deepEqual(selection.state(categories.engineCpu), { checked: false, indeterminate: true });
  assert.ok(selection.ids().includes(categories.engineCpuDecode));
  assert.ok(!selection.ids().includes(categories.engineCpu));
  assert.deepEqual(resolveCategorySelection(selection.ids()), selection.ids());
});

test("a selected branch is represented once and a subset cannot re-enable unselected siblings", () => {
  const selection = new CategorySelection(categoryCatalog);
  selection.setAll(false);
  selection.toggle(categories.engineCpu, true);
  assert.deepEqual(selection.ids(), [categories.engineCpu]);
  selection.toggle(categories.engineCpuDecode, false);
  const selected = selection.ids();
  assert.ok(!selected.includes(categories.engineCpu));
  assert.ok(!selected.includes(categories.engineCpuDecode));
  assert.ok(selected.every((id) => String(id).startsWith("engine.cpu.")));
});

test("a new catalog entry automatically joins the tree and its parent selection", () => {
  const addition = {
    id: "engine.cpu.new-component",
    label: "new component",
    description: "an added instrumentation owner",
    parent: categories.engineCpu,
    selectable: true,
  };
  const selection = new CategorySelection([...categoryCatalog, addition]);
  const parent = selection.nodes.get(categories.engineCpu);
  assert.equal(parent.children.find((node) => node.entry.id === addition.id).entry, addition);
  assert.deepEqual(selection.state(addition.id), { checked: true, indeterminate: false });
  selection.toggle(categories.engineCpu, false);
  assert.deepEqual(selection.state(addition.id), { checked: false, indeterminate: false });
  assert.equal(selection.nodes.has("observability"), false);
});
