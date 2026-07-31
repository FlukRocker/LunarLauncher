/**
 * Typed wrappers over the Rust command surface.
 *
 * These types mirror src-tauri/src/{config,distribution,commands}.rs. They are
 * hand-written rather than generated; if you change a Rust struct that crosses
 * the boundary, update it here too. Serde is configured to emit camelCase, so
 * field names match the old Electron config.json exactly.
 */
import { invoke } from '@tauri-apps/api/core'

// --- Errors ---------------------------------------------------------------

export type ErrorKind =
    | 'Io'
    | 'Json'
    | 'Network'
    | 'ConfigNotLoaded'
    | 'NoDistribution'
    | 'UnknownServer'
    | 'Other'

export interface ApiError {
    kind: ErrorKind
    message: string
}

export function isApiError(e: unknown): e is ApiError {
    return typeof e === 'object' && e !== null && 'kind' in e && 'message' in e
}

// --- Accounts -------------------------------------------------------------

export interface MicrosoftTokens {
    access_token: string
    refresh_token: string
    expires_at: string
}

export type Account =
    | {
          type: 'microsoft'
          accessToken: string
          username: string
          uuid: string
          displayName: string
          expiresAt: string
          microsoft: MicrosoftTokens
      }
    | {
          type: 'lunar'
          username: string
          displayName: string
          uuid: string
          expiresAt: string
      }
    | {
          type: 'mojang'
          accessToken: string
          username: string
          uuid: string
          displayName: string
      }

// --- Distribution ---------------------------------------------------------

export type ModuleType =
    | 'Library'
    | 'ForgeHosted'
    | 'Forge'
    | 'Fabric'
    | 'LiteLoader'
    | 'ForgeMod'
    | 'FabricMod'
    | 'LiteMod'
    | 'File'
    | 'VersionManifest'

export interface Artifact {
    size: number
    MD5?: string | null
    url: string
    path?: string | null
}

export interface DistroModule {
    id: string
    name: string
    type: ModuleType
    classpath?: boolean | null
    required?: { value?: boolean | null; def?: boolean | null } | null
    artifact: Artifact
    subModules: DistroModule[]
}

export interface Server {
    id: string
    name: string
    description: string
    icon: string
    version: string
    address: string
    minecraftVersion: string
    mainServer: boolean
    autoconnect: boolean
    modules: DistroModule[]
}

export interface Distribution {
    version: string
    rss?: string | null
    servers: Server[]
}

export interface EffectiveJavaOptions {
    supported: string
    distribution: 'CORRETTO' | 'TEMURIN'
    suggestedMajor: number
}

// --- Config ---------------------------------------------------------------

export interface GameSettings {
    resWidth: number
    resHeight: number
    fullscreen: boolean
    autoConnect: boolean
    launchDetached: boolean
}

export interface LauncherSettings {
    allowPrerelease: boolean
    dataDirectory: string
}

export interface Settings {
    game: GameSettings
    launcher: LauncherSettings
}

export interface Config {
    settings: Settings
    selectedServer: string | null
    selectedAccount: string | null
    authenticationDatabase: Record<string, Account>
}

export interface MemoryInfo {
    absoluteMin: number
    absoluteMax: number
}

export interface Bootstrap {
    firstLaunch: boolean
    selectedAccount: Account | null
    accounts: Account[]
    distributionLoaded: boolean
}

// --- Commands -------------------------------------------------------------

export const api = {
    /** Startup: load config, fetch distribution, resolve selected server. */
    bootstrap: () => invoke<Bootstrap>('bootstrap'),

    getDistribution: () => invoke<Distribution>('get_distribution'),
    getSelectedServer: () => invoke<Server | null>('get_selected_server'),
    setSelectedServer: (serverId: string) =>
        invoke<void>('set_selected_server', { serverId }),
    getEffectiveJavaOptions: (serverId: string) =>
        invoke<EffectiveJavaOptions>('get_effective_java_options', { serverId }),

    getAccounts: () => invoke<Account[]>('get_accounts'),
    addLunarAccount: (username: string) =>
        invoke<Account>('add_lunar_account', { username }),
    removeAccount: (uuid: string) => invoke<boolean>('remove_account', { uuid }),
    selectAccount: (uuid: string) => invoke<void>('select_account', { uuid }),

    getMemoryInfo: (serverId?: string) =>
        invoke<MemoryInfo>('get_memory_info', { serverId: serverId ?? null }),
    getConfig: () => invoke<Config>('get_config'),
    saveSettings: (settings: Settings) => invoke<void>('save_settings', { settings })
}
