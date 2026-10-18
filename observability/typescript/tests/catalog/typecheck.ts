import { categories, createCapture, resolveCategorySelection } from "../../src/index.js";
import type { Category } from "../../src/index.js";

const capture = createCapture({
  level: "basic",
  categories: [categories.engineCpu],
  output: { async write() {}, async flush() {} },
});
capture.observer(categories.engineCpuDecode).event("decode");
resolveCategorySelection(["engine.cpu"]);

// @ts-expect-error categories are nominal references, not raw identifier strings.
capture.observer("engine.cpu");
// @ts-expect-error unknown category names cannot be referenced.
capture.observer(categories.engineUnknown);
createCapture({
  level: "basic",
  // @ts-expect-error configuration also requires nominal references.
  categories: ["engine.cpu"],
  output: { async write() {}, async flush() {} },
});
// @ts-expect-error a raw string cannot fabricate the category brand.
const forged: Category = "engine.cpu";
void forged;
