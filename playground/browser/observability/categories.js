import {
  categoryCatalog,
  resolveCategorySelection,
} from "../../../observability/typescript/dist/index.js";
import { CategorySelection } from "./selection.js";

export class CategoryPicker {
  constructor(fieldset, summary) {
    this.fieldset = fieldset;
    this.summary = summary;
    this.selection = new CategorySelection(categoryCatalog);
    this.inputs = new Map();
    const document = fieldset.ownerDocument;
    const legend = document.createElement("legend");
    legend.textContent = "recording components";
    const actions = document.createElement("div");
    actions.className = "buttons";
    for (const [text, checked] of [
      ["select all", true],
      ["select none", false],
    ]) {
      const button = document.createElement("button");
      button.type = "button";
      button.textContent = text;
      button.addEventListener("click", () => {
        if (fieldset.disabled) return;
        this.selection.setAll(checked);
        this.refresh();
      });
      actions.append(button);
    }
    const list = (nodes) => {
      const container = document.createElement("ul");
      for (const node of nodes) {
        const item = document.createElement("li");
        const label = document.createElement("label");
        const input = document.createElement("input");
        input.type = "checkbox";
        input.value = node.entry.id;
        const description = document.createElement("p");
        description.id = `recording-category-description-${this.inputs.size}`;
        description.textContent = node.entry.description;
        input.setAttribute("aria-describedby", description.id);
        input.addEventListener("change", () => {
          if (fieldset.disabled) return;
          this.selection.toggle(node.entry.id, input.checked);
          this.refresh();
        });
        this.inputs.set(node.entry.id, input);
        label.append(input, document.createTextNode(node.entry.label));
        item.append(label, description);
        if (node.children.length) item.append(list(node.children));
        container.append(item);
      }
      return container;
    };
    fieldset.replaceChildren(legend, actions, list(this.selection.roots));
    this.refresh();
  }
  refresh() {
    for (const [id, input] of this.inputs) Object.assign(input, this.selection.state(id));
    const ids = this.selection.ids();
    this.summary.textContent =
      ids === undefined ? "all selected" : ids.length === 0 ? "none selected" : "custom selection";
  }
  categories() {
    return resolveCategorySelection(this.selection.ids());
  }
  setDisabled(disabled) {
    this.fieldset.disabled = disabled;
  }
}
