#!/usr/bin/env node
// Generates a Tauri config patch from environment variables.
//
// Values that live in `tauri.conf.json` — the product name, the bundle
// identifier, the version, the updater endpoint — are not readable through
// `option_env!`, so `build.rs` cannot reach them. Tauri's CLI does accept a
// second config file that it deep-merges over the first, which is what this
// writes.
//
// The point is unattended builds: CyberLauncherController produces one
// installer per customer and sets these per invocation. So every problem here
// is a hard failure with a specific message — a build that silently falls
// back to "Lunar Launcher" would ship to the wrong customer looking correct.

import { readFileSync, writeFileSync, existsSync } from 'node:fs'
import { resolve, dirname } from 'node:path'
import { fileURLToPath } from 'node:url'

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..')
const OUT = resolve(root, 'src-tauri/tauri.brand.json')

// Same precedence as build.rs: real environment first, then .env.local, then
// .env. Kept deliberately identical so a variable does not mean one thing to
// the Rust side and another here.
function loadEnv() {
    const env = { ...process.env }
    for (const name of ['.env.local', '.env']) {
        const path = resolve(root, name)
        if (!existsSync(path)) continue
        for (const line of readFileSync(path, 'utf8').split('\n')) {
            const trimmed = line.trim().replace(/^export\s+/, '')
            if (!trimmed || trimmed.startsWith('#')) continue
            const eq = trimmed.indexOf('=')
            if (eq < 1) continue
            const key = trimmed.slice(0, eq).trim()
            if (!/^[A-Za-z0-9_]+$/.test(key)) continue
            if (key in env) continue
            env[key] = trimmed.slice(eq + 1).trim().replace(/^(["'])(.*)\1$/, '$2')
        }
    }
    return env
}

const env = loadEnv()
const errors = []
const get = (key) => {
    const v = env[key]
    return v && v.trim() ? v.trim() : undefined
}

const patch = {}
const set = (path, value) => {
    const keys = path.split('.')
    let node = patch
    for (const key of keys.slice(0, -1)) node = node[key] ??= {}
    node[keys.at(-1)] = value
}

const brand = get('LUNAR_BRAND_NAME')
if (brand) {
    // productName also names the installer and the install directory, so a
    // character Windows forbids in a path fails the bundle step with a message
    // pointing at NSIS rather than at this variable.
    if (/[<>:"/\\|?*]/.test(brand)) {
        errors.push(`LUNAR_BRAND_NAME contains a character illegal in a Windows path: ${brand}`)
    }
    set('productName', brand)
    set('app.windows', [{ title: brand }])
    // Otherwise every brand's shortcuts land in a folder named "Lunar Launcher".
    set('bundle.windows.nsis.startMenuFolder', brand)
}

const identifier = get('LUNAR_APP_IDENTIFIER')
if (identifier) {
    // Tauri rejects an identifier ending in `.app` because macOS bundles use
    // that suffix; catching it here names the variable.
    if (!/^[A-Za-z0-9-]+(\.[A-Za-z0-9-]+)+$/.test(identifier)) {
        errors.push(`LUNAR_APP_IDENTIFIER must be reverse-DNS, e.g. net.example.launcher: ${identifier}`)
    } else if (identifier.endsWith('.app')) {
        errors.push(`LUNAR_APP_IDENTIFIER must not end in ".app": ${identifier}`)
    }
    set('identifier', identifier)
}

const version = get('LUNAR_VERSION')
if (version) {
    // NSIS and MSI both require a numeric x.y.z; a tag like "v2.3.0" or
    // "2.3.0-beta" fails deep inside the bundler.
    if (!/^\d+\.\d+\.\d+$/.test(version)) {
        errors.push(`LUNAR_VERSION must be x.y.z with no prefix or suffix: ${version}`)
    }
    set('version', version)
}

// Production is the channel real users poll. Everything below treats it as
// the setting that must not be loosened by accident.
const channel = get('LUNAR_UPDATE_CHANNEL') ?? 'development'
if (!['production', 'staging', 'development'].includes(channel)) {
    errors.push(`LUNAR_UPDATE_CHANNEL must be production, staging or development: ${channel}`)
}

const endpoint = get('LUNAR_UPDATER_ENDPOINT')
const pubkey = get('LUNAR_UPDATER_PUBKEY')
if (endpoint) {
    if (!/\{\{target\}\}/.test(endpoint) || !/\{\{current_version\}\}/.test(endpoint)) {
        errors.push(
            'LUNAR_UPDATER_ENDPOINT must contain {{target}} and {{current_version}}; ' +
                'without them every platform is served the same file'
        )
    }
    // An updater with no key trusts any response. Refuse rather than ship a
    // launcher that will install whatever that endpoint returns.
    if (!pubkey) {
        errors.push('LUNAR_UPDATER_ENDPOINT is set but LUNAR_UPDATER_PUBKEY is not; an unsigned updater would install anything the endpoint returns')
    }
    set('plugins.updater.active', true)
    set('plugins.updater.endpoints', [endpoint])
    if (endpoint.startsWith('http://')) {
        // The updater channel delivers an executable. Over plain http anyone
        // on the path chooses which bytes arrive; the signature check is the
        // only thing left standing, and it should not be the only thing.
        if (channel === 'production') {
            errors.push(
                'LUNAR_UPDATE_CHANNEL=production requires an https updater endpoint. ' +
                    'Plain http would need dangerousInsecureTransportProtocol, which turns off ' +
                    'transport security on the channel that delivers an executable.'
            )
        } else {
            console.warn(
                `[brand] WARNING: ${channel} channel over plain http; updates are downloaded ` +
                    'over an unencrypted channel'
            )
            set('plugins.updater.dangerousInsecureTransportProtocol', true)
        }
    }
}
if (pubkey) set('plugins.updater.pubkey', pubkey)

const iconDir = get('LUNAR_ICON_DIR')
if (iconDir) {
    const names = ['32x32.png', '128x128.png', '128x128@2x.png', 'icon.icns', 'icon.ico']
    const missing = names.filter((n) => !existsSync(resolve(root, iconDir, n)))
    if (missing.length) {
        errors.push(`LUNAR_ICON_DIR ${iconDir} is missing: ${missing.join(', ')}`)
    }
    // Bundle icon paths resolve relative to src-tauri, not to the repo root.
    set('bundle.icon', names.map((n) => resolve(root, iconDir, n)))
}

const bg = get('LUNAR_WINDOW_BG')
if (bg) {
    if (!/^#[0-9a-fA-F]{6}$/.test(bg)) {
        errors.push(`LUNAR_WINDOW_BG must be #rrggbb: ${bg}`)
    }
    // Merged into the same single-window array the brand name writes to.
    patch.app ??= {}
    patch.app.windows = [{ ...(patch.app.windows?.[0] ?? {}), backgroundColor: bg }]
}

if (errors.length) {
    console.error('[brand] refusing to build:')
    for (const e of errors) console.error(`  - ${e}`)
    process.exit(1)
}

if (Object.keys(patch).length === 0) {
    // Tauri still needs a readable file when --config is passed
    // unconditionally by the npm script.
    writeFileSync(OUT, '{}\n')
    console.log('[brand] no branding variables set; building unbranded')
} else {
    writeFileSync(OUT, JSON.stringify(patch, null, 2) + '\n')
    console.log(`[brand] wrote ${OUT}`)
    console.log(JSON.stringify(patch, null, 2))
    if (identifier) {
        // Worth saying out loud: the identifier picks the config directory, so
        // a branded build does not see an existing install's accounts.
        console.log(`[brand] identifier ${identifier} — this build stores its config separately from other brands`)
    }
}
