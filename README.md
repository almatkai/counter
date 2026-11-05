# Counter for Tezbar

The standalone Rust Native V2 implementation and integration notes are in [v2/README.md](v2/README.md).
The installable native Rust package is described in [Native Rust installable version](#native-rust-installable-version).

A simple counter with live subtitle updates in Tezbar on Windows or macOS.

## Commands

- **Increment Counter** — increments the stored value, persists via `LocalStorage`, and updates the decrement command subtitle via `launchCommand` background refresh.
- **Decrement Counter** — decrements the stored value and refreshes the increment command subtitle.
- **Reset Counter** — sets the counter to `0` and refreshes both increment and decrement subtitles.

The counter value is synchronized across commands through `updateCommandMetadata({ subtitle: "Value: N" })` and background `launchCommand` calls with `noUpdate` context to avoid loops.

## Compatibility

- Windows: built and exercised in Tezbar on Windows 11.
- macOS: uses only cross-platform `@raycast/api` (`LocalStorage`, `updateCommandMetadata`, `launchCommand`) and retains the upstream macOS-compatible design.
- Linux: not currently declared.

## Local verification

```powershell
pnpm install
pnpm typecheck
pnpm test
pnpm build
```

## Credits

Adapted for Tezbar from Raycast's MIT-licensed `counter` extension at upstream commit `6734d85992290066fd31a7f5bee3cca3d3545a1a`. Original author `EvanZhouDev` and contributor `pernielsentikaer` remain credited in `package.json`.

## Native Rust installable version

This repository also ships Counter as an installable Tezbar **schema 3 process
extension** (`com.tezbar.native.counter`, version 1.0.0, `execution: process`,
`protocolVersion: 1`, `replaces: ["raycast.counter"]`). The original Raycast
TypeScript/JSX source in `src/`, its `package.json` commands, icon and license are
preserved unchanged; the native package lives alongside it:

| Path | Purpose |
| --- | --- |
| `extension.json` | Schema 3 package manifest (validated against `schemas/extension-v3.schema.json` in tests) |
| `Cargo.toml`, `Cargo.lock` | Executable crate `tezbar-counter`; SDK pinned to `tezbar-extension-sdk-rust` rev `aaec571673ab64a85a136cf6bd054ef660c4d93c` |
| `native/` | `main.rs` (SDK `serve()` adapter), `host.rs` (maps the V2 `Host` trait to SDK private storage), `migration.rs` |
| `v2/` | Reused, unchanged Native V2 engine (`tezbar-counter-v2`) and its tests |
| `tests/process.rs` | Real-executable JSONL protocol tests with a fake narrow host |
| `scripts/package.py`, `scripts/smoke-package.py` | Build ZIP + SHA sidecars; offline ZIP smoke |
| `resources/`, `licenses/`, `NOTICE` | Packaged icon (byte-identical to `command-icon.png`) and notices |

### Download and install

[Native Rust 1.0.0 release](https://github.com/almatkai/counter/releases/tag/v1.0.0): [macOS Apple Silicon ZIP](https://github.com/almatkai/counter/releases/download/v1.0.0/counter-aarch64-apple-darwin.zip), [Windows x64 ZIP](https://github.com/almatkai/counter/releases/download/v1.0.0/counter-x86_64-pc-windows-msvc.zip), and [trusted install reference](https://github.com/almatkai/counter/releases/download/v1.0.0/extension-release.json).

Download the trusted JSON and your platform ZIP, then import them with:

```sh
tezbar --extensions-v3-import extension-release.json counter-<target>.zip
```

Both native builds, extracted-package protocol checks and Tezbar CLI imports into isolated profiles passed. [Validation and exact hashes](https://github.com/almatkai/counter/releases/download/v1.0.0/validation.json) record the tested source. Real desktop clipboard, selection and focus QA remains pending. Store discovery requires a Tezbar catalog update.

### Behavior

All three original commands are implemented natively by the existing V2 engine:
increment/decrement read the stored counter with JavaScript `parseInt`
semantics, add or subtract one and persist it; reset stores `0`.
`launchContext.noUpdate` (JavaScript truthiness) makes increment/decrement read
only, as in the source. Each command returns a no-view result with message
`Value: N`. Invalid stored values or malformed storage return an explicit error and
are never overwritten (see `v2/README.md`).

The executable requests **no host capabilities** (`capabilities: []`): storage is
the SDK's private, transactional `<dataDir>/counter/storage.json`. Overlapping
sessions serialize through the SDK store lock, so concurrent runs never lose an
update.

**Known gap — dynamic subtitle:** the source calls
`updateCommandMetadata({subtitle: "Value: N"})`. Protocol 1 has no command
metadata host API, so the live launcher subtitle is not updated; the value is shown
as the no-view message instead and the manifest subtitle stays the static
`Value: 0`. Increment and decrement declare this as a `gap` in `extension.json`.

### Migration

On initialize, if the private store has no counter and no migration marker, the
value is imported once from the host-granted `migrationPaths`, in order:
`nativeStore` (`native-extensions-v2/counter/storage.json`, all keys kept), then the
JavaScript runtime's LocalStorage shim files (`extensions/counter/`,
`extensions/raycast.counter/`, `extensions/packages/raycast.counter/`, `extension-registry/packages/raycast.counter/`
`.tezbar-local-storage.json`, `counter` key only, text kept for `parseInt`).
Existing private counters and completed migration markers always win, including an intentional zero.
Sources are only read, never modified. A malformed source is skipped and the
import retried on a later start. The marker `_tezbarMigration` records the source.

### Build, test, package

```sh
export CARGO_TARGET_DIR="$PWD/target"
cargo test -j2 --locked                 # adapter, migration and real-process tests (Rust >= 1.88 for dev-deps)
cargo clippy -j2 --locked --all-targets -- -D warnings
(cd v2 && CARGO_TARGET_DIR="$PWD/target" cargo test -j2 --locked)   # preserved V2 behavior suite
cargo build -j2 --locked --release --target aarch64-apple-darwin  # executable builds on Rust >= 1.85
python3 -I scripts/vendor-licenses.py --check  # both targets; locked normal/build closure + MSRV
python3 -I scripts/package.py --target aarch64-apple-darwin \
  --binary target/aarch64-apple-darwin/release/tezbar-counter
python3 -I scripts/smoke-package.py dist/counter-aarch64-apple-darwin.zip \
  --open increment-counter="Value: 1" --open decrement-counter="Value: 0" --open reset-counter="Value: 0"
```

`dist/` receives `counter-<target>.zip`, `.zip.sha256`, `.artifact.json`
(archive + executable digests), `.executable.sha256` and `.files.json`. The ZIP
root holds `extension.json` (complete `files` inventory, empty `artifacts`),
`bin/tezbar-counter[.exe]`, `resources/`, `licenses/`, `LICENSE` and `NOTICE`.
Windows x64 (`x86_64-pc-windows-msvc`) uses the same scripts on a Windows host.

Packaging first checks `scripts/vendor-licenses.py --check`. Exact upstream
LICENSE/NOTICE/COPYRIGHT documents (including the SDK LICENSE and NOTICE) for
both targets are bundled under `licenses/crates/`, with hashes and target
membership in `licenses/dependency-license-index.json`. Regenerate with
`python3 -I scripts/vendor-licenses.py` after an intentional lockfile change.
The script also rejects release dependencies whose declared MSRV exceeds 1.85;
the macOS release is built with Rust 1.85, and Windows compilation is central QA.

Verification status: native macOS arm64 and Windows x64 builds, offline process/package checks and isolated CLI installs passed. Launcher UI and actual OS effects have not been verified on either platform.
