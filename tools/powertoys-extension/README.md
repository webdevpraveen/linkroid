# Linkroid PowerToys Command Palette / PowerToys Run Extension (`linkroid-ptrun`)

`linkroid-ptrun` is a standalone, zero-dependency Rust helper that connects Windows launcher tools (PowerToys Command Palette, PowerToys Run, Flow Launcher, Raycast for Windows) to Linkroid's [Local Authenticated API](../../docs/api.md).

## Building

```powershell
cargo build --release --manifest-path tools/powertoys-extension/Cargo.toml
```

## Prerequisites

1. Open **Linkroid → Settings → Local automation API** and turn on **Enable local automation API**.
2. Linkroid writes `%LOCALAPPDATA%\Linkroid\api-token.json` containing the loopback port (`53320`) and a 256-bit Bearer token.

## Usage

### Querying Palette Items

```powershell
linkroid-ptrun query ""
linkroid-ptrun query "ring"
linkroid-ptrun query "https://linkroid.dev"
```

Outputs a JSON array of palette commands (`send-clipboard`, `ring`, `camera` `photo`/`scan`, `send-url`, `send-text`, plus recent timeline items).

### Invoking an Action

```powershell
linkroid-ptrun invoke ring
linkroid-ptrun invoke send-clipboard
linkroid-ptrun invoke camera photo
linkroid-ptrun invoke camera scan
linkroid-ptrun invoke send-url "https://linkroid.dev"
linkroid-ptrun invoke send-text "Copied from PowerToys"
linkroid-ptrun invoke send-file "C:\Users\Alice\Desktop\notes.pdf"
```
