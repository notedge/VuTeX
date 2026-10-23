# rutex-renderer-image

RuTeX 的图像渲染后端实现，负责将排版结果直接渲染为位图（如 PNG）。

## 核心职责

- **ImageBackend**: 基于 `tiny-skia` 的渲染后端实现。
- **字体渲染**: 使用 `ttf-parser` 直接从字体文件提取字形轮廓，实现高质量的矢量字形渲染。
- **独立性**: 不依赖系统渲染库，可在 WASM 或无界面环境下运行。

## 核心 Traits

- `LayoutBackend`: 与 `rutex-renderer-svg` 保持一致的渲染操作定义。

## 依赖

- `rutex-types`: 核心类型定义。
- `rutex-layout`: 布局节点定义。
- `rutex-font`: 提供字体数据支持。
- `tiny-skia`: 高性能 2D 绘图引擎。
- `ttf-parser`: 字体解析与轮廓提取。
