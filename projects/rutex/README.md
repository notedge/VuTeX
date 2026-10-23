# rutex

RuTeX 排版引擎的主入口，编排各个子模块完成从 LaTeX 源码到最终产物的转换。

## 项目愿景

打造一个工业级、纯 Rust 实现的数学排版引擎，作为 MathJax 和 KaTeX 的高性能、跨平台替代方案。

## 架构概览

RuTeX 采用多 crate 架构，实现高度解耦：

- `rutex-types`: 基础类型与错误处理。
- `rutex-parser`: LaTeX 词法分析与语义树构建。
- `rutex-layout`: 基于“盒子-胶水”模型的排版引擎。
- `rutex-font`: 字体度量与 OpenType Math 支持。
- `rutex-renderer`: 渲染后端抽象。

## 快速开始

```rust
use rutex::render;

fn main() {
    let tex = r"a^2 + b^2 = c^2";
    match render(tex) {
        Ok(svg) => println!("Generated SVG: {}", svg),
        Err(e) => eprintln!("Error: {}", e),
    }
}
```

## 技术特性

- **高性能**: 利用 Rust 的内存安全和并发特性，通过 `rayon` 支持公式并行处理。
- **可扩展性**: 插件化的渲染后端和宏系统。
- **轻量级**: WASM 优化，核心引擎保持在 < 200KB。

## 依赖

- `rutex-types`
- `rutex-parser`
- `rutex-layout`
- `rutex-font`
- `rutex-renderer`
- `rayon`: 并行计算支持。
- `serde`: 序列化支持。
