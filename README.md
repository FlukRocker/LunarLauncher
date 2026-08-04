<h1 align="center">Lunar Launcher</h1>

<p align="center">Join modded servers without worrying about installing Java, Forge, or other mods. We'll handle that for you.</p>

Built with [Tauri 2](https://tauri.app) (Rust) and React + TypeScript.
Originally a fork of [HeliosLauncher](https://github.com/dscalzi/HeliosLauncher)
by Daniel Scalzi, which was Electron-based; the Electron implementation was
removed in favour of this one. Its history is preserved in git — the last
commit containing it is `9e7f5b0`.

## Status

**Vanilla Minecraft launches. Modded servers do not yet.**

Working: offline and Microsoft login, distribution loading, asset/library
validation and download, Java discovery, JVM launch, settings, Discord Rich
Presence.

Not yet ported: Forge/Fabric mod loaders, distribution module downloads, JDK
auto-download, the news feed, drop-in mod management, and full visual parity.
See [Remaining work](#remaining-work).

**If you need to launch a modded server today, use the last Electron release or
check out `9e7f5b0`.**

## Development

**Requirements:** [Node.js](https://nodejs.org) 22, a [Rust](https://rustup.rs)
toolchain, and the [Tauri prerequisites](https://tauri.app/start/prerequisites/)
for your platform.

```console
npm install
npm run app:dev                    # vite + tauri with hot reload
npm run app:build                  # production build with installers
npm run app:build -- --no-bundle   # production binary only, much faster
npm run test:rust                  # cargo test
npm run lint                       # tsc --noEmit
```

### Running against a local distribution

The launcher needs a distribution index describing the servers it can launch.
`LUNAR_DISTRO_URL` overrides the configured remote and accepts an `http(s)://`
URL, a `file://` URL, or a plain path:

```console
LUNAR_DISTRO_URL=./dev-distribution.json npm run app:dev
```

`dev-distribution.json` declares a single unmodded 1.20.1 server, which
exercises the whole launch path end to end.

The production URL lives in `REMOTE_DISTRO_URL` in
`src-tauri/src/distribution.rs`. It currently points at `hermes-mc.net`, which
no longer resolves and needs updating. See [docs/distro.md](docs/distro.md) for
the index format and [Nebula](https://github.com/dscalzi/Nebula) for generating
one.

### Do not use plain `cargo build`

A bare `cargo build` (debug *or* release) produces a binary that points at the
dev server instead of the embedded frontend, because asset embedding is gated
behind a Cargo feature that only `tauri build` enables. The window opens
completely blank and no command ever reaches Rust. Always go through
`npm run app:build`.

## Architecture

```
src/                  React + TypeScript frontend
├── lib/api.ts        typed wrappers over the Rust commands
├── components/       Frame (custom titlebar)
└── views/            Loading, Welcome, LoginOptions, Landing, Settings, FatalError

src-tauri/src/        Rust backend
├── paths.rs             launcher/data directory resolution
├── config.rs            persisted configuration
├── distribution.rs      distribution index model, fetch and caching
├── java.rs              JVM discovery and version-range matching
├── dl.rs                Mojang download + SHA1 validation engine
├── process_builder.rs   classpath, natives and JVM argument construction
├── microsoft.rs         Microsoft/Xbox/Minecraft authentication chain
├── discord.rs           Rich Presence
├── commands.rs          the Rust -> JS command surface
└── error.rs             typed errors crossing the IPC boundary
```

All privileged work happens in Rust; the webview only renders and calls
commands. There is no Node runtime in the shipped app.

### Configuration compatibility

The launcher reads and writes the same `config.json` the Electron build used,
so an existing installation keeps its accounts and settings. `JavaConfig` uses
explicit `#[serde(rename = "minRAM")]` / `"maxRAM"` for this reason — serde's
`camelCase` would emit `minRam`/`maxRam`, which fails to deserialise a real
config, and a parse failure falls back to defaults, silently wiping the user's
accounts. `electron_written_config_survives_a_round_trip` guards it.

## Microsoft authentication

Sign-in opens the consent page in the user's **default browser** and catches
the authorization code on a short-lived loopback listener, which is the flow
RFC 8252 prescribes for native apps: the address bar stays visible, existing
sessions and password managers work, and it avoids the embedded webviews
Microsoft has been progressively restricting.

This requires `http://127.0.0.1` to be registered as a redirect URI on the
Azure application, under the *Mobile and desktop applications* platform. If it
is not, Microsoft rejects the request and the login screen offers an in-app
window as a fallback, which uses the original `nativeclient` redirect and
needs no extra configuration.

Third-party forks must register their own Azure application and replace
`AZURE_CLIENT_ID` in `src-tauri/src/microsoft.rs`. See
[docs/MicrosoftAuth.md](docs/MicrosoftAuth.md).

### Mojang / Yggdrasil

`authserver.mojang.com` was permanently shut down, so the Mojang login method
only works against a Yggdrasil-compatible endpoint — which is what servers
running authlib-injector, ely.by and similar provide. Point it at yours:

```console
LUNAR_AUTH_SERVER=https://auth.example.com/authserver npm run app:dev
```

Left unset it targets Mojang's old host and fails with a message saying so.

## Remaining work

1. **Mod loader support** — Forge/Fabric manifest resolution, the Forge
   installer/processor pipeline, merged classpaths and loader-specific
   arguments. `launch_game` refuses a server whose distribution declares a
   loader rather than starting a broken game. This is the largest gap and the
   one that blocks real use.
2. **Distribution module downloads** — the mods and files a server declares, as
   opposed to the vanilla Mojang assets, which are done.
3. **JDK auto-download** — discovery works; fetching a JDK when none matches
   does not, so a suitable JDK must already be installed.
4. **Auto-update** — active, with the public key in `plugins.updater.pubkey`.
   Two things remain before it works end to end:

   - **An endpoint.** `plugins.updater.endpoints` still points at a
     placeholder host. It must serve
     `/updates/{{target}}/{{current_version}}`, returning 204 when current and
     a signed manifest otherwise.
   - **Signing in CI.** The private key lives outside the repo
     (`~/.lunar-launcher-keys/lunarlauncher.key`) and is gitignored. Add it as
     the `TAURI_SIGNING_PRIVATE_KEY` secret so release builds are signed.

   Losing that private key means no future update can be signed, and every
   installed launcher stops updating permanently. Back it up somewhere other
   than this machine.
5. **News feed and drop-in mod management.**
6. **Visual parity** — the Electron stylesheet relied on ~50 `-webkit-` rules
   that only work in Chromium. Tauri renders in WKWebView / WebView2 /
   WebKitGTK, so each needs a standards-based replacement. The current styling
   is a clean baseline, not a port.

## Note on third-party usage

Please give credit to the original author of HeliosLauncher and provide a link
to the original source. This is free software, please do at least this much.

`libraries/java/PackXZExtract.jar` is retained for the Forge work in item 1 —
Forge versions up to 1.12 ship `.pack.xz` libraries that need it.
