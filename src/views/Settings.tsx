import { useEffect, useState } from 'react'
import { GameLog } from './GameLog'
import { Mods } from './Mods'
import {
    api,
    isApiError,
    launchApi,
    settingsApi,
    type Account,
    type JavaSettings,
    type JvmDetails,
    type MemoryInfo,
    type Settings as AppSettings
} from '../lib/api'

type Tab = 'account' | 'minecraft' | 'mods' | 'java' | 'launcher'

const NAV: { tab: Tab; label: string }[] = [
    { tab: 'account', label: 'Account' },
    { tab: 'minecraft', label: 'Minecraft' },
    { tab: 'mods', label: 'Mods' },
    { tab: 'java', label: 'Java' },
    { tab: 'launcher', label: 'Launcher' }
]

const HEADERS: Record<Tab, [string, string]> = {
    account: ['Account Settings', 'Add new accounts or manage existing ones.'],
    minecraft: ['Minecraft Settings', 'Options related to game launch, and the game log.'],
    mods: ['Mod Settings', 'Enable, disable, and manage mods.'],
    java: ['Java Settings', 'Manage the Java configuration (advanced).'],
    launcher: ['Launcher Settings', 'Options related to the launcher itself.']
}

/**
 * Settings, using the Electron `settings.ejs` markup — same ids and class
 * names — so the ported launcher.css styles it unchanged. That is also what
 * fixes legibility: launcher.css gives the panel its own dark backing, rather
 * than letting the game background show through at full strength.
 */
export function Settings({
    serverId,
    accounts,
    onClose,
    onAccountsChanged
}: {
    serverId: string | null
    accounts: Account[]
    onClose: () => void
    onAccountsChanged: () => void
}) {
    const [tab, setTab] = useState<Tab>('account')
    const [settings, setSettings] = useState<AppSettings | null>(null)
    const [java, setJava] = useState<JavaSettings | null>(null)
    const [memory, setMemory] = useState<MemoryInfo | null>(null)
    const [jvms, setJvms] = useState<JvmDetails[] | null>(null)
    const [error, setError] = useState<string | null>(null)
    const [saved, setSaved] = useState(false)

    const fail = (err: unknown) => setError(isApiError(err) ? err.message : String(err))

    useEffect(() => {
        api.getConfig().then((c) => setSettings(c.settings)).catch(fail)
        api.getMemoryInfo(serverId ?? undefined).then(setMemory).catch(fail)
        if (serverId) settingsApi.getJavaConfig(serverId).then(setJava).catch(fail)
    }, [serverId])

    const save = async () => {
        setError(null)
        try {
            if (settings) await api.saveSettings(settings)
            if (serverId && java) await settingsApi.saveJavaConfig(serverId, java)
            setSaved(true)
            setTimeout(() => setSaved(false), 1500)
        } catch (err) {
            fail(err)
        }
    }

    const done = async () => {
        await save()
        onClose()
    }

    const scan = async () => {
        if (!serverId) return
        setError(null)
        try {
            setJvms(await launchApi.scanJava(serverId))
        } catch (err) {
            fail(err)
        }
    }

    const [headerText, headerDesc] = HEADERS[tab]

    return (
        <div id="settingsContainer">
            <div id="settingsContainerLeft">
                <div id="settingsNavContainer">
                    <div id="settingsNavHeader">
                        <span id="settingsNavHeaderText">Settings</span>
                    </div>
                    <div id="settingsNavItemsContainer">
                        <div id="settingsNavItemsContent">
                            {NAV.map((n) => (
                                <button
                                    key={n.tab}
                                    className={
                                        'settingsNavItem' +
                                        (tab === n.tab ? ' settingsNavItem--active' : '')
                                    }
                                    // React strips `selected` from a <button> — it is only a
                                    // valid prop on <option> — so launcher.css's
                                    // [selected] rule can never match. A data attribute
                                    // reaches the DOM intact and keeps that rule working.
                                    data-selected={tab === n.tab ? 'true' : undefined}
                                    aria-current={tab === n.tab ? 'page' : undefined}
                                    onClick={() => setTab(n.tab)}
                                >
                                    {n.label}
                                </button>
                            ))}
                            <div className="settingsNavSpacer" />
                            <div id="settingsNavContentBottom">
                                <div className="settingsNavDivider" />
                                <button id="settingsNavDone" onClick={() => void done()}>
                                    Done
                                </button>
                            </div>
                        </div>
                    </div>
                </div>
            </div>

            <div id="settingsContainerRight">
                <div id="settingsContent">
                    <div className="settingsTab">
                        <div className="settingsTabHeader">
                            <span className="settingsTabHeaderText">{headerText}</span>
                            <span className="settingsTabHeaderDesc">{headerDesc}</span>
                        </div>

                        {error && <p className="panel__error">{error}</p>}

                        {tab === 'account' && (
                            <div className="settingsCurrentAccounts">
                                {accounts.length === 0 && (
                                    <span className="settingsFieldDesc">No accounts.</span>
                                )}
                                {accounts.map((a) => (
                                    <div className="settingsFieldContainer" key={a.uuid}>
                                        <div className="settingsFieldLeft">
                                            <span className="settingsFieldTitle">
                                                {a.displayName}
                                            </span>
                                            <span className="settingsFieldDesc">
                                                {a.type === 'lunar' ? 'offline' : a.type}
                                            </span>
                                        </div>
                                        <div className="settingsFieldRight">
                                            <button
                                                className="settingsFileSelButton"
                                                onClick={() =>
                                                    void api
                                                        .selectAccount(a.uuid)
                                                        .then(onAccountsChanged)
                                                        .catch(fail)
                                                }
                                            >
                                                Select
                                            </button>
                                            <button
                                                className="settingsFileSelButton"
                                                onClick={() =>
                                                    void api
                                                        .removeAccount(a.uuid)
                                                        .then(onAccountsChanged)
                                                        .catch(fail)
                                                }
                                            >
                                                Remove
                                            </button>
                                        </div>
                                    </div>
                                ))}
                            </div>
                        )}

                        {tab === 'minecraft' && settings && (
                            <>
                                <div className="settingsFieldContainer">
                                    <div className="settingsFieldLeft">
                                        <span className="settingsFieldTitle">Resolution</span>
                                        <span className="settingsFieldDesc">
                                            The game window size on launch.
                                        </span>
                                    </div>
                                    <div className="settingsFieldRight">
                                        <input
                                            className="settingsFieldValue"
                                            type="number"
                                            value={settings.game.resWidth}
                                            onChange={(e) =>
                                                setSettings({
                                                    ...settings,
                                                    game: {
                                                        ...settings.game,
                                                        resWidth: Number(e.target.value)
                                                    }
                                                })
                                            }
                                        />
                                        <input
                                            className="settingsFieldValue"
                                            type="number"
                                            value={settings.game.resHeight}
                                            onChange={(e) =>
                                                setSettings({
                                                    ...settings,
                                                    game: {
                                                        ...settings.game,
                                                        resHeight: Number(e.target.value)
                                                    }
                                                })
                                            }
                                        />
                                    </div>
                                </div>
                                <div className="settingsFieldContainer">
                                    <div className="settingsFieldLeft">
                                        <span className="settingsFieldTitle">Fullscreen</span>
                                        <span className="settingsFieldDesc">
                                            Launch the game in fullscreen.
                                        </span>
                                    </div>
                                    <div className="settingsFieldRight">
                                        <input
                                            type="checkbox"
                                            checked={settings.game.fullscreen}
                                            onChange={(e) =>
                                                setSettings({
                                                    ...settings,
                                                    game: {
                                                        ...settings.game,
                                                        fullscreen: e.target.checked
                                                    }
                                                })
                                            }
                                        />
                                    </div>
                                </div>
                                <div className="settingsFieldContainer">
                                    <div className="settingsFieldLeft">
                                        <span className="settingsFieldTitle">Auto-connect</span>
                                        <span className="settingsFieldDesc">
                                            Join the server automatically on launch.
                                        </span>
                                    </div>
                                    <div className="settingsFieldRight">
                                        <input
                                            type="checkbox"
                                            checked={settings.game.autoConnect}
                                            onChange={(e) =>
                                                setSettings({
                                                    ...settings,
                                                    game: {
                                                        ...settings.game,
                                                        autoConnect: e.target.checked
                                                    }
                                                })
                                            }
                                        />
                                    </div>
                                </div>

                                <div className="settingsTabHeader">
                                    <span className="settingsTabHeaderText">Game Log</span>
                                    <span className="settingsTabHeaderDesc">
                                        Output from the running game.
                                    </span>
                                </div>
                                <GameLog />
                            </>
                        )}

                        {tab === 'mods' && <Mods serverId={serverId} />}

                        {tab === 'java' &&
                            (java ? (
                                <>
                                    <div className="settingsFieldContainer">
                                        <div className="settingsFieldLeft">
                                            <span className="settingsFieldTitle">Maximum RAM</span>
                                            <span className="settingsFieldDesc">
                                                {memory
                                                    ? `This machine supports ${memory.absoluteMin.toFixed(
                                                          1
                                                      )}G – ${memory.absoluteMax.toFixed(0)}G.`
                                                    : 'Use a suffix, e.g. 4G or 4096M.'}
                                            </span>
                                        </div>
                                        <div className="settingsFieldRight">
                                            <input
                                                className="settingsFieldValue"
                                                value={java.maxRam}
                                                onChange={(e) =>
                                                    setJava({ ...java, maxRam: e.target.value })
                                                }
                                            />
                                        </div>
                                    </div>
                                    <div className="settingsFieldContainer">
                                        <div className="settingsFieldLeft">
                                            <span className="settingsFieldTitle">Minimum RAM</span>
                                            <span className="settingsFieldDesc">
                                                Setting minimum and maximum to the same value may
                                                reduce lag.
                                            </span>
                                        </div>
                                        <div className="settingsFieldRight">
                                            <input
                                                className="settingsFieldValue"
                                                value={java.minRam}
                                                onChange={(e) =>
                                                    setJava({ ...java, minRam: e.target.value })
                                                }
                                            />
                                        </div>
                                    </div>

                                    <div className="settingsFieldContainer">
                                        <div className="settingsFieldLeft">
                                            <span className="settingsFieldTitle">
                                                Java Executable
                                            </span>
                                            <span className="settingsFieldDesc">
                                                Selected: {java.executable ?? 'automatic (best match)'}
                                            </span>
                                        </div>
                                        <div className="settingsFieldRight">
                                            <button
                                                className="settingsFileSelButton"
                                                onClick={() => void scan()}
                                            >
                                                Scan
                                            </button>
                                        </div>
                                    </div>
                                    {jvms && (
                                        <div className="settingsCurrentAccounts">
                                            {jvms.length === 0 && (
                                                <span className="settingsFieldDesc">
                                                    No compatible Java runtime found.
                                                </span>
                                            )}
                                            {jvms.map((j) => (
                                                <div className="settingsFieldContainer" key={j.path}>
                                                    <div className="settingsFieldLeft">
                                                        <span className="settingsFieldTitle">
                                                            {j.versionStr} — {j.vendor}
                                                        </span>
                                                        <span className="settingsFieldDesc">
                                                            {j.path}
                                                        </span>
                                                    </div>
                                                    <div className="settingsFieldRight">
                                                        <button
                                                            className="settingsFileSelButton"
                                                            onClick={() =>
                                                                setJava({
                                                                    ...java,
                                                                    executable: j.path
                                                                })
                                                            }
                                                        >
                                                            Use
                                                        </button>
                                                    </div>
                                                </div>
                                            ))}
                                        </div>
                                    )}

                                    <div className="settingsFieldContainer">
                                        <div className="settingsFieldLeft">
                                            <span className="settingsFieldTitle">
                                                Additional JVM Options
                                            </span>
                                            <span className="settingsFieldDesc">
                                                Space-separated flags passed to the JVM.
                                            </span>
                                        </div>
                                        <div className="settingsFieldRight">
                                            <input
                                                className="settingsFieldValue"
                                                value={java.jvmOptions.join(' ')}
                                                onChange={(e) =>
                                                    setJava({
                                                        ...java,
                                                        jvmOptions: e.target.value
                                                            .split(/\s+/)
                                                            .filter(Boolean)
                                                    })
                                                }
                                            />
                                        </div>
                                    </div>
                                </>
                            ) : (
                                <span className="settingsFieldDesc">
                                    Select a server to configure Java.
                                </span>
                            ))}

                        {tab === 'launcher' && settings && (
                            <div className="settingsFieldContainer">
                                <div className="settingsFieldLeft">
                                    <span className="settingsFieldTitle">Data Directory</span>
                                    <span className="settingsFieldDesc">
                                        {settings.launcher.dataDirectory}
                                    </span>
                                </div>
                            </div>
                        )}

                        <div className="settingsFieldContainer">
                            <div className="settingsFieldLeft">
                                {saved && (
                                    <span className="settingsFieldDesc">Saved.</span>
                                )}
                            </div>
                            <div className="settingsFieldRight">
                                <button className="settingsFileSelButton" onClick={() => void save()}>
                                    Save
                                </button>
                            </div>
                        </div>
                    </div>
                </div>
            </div>
        </div>
    )
}
