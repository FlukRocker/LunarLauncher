# Lunar Launcher — Tauri migration

Ground-up rewrite of the Electron launcher onto Tauri 2 (Rust backend) with a
React + TypeScript frontend.

**Status: foundation only. This does not launch Minecraft yet.** The Electron
app on `main` remains the shipping launcher. See [Remaining work](#remaining-work).

## Layout

```
tauri/
├── src/                  React + TypeScript frontend
│   ├── lib/api.ts        typed wrappers over the Rust commands
│   ├── components/       Frame (custom titlebar)
│   └── views/            Loading, Welcome, LoginOptions, Landing, FatalError
└── src-tauri/            Rust backend
    └── src/
        ├── paths.rs         launcher/data directory resolution
        ├── config.rs        port of configmanager.js
        ├── distribution.rs  port of helios-core's distribution layer
        ├── commands.rs      the Rust -> JS command surface
        └── error.rs         typed errors crossing the IPC boundary
```

## Commands

```console
npm install
npm run app:dev              # dev: vite + tauri with hot reload
npm run app:build            # production build (bundles installers)
npm run app:build -- --no-bundle   # production binary only, much faster
npm run test:rust            # cargo test
npm run lint                 # tsc --noEmit
```

### Do not use plain `cargo build`

A bare `cargo build` (debug **or** release) produces a binary that points at
`devUrl` — `http://localhost:1420` — instead of the embedded frontend, because
asset embedding is gated behind the `custom-protocol` Cargo feature that only
`tauri build` enables. The window opens completely blank and no `bootstrap`
call ever reaches Rust, which looks alarming but is purely a build-mode
artifact. Always go through `npm run app:build`.

## What is ported and verified

Verified end to end by running the built app against a local distribution
index: config load → distribution fetch → default-server resolution → java
config creation → account selection UI.

- **Config** (`config.rs`) — full port of `configmanager.js`. Reads and writes
  the **existing** `config.json` unchanged, so an Electron install keeps its
  accounts and settings. RAM heuristics, per-server Java defaults (the Java 8
  vs 17+ option sets) and offline-account creation are ported verbatim.
- **Distribution** (`distribution.rs`) — the spec model, remote fetch with
  disk-cache fallback, `mcVersionAtLeast`, address parsing, and the
  platform/architecture precedence rules for `effectiveJavaOptions`.
- **Frontend shell** — view switching (replacing the `VIEWS` map and jQuery
  fades), a custom titlebar using Tauri drag regions instead of the
  Chromium-only `-webkit-app-region`, and offline login wired end to end.

`cargo test` covers the compatibility contract, including a real
Electron-written `config.json` round trip and parsing the shipped
`docs/sample_distribution.json`.

### Config compatibility is load-bearing

`JavaConfig` uses explicit `#[serde(rename = "minRAM")]` / `"maxRAM"`. Serde's
`camelCase` would emit `minRam`/`maxRam`, which fails to deserialise a real
config — and because a parse failure falls back to defaults, that silently
**wipes the user's accounts and settings**. This was a live bug caught in
testing; `electron_written_config_survives_a_round_trip` guards it.

## Remaining work

Roughly in dependency order. The first item is by far the largest.

1. **Download + validation engine.** Port `helios-core`'s `FullRepair`:
   version manifest and asset index resolution, library rule evaluation,
   parallel downloads with hash validation, and progress reporting to the UI.
2. **Process builder.** Port `processbuilder.js` (911 lines): classpath
   assembly, native extraction, Forge manifest handling, and the
   version-dependent JVM argument construction (`_constructJVMArguments112`
   vs `113`). This is the subtlest part — errors here mean the game silently
   fails to launch.
3. **Java discovery.** Port `helios-core/java`: locating installed JVMs,
   validating them against the server's `supported` range, and downloading a
   JDK when none matches.
4. **Microsoft authentication.** The OAuth flow needs a second Tauri window
   plus the MSA → Xbox Live → XSTS → Minecraft token exchange and refresh.
   Currently stubbed out in the UI.
5. **Remaining views.** Settings (the Electron `settings.js` is 1,624 lines,
   covering mods, Java config, accounts and updates), the news feed, and
   drop-in mod management.
6. **Auto-update.** `electron-updater` has no direct equivalent; use
   `tauri-plugin-updater`, which needs a signing key and an update manifest.
7. **Discord RPC**, currently absent.
8. **Visual parity.** `app/assets/css/launcher.css` has ~50 `-webkit-` rules
   (scrollbar styling, filters, `app-region`) that are Chromium-only. Tauri
   renders in WKWebView / WebView2 / WebKitGTK, so each needs a
   standards-based replacement. The current CSS is a minimal baseline, not a
   port.

## Known constraints

- The distribution host `hermes-mc.net` currently returns NXDOMAIN, so a fresh
  start hits the fatal-error screen. For development, drop a valid
  `distribution.json` into the launcher directory
  (`~/Library/Application Support/Lunar Launcher/` on macOS) — the loader
  falls back to that local copy. `docs/sample_distribution.json` works.
- Tauri and Electron share the same config path, so running both against one
  machine is fine, but a regression in the Rust config layer can affect the
  Electron install. Back up `config.json` when testing changes to `config.rs`.
