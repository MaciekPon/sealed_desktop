# Sealed (Desktop)

Encrypted, wallet-based messaging on Algorand — Windows/macOS desktop client. A Tauri v2 + React + TypeScript port of the Sealed mobile app.

## Download

Grab the latest build from the [Releases](../../releases) page — pick the installer for your platform (Windows `.msi`/`.exe`, macOS `.dmg`).

**These are unsigned test builds.** Your OS will warn you before running one:
- **Windows**: SmartScreen will say "Windows protected your PC" — click **More info** → **Run anyway**.
- **macOS**: Gatekeeper will block it on first launch — right-click the app → **Open**, or go to **System Settings → Privacy & Security → Open Anyway**.

This is expected for an unsigned build, not a sign of tampering — but only download builds from this repo's official Releases page.

## What it does

Messages are end-to-end encrypted (X25519 + ML-KEM-512 hybrid, post-quantum-resistant) and sent as Algorand blockchain transactions, so there's no central server holding your conversations. Network traffic to Algorand/indexer nodes is relayed through OHTTP so the relay never sees your IP alongside your requests.

## Development

```bash
npm install
npm run tauri dev    # run in dev mode
npm run tauri build   # produce a release build
```

Requires Rust + the [Tauri prerequisites](https://tauri.app/start/prerequisites/) for your platform.

### Known dev-environment limitation (Windows)

**Windows Smart App Control** can block `cargo check` / `npm run tauri dev` on a stock Windows 11 machine with Smart App Control in Enforce mode (`HKLM:\SYSTEM\CurrentControlSet\Control\CI\Policy\VerifiedAndReputablePolicyState = 1`). The `tauri-build` build script loads a freshly-compiled, unsigned `zerofrom_derive` proc-macro DLL, which Smart App Control refuses to load (`os error 4551`).

There is no safe in-place fix: disabling Smart App Control is effectively a one-way operation on Windows (Microsoft does not provide a supported way to re-enable it without resetting/reinstalling Windows). If you hit this, either build on a machine without Smart App Control enforced, or accept the risk and disable it yourself via Windows Security settings.

## Recommended IDE Setup

- [VS Code](https://code.visualstudio.com/) + [Tauri](https://marketplace.visualstudio.com/items?itemName=tauri-apps.tauri-vscode) + [rust-analyzer](https://marketplace.visualstudio.com/items?itemName=rust-lang.rust-analyzer)
