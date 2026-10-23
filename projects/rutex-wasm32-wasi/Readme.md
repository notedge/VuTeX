# rutex-wasm32-wasi

RuTeX 的 WebAssembly (WASI) 绑定实现。目前作为过渡方案，提供了基于 KaTeX 的 WASM 渲染能力。

## 核心职责

- **WASM 适配**: 为不支持浏览器 DOM 的 WASI 环境提供数学公式渲染支持。
- **KaTeX 桥接**: (当前阶段) 封装 KaTeX 核心逻辑，通过 `wasm-bindgen` 提供高效的渲染接口。
- **跨语言调用**: 为其他支持 WASM 的运行时（如 Node.js, Wasmtime）提供公式渲染服务。

## 使用示例

```rust
use rutex_wasm32_wasi::KaTeXOptions;

fn main() {
    let options = KaTeXOptions::display_mode();
    let result = options.render("\\frac{a}{b}");
    println!("Rendered output: {}", result);
}
```

## 技术特性

- **高性能**: 直接在 WASM 虚拟机中运行，减少 JS 与 Rust 之间的序列化开销。
- **标准化**: 遵循 WASI 规范，具有良好的可移植性。

## 依赖

- `wasm-bindgen`: 处理 Rust 与 JavaScript 的交互。
- `serde`: 配置选项的序列化。
