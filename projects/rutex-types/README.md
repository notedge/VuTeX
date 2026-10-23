# rutex-types

基础类型库，为 RuTeX 排版引擎提供核心数据结构和错误处理定义。

## 核心职责

- **错误处理**: 定义 `RuTeXError` 枚举和统一的 `Result` 类型，涵盖解析、排版、字体和渲染后端的各类错误。
- **数学语义树 (MST)**: 定义 `MathSemanticTree` 和 `SemanticNode`，作为 LaTeX 源码与物理布局之间的中间表示层。
- **基础原子**: 定义间距规则 (`SpacingRule`)、对齐方式 (`Alignment`)、符号角色 (`SymbolRole`) 等排版相关的元数据。

## 主要数据结构

- `RuTeXError`: 集中化的错误枚举。
- `MathSemanticTree`: 承载公式语义结构的根节点。
- `SemanticNode`: 递归的语义节点，包括：
    - `Sequence`: 节点序列。
    - `HorizontalBox` / `VerticalBox`: 水平/垂直布局容器。
    - `Fraction`: 分数。
    - `Radical`: 根号。
    - `Symbol`: 数学符号。
    - `Subscript` / `Superscript`: 上下标及其组合。

## 依赖

- `serde`: 提供高效的序列化与反序列化支持。
