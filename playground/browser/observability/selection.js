export class CategorySelection {
  constructor(catalog) {
    const entries = new Map(catalog.map((entry) => [entry.id, entry]));
    this.nodes = new Map(
      catalog
        .filter((entry) => entry.selectable)
        .map((entry) => [entry.id, { entry, children: [] }]),
    );
    this.roots = [];
    for (const node of this.nodes.values()) {
      let parent = node.entry.parent;
      while (parent !== null && !this.nodes.has(parent))
        parent = entries.get(parent)?.parent ?? null;
      if (parent === null) this.roots.push(node);
      else this.nodes.get(parent).children.push(node);
    }
    const leaves = (node) => {
      node.leaves = node.children.length ? node.children.flatMap(leaves) : [node.entry.id];
      return node.leaves;
    };
    this.leaves = this.roots.flatMap(leaves);
    this.selected = new Set(this.leaves);
  }
  state(id) {
    const node = this.nodes.get(id);
    if (!node) throw new Error(`unknown recording category: ${id}`);
    const count = node.leaves.filter((leaf) => this.selected.has(leaf)).length;
    return {
      checked: count === node.leaves.length,
      indeterminate: count > 0 && count < node.leaves.length,
    };
  }
  toggle(id, checked) {
    const node = this.nodes.get(id);
    if (!node) throw new Error(`unknown recording category: ${id}`);
    for (const leaf of node.leaves) checked ? this.selected.add(leaf) : this.selected.delete(leaf);
  }
  setAll(checked) {
    this.selected = checked ? new Set(this.leaves) : new Set();
  }
  ids() {
    if (this.selected.size === this.leaves.length) return undefined;
    const collect = (node) =>
      this.state(node.entry.id).checked ? [node.entry.id] : node.children.flatMap(collect);
    return this.roots.flatMap(collect);
  }
}
