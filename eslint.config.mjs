import js from "@eslint/js";
import html from "@html-eslint/eslint-plugin";
import { defineConfig, globalIgnores } from "eslint/config";
import prettier from "eslint-config-prettier/flat";
import globals from "globals";
import tseslint from "typescript-eslint";

export default defineConfig(
  globalIgnores(["target/**", "**/dist/**", "**/node_modules/**", ".husky/_/**"]),
  {
    files: ["**/*.{js,mjs,cjs}"],
    extends: [js.configs.recommended],
    languageOptions: { globals: globals.node },
  },
  {
    files: ["**/*.{ts,tsx,mts,cts}"],
    extends: [js.configs.recommended, tseslint.configs.recommended],
  },
  {
    files: ["runtime/src/**/*.{ts,tsx}"],
    languageOptions: { globals: { ...globals.browser, ...globals.worker } },
  },
  {
    files: ["**/*.html"],
    plugins: { html },
    extends: ["html/recommended"],
    language: "html/html",
    rules: {
      "html/no-duplicate-class": "error",
      "html/require-closing-tags": ["error", { selfClosing: "always" }],
      // prettier owns html whitespace and quote style.
      "html/attrs-newline": "off",
      "html/element-newline": "off",
      "html/indent": "off",
      "html/quotes": "off",
      "html/no-extra-spacing-tags": "off",
    },
  },
  prettier,
);
