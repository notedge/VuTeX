import assert from "node:assert/strict";
import { mkdtemp, readFile, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";

import { PackageManager } from "../src/packages/manager.js";

test("package manager writes lockfile on install", async () => {
    const dir = await mkdtemp(join(tmpdir(), "vutex-pkg-"));
    try {
        const manager = new PackageManager(dir);
        await manager.add("font-libertinus", { version: "^0.1.0" });
        await manager.install();
        const lock = await readFile(join(dir, "vutex.lock"), "utf8");
        assert.match(lock, /font-libertinus/);
    } finally {
        await rm(dir, { recursive: true, force: true });
    }
});
