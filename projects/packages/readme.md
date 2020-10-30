# packages

| 目录 | 包名 | 说明 |
|---|---|---|
| [`vutex`](vutex/readme.md) | `@vutex/vutex` | 统一 runtime facade + 官方 CLI |
| [`wetex`](wetex/readme.md) | `@vutex/wetex` | 公式渲染发行版 |
| [`vutex-unknown-wasm32`](vutex-unknown-wasm32/readme.md) | `@vutex/vutex-unknown-wasm32` | wasm-pack 产物（`pnpm build:wasm`） |
| `vutex-win32-x64` 等 | `@vutex/vutex-<platform>` | `vutex-napi` 平台袋（`pnpm build:napi`） |

Rust 绑定：`../crates/vutex-napi`、`../crates/wetex-wasm`。
