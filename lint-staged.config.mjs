export default {
  "*.{js,mjs,cjs,ts,tsx,mts,cts,html}": ["eslint --fix --max-warnings=0", "prettier --write"],
  "*.{css,json,yaml,yml,md}": "prettier --write",
  "*.{ts,tsx,mts,cts}": () => "pnpm typecheck",
  "{*.rs,Cargo.toml,Cargo.lock,rust-toolchain.toml,rustfmt.toml,.rustfmt.toml,clippy.toml,.clippy.toml}":
    () => ["cargo fmt --all -- --check", "cargo clippy --workspace --lib --locked -- -D warnings"],
};
