# rutex-wit

RuTeX 的 WebAssembly 组件模型 (WIT) 接口定义与实现。

## 核心职责

- **组件化定义**: 使用 WIT (WebAssembly Interface Type) 定义 RuTeX 的对外接口，支持 WebAssembly 组件模型。
- **多语言互操作**: 允许不同编程语言（如 Python, Go, JavaScript）通过标准化的组件接口调用 RuTeX 的排版能力。
- **现代 WASM 生态适配**: 紧跟 WASM 组件模型发展，为 RuTeX 提供下一代的跨平台集成方案。

## 核心接口

(待完善，将包含 `render`、`layout` 等核心方法的 WIT 定义)

## 依赖

- `wasm-bindgen`: 基础绑定支持。
- `wit-bindgen`: 生成 WIT 接口代码。
- `serde`: 数据结构的序列化与反序列化。
