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

// --- Java & launch --------------------------------------------------------

export interface JvmDetails {
    path: string
    version: { major: number; minor: number; patch: number }
    versionStr: string
    vendor: string
    arch: string
}

export interface LaunchProgress {
    stage: 'resolving' | 'validating' | 'downloading' | 'java' | 'launching' | 'done'
    detail: string
    percent: number
}

export const launchApi = {
    scanJava: (serverId: string) => invoke<JvmDetails[]>('scan_java', { serverId }),
    /** Validates, downloads and launches. Resolves with the game's PID. */
    launchGame: () => invoke<number>('launch_game')
}

// --- Settings, accounts, Discord, updates ---------------------------------

export interface JavaSettings {
    minRam: string
    maxRam: string
    executable: string | null
    jvmOptions: string[]
}

export const settingsApi = {
    getJavaConfig: (serverId: string) =>
        invoke<JavaSettings>('get_java_config', { serverId }),
    saveJavaConfig: (serverId: string, settings: JavaSettings) =>
        invoke<void>('save_java_config', { serverId, settings })
}

export const authApi = {
    /**
     * Preferred: opens the consent page in the user's default browser and
     * catches the redirect on a loopback listener (RFC 8252).
     */
    microsoftLoginBrowser: () => invoke<Account>('microsoft_login_browser'),
    /** Fallback: consent inside an embedded Tauri window. */
    microsoftLogin: () => invoke<Account>('microsoft_login'),
    /** Aborts a pending browser sign-in. False if nothing was waiting. */
    cancelMicrosoftLogin: () => invoke<boolean>('cancel_microsoft_login'),
    /** Yggdrasil sign-in against LUNAR_AUTH_SERVER. */
    mojangLogin: (username: string, password: string) =>
        invoke<Account>('mojang_login', { username, password }),
    microsoftLogout: (uuid: string) => invoke<boolean>('microsoft_logout', { uuid }),
    /** Refreshes the selected account if expired. False means re-login needed. */
    validateSelected: () => invoke<boolean>('validate_selected_account')
}

export const discordApi = {
    connect: () => invoke<boolean>('discord_connect'),
    setDetails: (details: string, stateLine: string) =>
        invoke<void>('discord_set_details', { details, stateLine }),
    disconnect: () => invoke<void>('discord_disconnect')
}

// --- Mod manager ----------------------------------------------------------

export interface OptionalMod {
    id: string
    name: string
    /** Required mods are shown but cannot be turned off. */
    required: boolean
    enabled: boolean
}

export interface DropinMod {
    /** Handle for toggle/delete; includes any version folder and .disabled. */
    fullName: string
    name: string
    ext: string
    disabled: boolean
}

export interface Shaderpack {
    fullname: string
    name: string
}

export interface ShaderState {
    packs: Shaderpack[]
    selected: string
}

export const modsApi = {
    getDistributionMods: (serverId: string) =>
        invoke<OptionalMod[]>('get_distribution_mods', { serverId }),
    setDistributionModEnabled: (serverId: string, modId: string, enabled: boolean) =>
        invoke<void>('set_distribution_mod_enabled', { serverId, modId, enabled }),

    getDropinMods: (serverId: string) =>
        invoke<DropinMod[]>('get_dropin_mods', { serverId }),
    /** Returns the mod's new handle, since the file is renamed. */
    toggleDropinMod: (serverId: string, fullName: string, enable: boolean) =>
        invoke<string>('toggle_dropin_mod', { serverId, fullName, enable }),
    deleteDropinMod: (serverId: string, fullName: string) =>
        invoke<void>('delete_dropin_mod', { serverId, fullName }),
    addDropinMods: (serverId: string, paths: string[]) =>
        invoke<number>('add_dropin_mods', { serverId, paths }),
    openModsFolder: (serverId: string) => invoke<void>('open_mods_folder', { serverId }),

    getShaderpacks: (serverId: string) => invoke<ShaderState>('get_shaderpacks', { serverId }),
    setShaderpack: (serverId: string, pack: string) =>
        invoke<void>('set_shaderpack', { serverId, pack })
}

// --- Server status --------------------------------------------------------

export interface ServerStatus {
    online: boolean
    playersOnline: number | null
    playersMax: number | null
    version: string | null
}

/** Live player count. Resolves with online:false rather than throwing. */
export const statusApi = {
    getServerStatus: (serverId: string) =>
        invoke<ServerStatus>('get_server_status', { serverId })
}
