# 产品边界与渲染流程

VuTeX 围绕语言无关的文档语义、布局和输出组织。输入语言由可替换适配器处理，统一进入文档模型后再计算版面。

```text
Source → Adapter → Document IR → Layout → Presentation → Output
```

核心输出方向包括 PDF、SVG、HTML 与栅格格式。公式排版由同一内部公式管线驱动，WeTeX 则提供独立的 WASM 与 JavaScript 门面。
