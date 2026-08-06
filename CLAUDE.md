# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project

Lunar Launcher — a modded Minecraft launcher built with Tauri 2 (Rust) and
React + TypeScript. It began as a fork of
[HeliosLauncher](https://github.com/dscalzi/HeliosLauncher) (Electron + Node);
the Electron implementation was removed in favour of a full Rust rewrite. The
last commit containing it is `9e7f5b0` — useful as a reference when porting
remaining features, since much of the Rust is a direct port of specific JS
files and the commit messages name them.

Requires Node 22, a Rust toolchain, and the platform Tauri prerequisites.

## Commands

```console
npm install
npm run app:dev                    # vite + tauri, hot reload
npm run app:build                  # production build with installers
npm run app:build -- --no-bundle   # production binary only, much faster
npm run brand                      # write the tauri config patch from .env
npm run app:build:branded          # branded build (see Build-time configuration)
npm run test:rust                  # cargo test
npm run lint                       # tsc --noEmit

# One test, or the network/launch integration tests (ignored by default):
cargo test --manifest-path src-tauri/Cargo.toml <name>
cargo test --manifest-path src-tauri/Cargo.toml -- --ignored --nocapture
```

### Build-time configuration

`.env` (git-ignored; `.env.example` is the template) feeds three consumers,
all with the same precedence — real environment, then `.env.local`, then
`.env`:

- **`build.rs`** bakes the values in `BAKED` into the binary via
  `cargo:rustc-env`, so `option_env!("LUNAR_DISTRO_URL")` sees them. This is
  what lets a controller-built launcher work on a customer's machine, where
  nothing sets an environment variable.
- **`scripts/brand.mjs`** writes `src-tauri/tauri.brand.json`, a config patch
  Tauri deep-merges over `tauri.conf.json`. It covers what `option_env!`
  cannot reach: product name, bundle identifier, version, updater endpoint,
  icons. `npm run app:build:branded`.
- **`vite.config.ts`** substitutes `__BRAND_NAME__` into the frontend, so the
  Windows titlebar follows the brand.

Runtime still wins over baked, so a branded build can be pointed at a staging
index without rebuilding. The exception is `LUNAR_AZURE_CLIENT_ID`, which is
build-time only — it identifies the application to Microsoft, and a stray
environment variable should not change who the user consents to.

`brand.mjs` fails the build rather than falling back on anything invalid: an
unattended per-customer build that quietly shipped as "Lunar Launcher" would
look entirely correct. It also refuses an updater endpoint with no public key,
which would otherwise install whatever that endpoint returned.

Debug builds also read `.env` at *startup*, which is what makes
`npm run app:dev` work without exporting anything. Release builds ignore a
stray `.env` unless `LUNAR_ENV_FILE` names one, since a dropped file could
otherwise repoint a shipped launcher at another distribution index.

### Two traps worth knowing before you debug anything

- **Never use plain `cargo build`.** Debug *or* release, it yields a binary
  pointing at `devUrl` rather than the embedded frontend, because asset
  embedding is gated behind a Cargo feature only `tauri build` sets. The window
  opens blank and no command reaches Rust. It looks like a catastrophic
  failure and is purely a build-mode artifact.
- **`hermes-mc.net` no longer resolves,** so a default start hits the fatal
  error screen. Use `LUNAR_DISTRO_URL` (an `http(s)://` URL, `file://` URL, or
  plain path) — `dev-distribution.json` is an unmodded 1.20.1 server that
  exercises the full launch path.

## Architecture

The webview renders and calls commands; everything privileged happens in Rust.
There is no Node runtime in the shipped app, which is the fundamental
difference from the Electron original and the reason each module below had to
be a real port rather than a wrapper.

- **`config.rs`** — the persisted `config.json`. Deliberately byte-compatible
  with what Electron wrote so existing installs keep their accounts.
- **`distribution.rs`** — the distribution index: spec model, remote fetch with
  disk-cache fallback, `mcVersionAtLeast`, and the platform/architecture
  precedence rules that resolve a server's effective Java options.
- **`java.rs`** — JVM discovery. Candidates are *executed* to read their real
  properties, because a directory name says nothing reliable about
  architecture or vendor. Filters to 64-bit and rejects x86 JVMs on arm64 hosts
  so nothing runs under Rosetta.
- **`dl.rs`** — version manifest → version JSON → asset index, then SHA1
  validation of every asset, library and the client jar. Parallel downloads
  with retries and atomic temp+rename writes.
- **`process_builder.rs`** — classpath, native extraction, and JVM arguments
  for both the 1.13+ structured form and the pre-1.13 flat form. The subtlest
  code here; mistakes produce a game that silently fails to start.
- **`microsoft.rs`** — auth code → MS token → Xbox Live → XSTS → Minecraft
  token → profile, plus refresh. XSTS error codes map to actionable messages.
- **`commands.rs`** — the IPC surface. Keep commands coarse-grained: one
  meaningful operation each, not getters, since every call crosses a boundary.

`src/lib/api.ts` mirrors these types by hand. Change a Rust struct that crosses
the boundary and you must update it there too — nothing enforces this.

### Config compatibility is load-bearing

`JavaConfig` carries explicit `#[serde(rename = "minRAM")]` / `"maxRAM"`.
Serde's `camelCase` renders these as `minRam`/`maxRam`, which fails to
deserialise a real config — and since a parse failure falls back to defaults,
that silently wipes the user's accounts and settings. This was a live bug found
in testing. `electron_written_config_survives_a_round_trip` guards it; do not
"simplify" those renames away.

### Launch flow

`launch_game` in `commands.rs` is the whole path: resolve version → validate →
download what's missing → locate a compatible JVM → build arguments → spawn.
Progress is pushed to the frontend as `launch://progress` events rather than
returned, because the download is often multi-gigabyte.

It refuses servers whose distribution declares Forge/Fabric, since mod loaders
are not ported. That refusal is deliberate — starting a modded server without
its loader produces a confusing failure deep in the game rather than a clear
message.

## Conventions

Rust is standard rustfmt. TypeScript follows the existing files: 4-space
indent, single quotes, no semicolons.

When porting the remaining features, prefer faithfulness to the original JS
over improving it, and say so in a comment where the original did something
surprising — several ported functions reproduce quirks (the first-matching-rule
early return in library rules, the fullscreen argument rewrite) that look like
bugs but are what the game expects.
