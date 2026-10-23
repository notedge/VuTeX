# rutex-font

RuTeX 的字体度量系统，负责管理 OpenType Math 表并提供符号尺寸、偏移等核心数据。

## 核心职责

- **度量查询**: 提供 `GlyphMetrics` 查询接口，返回符号的宽度、高度和深度。
- **并发缓存**: 使用 `DashMap` 实现全局共享的并发度量缓存系统，支持异步加载和高效访问。
- **OpenType Math 支持**: (计划中) 解析 OpenType 字体中的数学表，获取积分号、根号等特殊符号的伸缩和定位参数。

## 核心组件

- `FontMetricsSystem`: 字体度量系统的核心句柄，管理底层缓存和加载逻辑。
- `GlyphMetrics`: 描述单个字形物理尺寸的结构体。

## 技术特性

- **异步加载**: `get_metrics` 接口支持异步调用，为 WebAssembly 和多线程环境优化。
- **线程安全**: 基于 `Arc` 和 `DashMap`，支持在多个排版任务间并行共享字体数据。

## 依赖

- `rutex-types`: 核心类型定义。
- `dashmap`: 高性能并发哈希映射。
- `arc-swap`: (可选) 用于原子化替换全局配置。
