import { useEffect, useState } from 'react'
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

type Tab = 'game' | 'java' | 'mods' | 'accounts'

/**
 * Settings, replacing the Electron settings.ejs/settings.js pair.
 *
 * Edits are held locally and committed on Save, matching how the Electron
 * view batched changes before calling ConfigManager.save().
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
    const [tab, setTab] = useState<Tab>('game')
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

    const scan = async () => {
        if (!serverId) return
        setError(null)
        try {
            setJvms(await launchApi.scanJava(serverId))
        } catch (err) {
            fail(err)
        }
    }

    return (
        <div className="view settings">
            <nav className="settings__tabs">
                {(['game', 'java', 'mods', 'accounts'] as Tab[]).map((t) => (
                    <button
                        key={t}
                        className={`settings__tab${tab === t ? ' settings__tab--active' : ''}`}
                        onClick={() => setTab(t)}
                    >
                        {t === 'game'
                            ? 'Game'
                            : t === 'java'
                              ? 'Java'
                              : t === 'mods'
                                ? 'Mods'
                                : 'Accounts'}
                    </button>
                ))}
                <div className="settings__spacer" />
                <button className="button" onClick={onClose}>
                    Done
                </button>
            </nav>

            <div className="settings__body">
                {error && <p className="panel__error">{error}</p>}

                {tab === 'game' && settings && (
                    <>
                        <label className="field">
                            <span>Width</span>
                            <input
                                className="input input--inline"
                                type="number"
                                value={settings.game.resWidth}
                                onChange={(e) =>
                                    setSettings({
                                        ...settings,
                                        game: { ...settings.game, resWidth: Number(e.target.value) }
                                    })
                                }
                            />
                        </label>
                        <label className="field">
                            <span>Height</span>
                            <input
                                className="input input--inline"
                                type="number"
                                value={settings.game.resHeight}
                                onChange={(e) =>
                                    setSettings({
                                        ...settings,
                                        game: { ...settings.game, resHeight: Number(e.target.value) }
                                    })
                                }
                            />
                        </label>
                        <label className="field field--check">
                            <input
                                type="checkbox"
                                checked={settings.game.fullscreen}
                                onChange={(e) =>
                                    setSettings({
                                        ...settings,
                                        game: { ...settings.game, fullscreen: e.target.checked }
                                    })
                                }
                            />
                            <span>Fullscreen</span>
                        </label>
                        <label className="field field--check">
                            <input
                                type="checkbox"
                                checked={settings.game.autoConnect}
                                onChange={(e) =>
                                    setSettings({
                                        ...settings,
                                        game: { ...settings.game, autoConnect: e.target.checked }
                                    })
                                }
                            />
                            <span>Auto-connect to the server on launch</span>
                        </label>
                        <p className="panel__hint">
                            Data directory: {settings.launcher.dataDirectory}
                        </p>
                    </>
                )}

                {tab === 'java' && (
                    java ? (
                        <>
                            <label className="field">
                                <span>Minimum RAM</span>
                                <input
                                    className="input input--inline"
                                    value={java.minRam}
                                    onChange={(e) => setJava({ ...java, minRam: e.target.value })}
                                />
                            </label>
                            <label className="field">
                                <span>Maximum RAM</span>
                                <input
                                    className="input input--inline"
                                    value={java.maxRam}
                                    onChange={(e) => setJava({ ...java, maxRam: e.target.value })}
                                />
                            </label>
                            {memory && (
                                <p className="panel__hint">
                                    This machine supports {memory.absoluteMin.toFixed(1)}G –{' '}
                                    {memory.absoluteMax.toFixed(0)}G. Use a suffix, e.g. 4G or 4096M.
                                </p>
                            )}
                            <label className="field">
                                <span>JVM options</span>
                                <input
                                    className="input input--inline"
                                    value={java.jvmOptions.join(' ')}
                                    onChange={(e) =>
                                        setJava({
                                            ...java,
                                            jvmOptions: e.target.value.split(/\s+/).filter(Boolean)
                                        })
                                    }
                                />
                            </label>

                            <button className="button" onClick={() => void scan()}>
                                Scan for Java installations
                            </button>
                            {jvms && (
                                <ul className="jvm-list">
                                    {jvms.length === 0 && (
                                        <li className="panel__hint">
                                            No compatible Java runtime found. Install one matching this
                                            server&apos;s requirement.
                                        </li>
                                    )}
                                    {jvms.map((j) => (
                                        <li key={j.path}>
                                            <button
                                                className="jvm-list__item"
                                                onClick={() => setJava({ ...java, executable: j.path })}
                                            >
                                                <strong>{j.versionStr}</strong> {j.vendor} ({j.arch})
                                                <br />
                                                <span className="panel__hint">{j.path}</span>
                                            </button>
                                        </li>
                                    ))}
                                </ul>
                            )}
                            <p className="panel__hint">
                                Selected: {java.executable ?? 'automatic (best match)'}
                            </p>
                        </>
                    ) : (
                        <p className="panel__hint">Select a server to configure Java.</p>
                    )
                )}

                {tab === 'mods' && <Mods serverId={serverId} />}

                {tab === 'accounts' && (
                    <ul className="account-list">
                        {accounts.length === 0 && <li className="panel__hint">No accounts.</li>}
                        {accounts.map((a) => (
                            <li key={a.uuid} className="account-list__item">
                                <span>
                                    {a.displayName}{' '}
                                    <span className="panel__hint">
                                        ({a.type === 'lunar' ? 'offline' : a.type})
                                    </span>
                                </span>
                                <span className="account-list__actions">
                                    <button
                                        className="button"
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
                                        className="button"
                                        onClick={() =>
                                            void api
                                                .removeAccount(a.uuid)
                                                .then(onAccountsChanged)
                                                .catch(fail)
                                        }
                                    >
                                        Remove
                                    </button>
                                </span>
                            </li>
                        ))}
                    </ul>
                )}
            </div>

            <footer className="settings__footer">
                {saved && <span className="panel__hint">Saved.</span>}
                <button className="button button--primary" onClick={() => void save()}>
                    Save
                </button>
            </footer>
        </div>
    )
}
