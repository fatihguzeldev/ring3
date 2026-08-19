# ring3

## Toolchains

- Rust/Cargo: `1.97.1`
- WebAssembly target: `wasm32-unknown-unknown`
- Node.js: `22.16.0`
- pnpm: `11.9.0`
- TypeScript: `5.9.3`

## Verification

```bash
pnpm install --frozen-lockfile
pnpm toolchain:check
pnpm format:rust
pnpm lint:rust
pnpm typecheck
pnpm build
```
