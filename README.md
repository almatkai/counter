# Counter for Tezbar

The standalone Rust Native V2 implementation and integration notes are in [v2/README.md](v2/README.md).

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
