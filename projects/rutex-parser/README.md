# rutex-parser

RuTeX 排版引擎的解析器核心，负责将 LaTeX 源码转换为数学语义树 (MST)。

## 核心职责

- **词法分析 (Lexing)**: 基于 `logos` 库实现的高效词法扫描器，支持 LaTeX 关键字、操作符、分组符号等。
- **宏系统 (Macro System)**: 采用不可变数据结构 (`im::HashMap`) 实现的宏定义与展开系统，支持 `\def` 类宏及代数效应式的状态管理。
- **语法解析 (Parsing)**: 递归下降解析器，处理 LaTeX 的数学模式语法，生成 `MathSemanticTree`。
- **环境管理**: 维护环境栈 (`environment_stack`) 和数学样式 (`math_style`)，支持 `\displaystyle` 等样式切换。

## 技术特性

- **不可变上下文**: `ParseContext` 使用持久化数据结构，支持确定性回放和增量解析。
- **错误定位**: 提供详尽的 `ParseError`，包含错误信息及在源码中的位置（待完善）。
- **代数效应模型**: 通过 `ParseEffect` 描述状态变更，实现解析过程与状态副作用的解耦。

## 依赖

- `rutex-types`: 核心类型定义。
- `logos`: 词法分析器生成工具。
- `im`: 持久化/不可变数据结构库。
- `hashbrown`: 高性能哈希映射实现。
