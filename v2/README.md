# Counter — Native V2

Standalone Rust library for all three source commands: `increment-counter`,
`decrement-counter`, and `reset-counter`. Intended platforms are macOS and Windows.
This package requires host integration; it has not been
registered, installed, or exercised in either platform's launcher UI.

The original command icon is embedded as `ICON`, unchanged (23,168 bytes;
SHA-256 `06961936531a678d738f83befba4ff04cfe43c9b53de8aca6d758aa229c9e3be`).
`MANIFEST` embeds `manifest.json`; `commands()` reports compiled command coverage.
Coverage of counter operations does not imply live launcher subtitles work.

## Behavior and compatibility

Increment/decrement read the shared counter, apply JavaScript `parseInt`, add or
subtract one using binary64 arithmetic, and persist the result. A missing or null
counter defaults to zero. Numeric JSON values first undergo JavaScript
`Number.toString` formatting via `ryu-js`: a stored number `1e21` parses as
`parseInt("1e+21") === 1`, then increments to 2. Strings accept the decimal or
signed hexadecimal numeric prefix, leading ECMAScript whitespace, and trailing
text. Values beyond the exact integer range round like JavaScript numbers;
there is no checked i64 arithmetic or integer saturation.

Deliberate validation difference: booleans, objects, arrays, strings without an
integer prefix, and non-finite parse/results return an explicit error. V1 can
produce NaN (later JSON serialization can turn it into null). V2 never persists
NaN/null on failure. Reset always writes zero, so it repairs an invalid counter
inside an otherwise valid object, preserving other keys. Malformed JSON or a
non-object store errors, including on reset, and is never replaced with defaults.

The host payload is the actual native run-command body with preferences added.
Only `payload.launchContext.noUpdate` controls the increment/decrement read-only
path. JSON truthiness follows JavaScript: nonempty strings (including `"false"`),
objects, arrays, and nonzero numbers are true; null, false, zero, empty string,
and missing values are false. A true value reads without an update transaction,
without rewriting existing bytes, and without creating a missing store or directory.
Reset ignores launch context, as the original source does. The beta launcher UI
does not currently expose launchContext; this contract supports supplied native
payloads without adding a worker or background-launch route.

Each successful command returns `{ok:true, mode:"no-view", message:"Value: N",
value:N}`. This native message/HUD is an **explicit UI adaptation**: the actual
increment/decrement source calls `updateCommandMetadata({subtitle:"Value: N"})`
and reset only stores zero. None of the three actual source files launches
background siblings, shows a HUD, accesses the clipboard, pastes, opens a browser,
or closes a window. The legacy root README's background-refresh description does
not describe those source files.

In pinned beta `fd300e276356587726566026f072563eb9c34cb1`,
`src/main/extensions/raycastShim.ts` implements `updateCommandMetadata` as a no-op;
`v2_sdk.rs` has no metadata operation; launcher registry rows derive subtitles
from command descriptions. Dynamic launcher subtitles were not already working
in this beta. Preserved manifest `subtitle: "Value: 0"` fields are static source
metadata only. `command_metadata(f64)` is a pure exported helper for future host
integration; nothing calls an invented metadata RPC. Live root subtitles need
host metadata storage/registry/renderer wiring and an app rebuild, outside this
repository's implementation scope. Native message rendering also needs real UI QA.

## Storage and API

`handle(host, command, operation, payload, state) -> Result<Value, String>` supports
`extension:run-command` and harmless `extension:dispose-session`. Unknown
operations/commands error without storage effects. No session/view is created;
`state` is untouched. There are no OS requests. Host `Err` and `{ok:false,error}`
or `{ok:false,message}` replies propagate as errors without a success value/HUD.

`Host::read_store` and `Host::update_store` return
`{ok:true, store:<namespace object>}` on success. The response envelope is separate
from stored data, so unknown namespace keys such as `ok`, `error`, and `message`
cannot be misidentified as a host failure.
`StoreChange` is `Box<dyn FnOnce(&mut Value) -> Result<(), String> + Send + 'static>`.
The update implementation must read, apply, and persist as one transaction shared
by all contexts; failed changes/writes must preserve the previous store. The
concrete `tezbar_adapter.rs` maps to the actual beta `CoreContext` and
`v2_sdk::{read_store,update_store}` with slug **counter**. The SDK's global
`STORE_LOCK` serializes in-process transactions across contexts and uses the real
`write_json_atomic` implementation. Successful results use their own committed
snapshot, so later updates cannot change the value reported by that command.
This is in-process linearizability; the beta SDK does not supply a cross-process
file lock.

Data lives under `<data_dir>/native-extensions-v2/counter/storage.json`, isolated
from `count-numbers`, V1 storage, preferences, and installed packages. Unknown
namespace keys survive writes.
There is no invented V1 storage migration.

## Build and tests

Rust 1.85 or newer; `Cargo.lock` is included. Normal Cargo builds/tests need no
Node runtime, app checkout, profile, or OS integration. Run from `v2`:

```sh
cargo build --locked
cargo fmt --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
cargo +1.85.0 test --locked
cargo check --locked --all-targets --target x86_64-pc-windows-gnu
cargo clippy --locked --all-targets --target x86_64-pc-windows-gnu -- -D warnings
cargo package --locked --allow-dirty
```

Cross-compilation checks are compile validation, not Windows execution or UI QA.
Tests compile the concrete wrapper against **unmodified pinned beta common.rs
and v2_sdk.rs**, stubbing only Tauri AppHandle/Emitter and rejecting OS router
requests. Temporary CorePaths test persistence, restarts, namespaces, corruption,
callback rollback, write failures, and concurrent operations across independent
contexts, including increment/decrement/reset history linearizability with
real-time ordering. Vendored SDK/common tests also run.

`tests/oracle.json` records values, persisted state, writes, and command metadata
from the three actual source files. Rust checks those separately from its native
message adaptation. Optional Node verification executes the verbatim source
fixtures in a VM with fake LocalStorage/updateCommandMetadata, removing only
import/export syntax:

```sh
node tests/oracle.mjs
# Only after intentional source/expectation review:
node tests/oracle.mjs --write
```

The crate archive includes source fixtures and all test/license assets; extract
it and run `cargo test --locked` to verify it independently. See fixture
[provenance](tests/fixtures/PROVENANCE.md). macOS and Windows OS/UI verification
remain `not-run` in the manifest. The pinned beta schema and source fixture
metadata are checked in tests; source icon hash is checked separately.

## Host integration (not performed here)

The following are explicit changes to make in a separately authorized Tezbar
checkout based on pinned beta; this package never edits the app or installed
profile. Vendor the published repository's `v2/` directory at the stable host
path `src-tauri/native-extensions/counter/`, preserving its licenses and fixtures.

1. In `src-tauri/Cargo.toml`, add:

   ```toml
   tezbar-counter-v2 = { path = "native-extensions/counter" }
   ```

2. Copy the vendored `tezbar_adapter.rs` to
   `src-tauri/src/native_core/extensions/v2_counter.rs`, beside `v2_sdk.rs`.
   Declare `mod v2_counter;` in that directory's `mod.rs`.
3. In `src-tauri/src/native_core/extensions/v2_runtime.rs`, add `"counter"` to
   `slug()`'s allowlist (canonical and `raycast.counter` aliases are already
   stripped there). Add `"counter" => super::v2_counter::commands(),` to
   `commands()` and this match arm to `handle()`:

   ```rust,ignore
   "counter" => super::v2_counter::handle(ctx, command, operation, payload, state).await,
   ```

4. Append the **object** from the vendored `manifest.json` to the array in
   `src-tauri/src/native_core/extensions/v2_catalog.json`. The existing catalog
   derives runtime coverage from compiled dispatch; copying JSON alone cannot
   register executable Rust. No counter startup worker is needed.
5. Copy the unchanged vendored `assets/command-icon.png` to
   **`src/renderer/public/extensions/icons/counter.png`**. Vite's publicDir is
   `src/renderer/public`; this serves the required manifest URL
   `/extensions/icons/counter.png`. Do not put it at repository-root `public`.
6. Rebuild the host and frontend. Use the rebuilt native catalog/install flow
   for `com.tezbar.native.counter` and verify all three commands and the value
   message on both platforms. The current store cannot directly install this
   standalone Rust source package or supply the missing dispatch. Installed
   V1 storage is left in place for fallback.

Dynamic launcher subtitle support is additional host work described above;
registration alone does not implement it. No UI, installation, or metadata
integration verification is claimed here.

## Credits and licenses

Original Raycast author EvanZhouDev; contributor pernielsentikaer; Tezbar port
almatkai. Upstream `raycast/extensions` commit
`6734d85992290066fd31a7f5bee3cca3d3545a1a`, `extensions/counter`.
The original icon and extension-derived code are MIT; `LICENSE` contains the
2021 Raycast and 2026 almatkai notices with actual newlines. Root V1 files and
license remain unchanged. Pinned Tezbar fixture code/schema are Apache-2.0 with
the original license in `tests/fixtures/beta/LICENSE`. The complete crate's SPDX
expression is `MIT AND Apache-2.0` because it distributes both sets of material.
