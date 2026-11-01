# Install and get started

## Requirements

- Node.js 20 or newer.
- pnpm, or an npm client that supports workspace packages.

## Formula typesetting

```sh
pnpm add @vutex/watex
```

`@vutex/watex` provides formula rendering and measurement without the full document CLI.

## Runtime and CLI

```sh
pnpm add @vutex/vutex
pnpm exec vutex --help
```

The CLI currently exposes `render`, `inspect` and `package` commands. Native bindings require a package for the current platform. Run `vutex --help` to confirm the command entry is available.
