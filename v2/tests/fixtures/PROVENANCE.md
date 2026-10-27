# Fixture provenance

All fixtures are public source material. They contain no profile/user data.

## Tezbar beta — Apache-2.0

Repository: https://github.com/almatkai/Tezbar
Commit: `fd300e276356587726566026f072563eb9c34cb1`.
Copied byte-for-byte via `git show <commit>:<path>`; no source patches.

| Fixture | Original path | SHA-256 |
| --- | --- | --- |
| `beta/common.rs` | `src-tauri/src/native_core/common.rs` | `280a0a2f0dd04a8570b320aafc0c3ece0f5de8ddb21c363645567d6a35d84da7` |
| `beta/v2_sdk.rs` | `src-tauri/src/native_core/extensions/v2_sdk.rs` | `4f987346f863db9a7bd6aa1d5a99e149925dc69a44d629727ccc80a567cd27c2` |
| `beta/extension-v2.schema.json` | `schemas/extension-v2.schema.json` | `6d5afda1b652605982104bdf16d1547547b98dfd707a7b2969316406c49f32af` |
| `beta/LICENSE` | `LICENSE` | `0ed6b1b2760294d4aaaa45be38e166fcc0168e1dd5190423c096fcbe1c94afbf` |

Only Tauri AppHandle/Emitter and the OS router are test stubs. Real CorePaths,
CoreContext, ConfigStore, atomic writing, SDK storage locking/reading/transactions,
and the concrete adapter compile together, including the vendored unit tests.
`beta/LICENSE` is the original Apache-2.0 license.

## Counter source — MIT

Repository: https://github.com/almatkai/counter
Commit: `25dbd381b02c344b67f522d1b480412c3bb9b94e`.
`v1/{increment-counter,decrement-counter,reset-counter}.jsx` are unchanged copies
of `src/` source files at that commit; `v1/package.json` is the actual source
metadata. `v1/LICENSE` carries the source-derived MIT notices. The Node VM oracle
executes these commands with fake LocalStorage and updateCommandMetadata and
records actual results in `tests/oracle.json`. It does not implement assumed
background calls from the root README.

Original upstream: https://github.com/raycast/extensions/tree/6734d85992290066fd31a7f5bee3cca3d3545a1a/extensions/counter
Original author EvanZhouDev; contributor pernielsentikaer; Tezbar author almatkai.

`assets/command-icon.png` is copied unchanged from the original pinned package's
`extensions/counter/assets/command-icon.png`, matching both root icon copies:
23,168 bytes; SHA-256
`06961936531a678d738f83befba4ff04cfe43c9b53de8aca6d758aa229c9e3be`.
The icon and source-derived code are MIT, with 2021 Raycast and 2026 almatkai
notices in the crate LICENSE. Root V1 license has not been rewritten.
