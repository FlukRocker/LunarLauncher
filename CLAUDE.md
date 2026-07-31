# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project

Lunar Launcher — an Electron-based modded Minecraft launcher. It is a rebranded fork of [HeliosLauncher](https://github.com/dscalzi/HeliosLauncher) by Daniel Scalzi; most files still carry upstream naming (`helios-core`, `SealCircle.png`, WesterosCraft strings). The README is still the upstream one and describes Helios, not this fork.

Node 22 is required (`.nvmrc`, `engines`). There is no test suite.

### Syncing with upstream

The fork shares history with HeliosLauncher (diverged at `ab7e3c3`), so upstream can be merged directly:

```console
git remote add upstream https://github.com/dscalzi/HeliosLauncher.git
git fetch upstream
git merge upstream/master
```

Last synced to upstream `86e4316` (Helios 2.2.1). `main` is published, so prefer merging over rebasing — a rebase would rewrite pushed history and break the `Oi-Dev` branch.

Conflicts recur in predictable places: `package.json` (keep the Lunar identity block, `publish` script, `build.publish`, and `crypto-js`), `_custom.toml` (branding), and `landing.js`. For `package-lock.json`, take upstream's and re-run `npm install` rather than hand-merging.

## Commands

```console
npm start           # run the launcher (electron .)
npm run lint        # eslint over the repo
npm run dist        # build installer for the current platform (output: dist/)
npm run dist:win    # / dist:mac / dist:linux
npm run publish     # electron-builder -p always → GitHub release (needs GH_TOKEN)
```

`.github/workflows/build.yml` runs `npm ci && npm run dist` on every push across macOS/Ubuntu/Windows.

Debugging: launch with the VS Code configs documented in README.md (main process via `node_modules/electron/cli.js`, renderer via `--remote-debugging-port=9222`). In the running app, DevTools is `ctrl + shift + i`.

## Architecture

### Two processes, thin main

`index.js` is the entire main process. It only handles things the renderer cannot: creating the frameless `BrowserWindow`, opening the Microsoft OAuth/logout `BrowserWindow`s and scraping the redirect URI for the auth code, `electron-updater` wiring, `shell.trashItem`, the macOS menu, and `relaunchApp`. All IPC channel names live in `app/assets/js/ipcconstants.js`.

The renderer runs with `nodeIntegration: true` / `contextIsolation: false`, so renderer scripts `require()` Node modules directly. Business logic lives in the renderer, not the main process.

### Views are one page

`app/app.ejs` is the only page loaded. It `include`s every view partial (`welcome`, `login`, `login_lunar`, `waiting`, `loginOptions`, `settings`, `landing`, plus `frame` and `overlay`) into a single DOM. Each partial ends with a `<script src>` tag for its controller in `app/assets/js/scripts/`, so **all view scripts share one global scope** — that is why `.eslintrc.json` disables `no-undef`/`no-unused-vars` for `app/assets/js/scripts/*.js`.

Navigation is not routing: `switchView(current, next)` in `uibinder.js` jQuery-fades container IDs listed in the `VIEWS` map. `uicore.js` loads first and must not depend on internal modules (it is the crash-safe layer: frame buttons, auto-update listeners, `window.eval` disabling). `uibinder.js` loads second and owns startup sequencing — distribution load, account validation, deciding whether to show welcome/loginOptions/landing.

Gotchas when editing views:
- `require('./assets/js/...')` in renderer scripts resolves relative to `app/app.ejs` (the page URL), not the script's own file.
- `app.ejs` has a CSP `script-src` with a sha256 hash. Changing the inline script there requires updating the hash.
- **Cross-script globals.** Because all view scripts share one scope, a `const` in one file is a dependency of another with nothing to signal it. `landing.js` declares `md5Encode` (consumed by `lunarLogin.js`) and `exec` (consumed by `getJavaPaths` in `uibinder.js`). Removing a seemingly unused import in one file can break a different one — grep the whole `scripts/` directory, not just the file you are editing.

### Modules (`app/assets/js/`)

- **configmanager.js** — the single source of persisted state, `~/.lunarlauncher/config.json` (note: fork-specific directory name). Holds accounts, per-server Java config, resolution, mod configurations. Load/save is explicit (`ConfigManager.save()`); `validateKeySet` merges new default keys into existing user configs on upgrade.
- **distromanager.js** — wraps `helios-core`'s `DistributionAPI` against `REMOTE_DISTRO_URL` (currently `https://hermes-mc.net/downloads/lunarpixel/distribution.json`). The distribution index defines servers, modules, and Java requirements; see `docs/distro.md` and `docs/sample_distribution.json`. **This host currently returns NXDOMAIN**, so a fresh install fails at startup with "Fatal Error: Unable to Load Distribution Index". To develop without it, drop a valid `distribution.json` into the launcher directory (`~/Library/Application Support/Lunar Launcher/` on macOS) — `DistributionAPI` falls back to that local copy.
- **preloader.js** — Electron preload. Runs before the window: loads config, force-injects `commonDir`/`instanceDir` into `DistroAPI`, fetches the distribution, picks a default server, cleans the temp natives dir, then fires `distributionIndexDone`.
- **authmanager.js** — three account types: `microsoft` (full MSA→Xbox→MC flow via `helios-core/microsoft`, with token refresh in `validateSelected`), `lunar` (fork-specific offline mode: UUID is just the MD5 of the entered username, no server validation), and `mojang` (Yggdrasil — the code path still exists but `authserver.mojang.com` is permanently shut down, so it is dead). `validateSelected` only branches on `microsoft` vs everything else.
- **processbuilder.js** — builds and spawns the JVM. Split by Minecraft version: `_constructJVMArguments112` vs `_constructJVMArguments113` (1.13+ uses the manifest's argument templates). Also handles classpath assembly, native extraction to a temp dir, LiteLoader, mod list JSON/arg generation, and the autoconnect flag.
- **langloader.js** — all user-facing strings come from TOML. `app/assets/lang/en_US.toml` is the base; `_custom.toml` is merged on top and is where fork branding/URLs are overridden. `[ejs.*]` keys are read from templates via `lang(...)`; `[js.*]` keys from code via `Lang.queryJS(...)`.
- **dropinmodutil.js**, **serverstatus.js**, **discordwrapper.js** (Discord RPC, marked WIP), **isdev.js**.

### Launch flow

`landing.js:dlAsync()` is the whole "play" path: resolve server from distro → `validateSelectedJvm` / download Java if missing (`asyncSystemScan`, `downloadJava`) → `FullRepair` module from `helios-core` validates and redownloads assets in a child process → `new ProcessBuilder(...).build()` spawns the game → the launcher watches stdout for the game window handshake before hiding progress UI.

## Conventions

`eslint.config.mjs` (flat config, ESLint 9) enforces via `@stylistic`: 4-space indent, single quotes, **no semicolons**, no `var`, and `linebreak-style: windows` (CRLF).

Note that `npm run lint` currently reports ~7200 errors, almost all `linebreak-style`. This is inherited from upstream — a pristine `upstream/master` checkout fails its own lint the same way (~6900 errors), because several tracked files are LF while the rule demands CRLF. **Do not mass-convert line endings**: it would produce a repo-wide diff and cause a conflict in every future upstream merge. To see only real problems, filter them out:

```console
npx eslint . 2>&1 | grep -v linebreak-style
```

Releases are cut by bumping `version` in `package.json` and committing with the version number as the message (see `git log`). `electron-builder.yml` publishes to the `FlukRocker/LunarLauncher` GitHub repo, but the in-app macOS update URL in `uicore.js` points at `FlukRocker/LunarLauncherPublic` — keep that in mind when changing release targets.

## Known issues

- **The Lunar login button is unreachable.** `loginOptions.ejs` has the `loginOptionLunar` button commented out, but `loginOptions.js:40` still binds `loginOptionLunar.onclick` unconditionally. That throws `TypeError: Cannot set properties of null`, which aborts the rest of the script — so `loginOptionsCancelButton.onclick` and everything below it never binds either, leaving the cancel button on the login-options screen dead. Fixing it means deciding whether the Lunar option should be visible (uncomment the button) or not (guard the binding).
