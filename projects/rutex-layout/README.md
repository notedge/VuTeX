# rutex-layout

RuTeX 的排版引擎，实现核心的“盒子-胶水-惩罚”模型及排版算法。

## 核心职责

- **盒子模型 (Box Model)**: 定义 `LayoutItem::Box`，包含宽度、高度和深度，是排版的基本物理单位。
- **弹性间距 (Glue)**: 定义 `LayoutItem::Glue`，支持拉伸和收缩，用于实现灵活的间距填充。
- **断行惩罚 (Penalty)**: 定义 `LayoutItem::Penalty`，用于在断行算法中计算最优断点。
- **Knuth-Plass 算法集成**: 计划实现完整的 Knuth-Plass 全局最优断行算法，以实现多行公式的高质量排版。

## 核心数据结构

- `LayoutItem`: 排版项枚举，包含 `Box`、`Glue` 和 `Penalty`。
- `LayoutEngine`: (待实现) 负责将 `MathSemanticTree` 转换为 `LayoutItem` 序列并进行空间分配。

## 未来规划

- 实现真正的 Knuth-Plass 动态规划算法。
- 使用定点数 (Fixed-point arithmetic) 进行尺寸计算，以避免浮点误差导致的对齐问题。
- 支持 OpenType Math 的扩展规则（如大括号的伸缩）。

## 依赖

- `rutex-types`: 核心类型定义。
- `serde`: 序列化支持。
