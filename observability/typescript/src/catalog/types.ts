declare const categoryBrand: unique symbol;

export type Category = string & { readonly [categoryBrand]: true };

export interface CategoryDescriptor {
  readonly id: Category;
  readonly label: string;
  readonly description: string;
  readonly parent: Category | null;
  readonly selectable: boolean;
}
