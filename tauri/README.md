# Lunar Launcher — Tauri migration

Ground-up rewrite of the Electron launcher onto Tauri 2 (Rust backend) with a
React + TypeScript frontend.

**Status: vanilla Minecraft launches; modded servers do not.** The Electron app
on `main` remains the shipping launcher. See [Remaining work](#remaining-work).

## Layout

```
tauri/
├── src/                  React + TypeScript frontend
│   ├── lib/api.ts        typed wrappers over the Rust commands
│   ├── components/       Frame (custom titlebar)
│   └── views/            Loading, Welcome, LoginOptions, Landing, FatalError
└── src-tauri/            Rust backend
    └── src/
        ├── paths.rs            launcher/data directory resolution
        ├── config.rs           port of configmanager.js
        ├── distribution.rs     port of helios-core's distribution layer
        ├── java.rs             JVM discovery and version-range matching
        ├── dl.rs               Mojang download + SHA1 validation engine
        ├── process_builder.rs  classpath, natives and JVM arguments
        ├── commands.rs         the Rust -> JS command surface
        └── error.rs            typed errors crossing the IPC boundary
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

### Running against a local distribution

`hermes-mc.net` is a stale domain. Point the launcher at any local or
alternative index with `LUNAR_DISTRO_URL`, which accepts an `http(s)://` URL, a
`file://` URL, or a plain path:

```console
LUNAR_DISTRO_URL=../docs/sample_distribution.json npm run app:dev
```

When the new production domain is known, update `REMOTE_DISTRO_URL` in
`src-tauri/src/distribution.rs`.

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
- **Java discovery** (`java.rs`) — enumerates JVM roots from `JAVA_HOME`,
  conventional platform directories and the launcher's own runtime dir, then
  executes each candidate to read its real properties. Filters to 64-bit, and
  on arm64 hosts rejects x86 JVMs so nothing launches under Rosetta. Version
  ranges (`8.x`, `>=17.x`) are matched by a small parser covering the subset
  distribution indexes actually use.
- **Download engine** (`dl.rs`) — the Mojang pipeline: version manifest →
  version JSON → asset index, then SHA1 validation of every asset, library and
  the client jar, with parallel downloads, retries, and atomic writes. Library
  platform rules and native classifiers are ported verbatim. Progress streams
  to the UI over the `launch://progress` event.
- **Process builder** (`process_builder.rs`) — classpath assembly, native
  extraction, and JVM argument construction for both the 1.13+ structured form
  and the pre-1.13 flat form, including conditional-argument rule evaluation
  and full `${...}` placeholder substitution.
- **Frontend shell** — view switching (replacing the `VIEWS` map and jQuery
  fades), a custom titlebar using Tauri drag regions instead of the
  Chromium-only `-webkit-app-region`, offline login, and a PLAY button wired to
  the real pipeline with a live progress bar.

### How far the launch path is verified

An ignored integration test (`cargo test -- --ignored`) runs the real thing
against Mojang's live servers: it resolves 1.20.1, downloads all ~3,650 files
(~700 MB), re-validates to confirm nothing is left, locates a JDK, and spawns
the game. It asserts the classpath resolves and the JVM accepts the arguments.

It deliberately does **not** assert that a game window appears. Once control
passes to GL/AppKit the outcome depends on the display session the test runs
under, so asserting on it would be flaky rather than meaningful. **A human
should confirm the game actually reaches the main menu before this is
considered done.**

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

1. **Mod loader support.** The single biggest gap. Forge/Fabric need manifest
   resolution, the Forge installer/processor pipeline, merged classpaths and
   loader-specific arguments. `launch_game` currently refuses a server whose
   distribution declares a loader rather than starting a broken game — most
   real Lunar servers are modded, so this is what stands between the Tauri
   build and actual use.
2. **Distribution module downloads.** `DistributionIndexProcessor` — the mods
   and files a server declares, as opposed to the vanilla Mojang assets, which
   are done.
3. **JDK auto-download.** Discovery is done; fetching a JDK when none matches
   (Temurin/Corretto per `effectiveJavaOptions`) is not, so the user must have
   a suitable JDK installed.
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

- The distribution host `hermes-mc.net` is stale and returns NXDOMAIN. Use
  `LUNAR_DISTRO_URL` (above) for development, or drop a `distribution.json`
  into the launcher directory (`~/Library/Application Support/Lunar Launcher/`
  on macOS) — the loader falls back to that cached copy.
- Tauri and Electron share the same config path, so running both against one
  machine is fine, but a regression in the Rust config layer can affect the
  Electron install. Back up `config.json` when testing changes to `config.rs`.
