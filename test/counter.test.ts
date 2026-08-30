import assert from "node:assert/strict";
import test from "node:test";
import { readFileSync, existsSync } from "node:fs";
import { join } from "node:path";

test("counter source files exist and use LocalStorage API", () => {
  const srcDir = join(import.meta.dirname, "..", "src");
  for (const file of ["increment-counter.jsx", "decrement-counter.jsx", "reset-counter.jsx"]) {
    const p = join(srcDir, file);
    assert.equal(existsSync(p), true, `${file} should exist`);
    const content = readFileSync(p, "utf8");
    assert.match(content, /LocalStorage/);
  }
  const inc = readFileSync(join(srcDir, "increment-counter.jsx"), "utf8");
  assert.match(inc, /counter\+\+/);
  const dec = readFileSync(join(srcDir, "decrement-counter.jsx"), "utf8");
  assert.match(dec, /counter--/);
  const reset = readFileSync(join(srcDir, "reset-counter.jsx"), "utf8");
  assert.match(reset, /setItem\("counter", counter\)/);
});

test("package.json declares Windows+macOS and Tezbar metadata", async () => {
  const pkg = await import("../package.json", { with: { type: "json" } });
  const data = (pkg as unknown as { default: Record<string, unknown> }).default ??
    (pkg as unknown as Record<string, unknown>);
  assert.deepEqual((data as { platforms: string[] }).platforms, ["macOS", "Windows"]);
  assert.equal((data as { id: string }).id, "com.tezbar.counter");
  assert.equal((data as { author: string }).author, "almatkai");
  const tezbar = (data as { tezbar: { platforms: string[]; repository: string; upstream: { commit: string; path: string } } }).tezbar;
  assert.deepEqual(tezbar.platforms, ["macos", "windows"]);
  assert.equal(tezbar.repository, "https://github.com/almatkai/counter");
  assert.equal(tezbar.upstream.path, "extensions/counter");
  assert.equal(tezbar.upstream.commit, "6734d85992290066fd31a7f5bee3cca3d3545a1a");
  const commands = (data as { commands: { platforms: string[] }[] }).commands;
  assert.equal(commands.length, 3);
  for (const cmd of commands) assert.deepEqual(cmd.platforms, ["macOS", "Windows"]);
});
