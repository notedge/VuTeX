# 安装与快速开始

## 环境要求

- Node.js 20 或更新版本。
- pnpm，或支持 workspace package 的 npm 客户端。

## 公式排版

```sh
pnpm add @vutex/wetex
```

`@vutex/wetex` 提供公式渲染与测量接口，不需要安装完整文档 CLI。

## runtime 与 CLI

```sh
pnpm add @vutex/vutex
pnpm exec vutex --help
```

CLI 当前提供 `render`、`inspect` 与 `package` 命令。平台 native binding 需要对应平台包可用；安装后可先运行 `vutex --help` 检查命令入口。
