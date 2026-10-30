# `vutex.ts`

| 目录 | 包名 | 说明 |
|---|---|---|
| [`packages/vutex`](packages/vutex/readme.md) | `@vutex/vutex` | 统一 runtime facade + 官方 CLI |
| [`packages/wetex`](packages/wetex/readme.md) | `@vutex/wetex` | 公式渲染发行版 |
| [`platforms/vutex-unknown-wasm32`](platforms/vutex-unknown-wasm32/readme.md) | `@vutex/vutex-unknown-wasm32` | wasm-pack 产物（`pnpm build:wasm`） |
| `platforms/vutex-win32-x64` 等 | `@vutex/vutex-<platform>` | `vutex-napi` 平台袋（`pnpm build:napi`） |
| [`homepage`](homepage/package.json) | `@vutex/homepage` | 产品站 |

Rust 绑定：`../vutex.rs/exports/vutex-napi`、`../vutex.rs/exports/wetex-wasm`。
