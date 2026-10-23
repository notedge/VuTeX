# rutex-renderer

RuTeX 的渲染后端抽象与实现，负责将排版结果输出为 SVG、HTML 或 Canvas 指令。

## 核心职责

- **渲染接口 (LayoutBackend)**: 定义统一的渲染 Traits，解耦排版引擎与具体输出格式。
- **SVG 后端**: (当前实现) 生成高质量、可缩放的 SVG 代码，支持 Web 和原生应用嵌入。
- **未来扩展**: 计划支持 HTML/CSS (精准定位 `<span>`) 和 `raqote`/`lyon` (原生 2D 绘图) 后端。

## 核心 Traits

- `LayoutBackend`: 定义基础渲染操作，如 `render_text` 和 `render_rect`。

## 依赖

- `rutex-types`: 核心类型定义。
- `serde`: 序列化支持。
