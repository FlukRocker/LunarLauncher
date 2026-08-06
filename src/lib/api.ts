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

/**
 * Outbound links for the landing page's social row.
 *
 * A local extension to the Helios spec. Every field is optional and so is the
 * whole object — an index that omits it is the normal case, and an absent field
 * means "hide that icon", never "render a dead one".
 */
export interface SocialLinks {
    website?: string | null
    discord?: string | null
    x?: string | null
    instagram?: string | null
    youtube?: string | null
}

export interface Distribution {
    version: string
    rss?: string | null
    links?: SocialLinks | null
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
    /**
     * Effective state — this mod's own preference AND every ancestor being on.
     * A mod nested under a switched-off parent reports `false` here even when
     * its own stored preference is on.
     */
    enabled: boolean
    /**
     * Nearest toggleable ancestor's id, or absent at the top level. Present so
     * the list can be rendered nested; the backend has already applied the
     * parent gating to `enabled`.
     */
    parent?: string | null
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

// --- News -----------------------------------------------------------------

export interface Article {
    title: string
    link: string
    author: string
    date: string
    /** Feed HTML. Rendered as markup — see the note in News.tsx. */
    content: string
}

export const newsApi = {
    getNews: () => invoke<Article[]>('get_news')
}

// --- Game process ---------------------------------------------------------

export const gameApi = {
    isRunning: () => invoke<boolean>('is_game_running'),
    getLog: () => invoke<string[]>('get_game_log'),
    clearLog: () => invoke<void>('clear_game_log')
}

// --- Telemetry ------------------------------------------------------------

export interface TelemetryConfig {
    enabled: boolean
    /** OTLP/HTTP collector, e.g. http://localhost:4318. Empty means inactive. */
    endpoint: string
    /** Optional OpenTelemetry Java agent jar, to instrument the game too. */
    javaAgentPath: string | null
}

export const telemetryApi = {
    get: () => invoke<TelemetryConfig>('get_telemetry'),
    save: (telemetry: TelemetryConfig) => invoke<void>('save_telemetry', { telemetry })
}

// --- Diagnostics ----------------------------------------------------------

export const diagnosticsApi = {
    /**
     * Build a support report. Assembled in Rust, where the useful context
     * lives. Redacted: no tokens, no full account UUIDs.
     */
    export: (errorContext?: string) =>
        invoke<string>('export_diagnostics', { errorContext: errorContext ?? null })
}

// --- Launcher updates -----------------------------------------------------

export interface UpdateInfo {
    version: string
    currentVersion: string
    /** Absent, not null, when the release carries no notes. */
    notes?: string
    pubDate?: string
}

export interface UpdateProgress {
    downloaded: number
    /** Absent when the server sends no content-length. */
    total?: number
    percent?: number
}

export const updateApi = {
    /**
     * The update found by the startup check, if any.
     *
     * Polled once on mount as well as listened for: the check runs in the
     * background and may finish before this component exists, and an offer
     * that depends on catching an event would be missed entirely.
     */
    pending: () => invoke<UpdateInfo | null>('get_pending_update'),
    /**
     * Download and install. Refused while the game is running — replacing the
     * launcher under a live session is the one destructive thing here.
     *
     * On Windows this does not return: the installer terminates the process.
     */
    install: () => invoke<void>('install_update'),
    /** Decline for this session. The next start checks again. */
    dismiss: () => invoke<void>('dismiss_update')
}

// --- Server icons ---------------------------------------------------------

/**
 * A server's icon as a `data:` URI, or null when it has none.
 *
 * Fetched in Rust rather than with an `<img src>` pointing at the
 * distribution's URL: the content security policy allows `https:` images but
 * not plain `http:`, so a controller on a LAN address would fail silently.
 * Results are cached for the session on the Rust side.
 */
export const iconApi = {
    forServer: (serverId: string) => invoke<string | null>('get_server_icon', { serverId })
}
