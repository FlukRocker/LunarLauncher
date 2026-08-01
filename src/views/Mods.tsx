import { useCallback, useEffect, useState } from 'react'
import { open } from '@tauri-apps/plugin-dialog'
import {
    isApiError,
    modsApi,
    type DropinMod,
    type OptionalMod,
    type ShaderState
} from '../lib/api'

/**
 * Mod manager, replacing the mods section of the Electron settings view.
 *
 * Two independent lists, as in the original:
 *
 *  - Distribution mods, declared by the server. Optional ones can be toggled
 *    and the choice is persisted to `modConfigurations`. Required ones are
 *    shown but locked, so you can see what a server forces on you.
 *  - Drop-in mods, jars the user put in the instance's mods folder. Toggling
 *    renames between `.jar` and `.jar.disabled` rather than deleting, which
 *    is the convention mod loaders themselves understand.
 */
export function Mods({ serverId }: { serverId: string | null }) {
    const [distro, setDistro] = useState<OptionalMod[]>([])
    const [dropins, setDropins] = useState<DropinMod[]>([])
    const [shaders, setShaders] = useState<ShaderState | null>(null)
    const [error, setError] = useState<string | null>(null)
    const [busy, setBusy] = useState(false)

    const fail = (err: unknown) => setError(isApiError(err) ? err.message : String(err))

    const refresh = useCallback(() => {
        if (!serverId) return
        setError(null)
        modsApi.getDistributionMods(serverId).then(setDistro).catch(fail)
        modsApi.getDropinMods(serverId).then(setDropins).catch(fail)
        modsApi.getShaderpacks(serverId).then(setShaders).catch(fail)
    }, [serverId])

    useEffect(refresh, [refresh])

    if (!serverId) {
        return <p className="panel__hint">Select a server to manage its mods.</p>
    }

    const toggleDistro = async (m: OptionalMod) => {
        try {
            await modsApi.setDistributionModEnabled(serverId, m.id, !m.enabled)
            setDistro((prev) =>
                prev.map((x) => (x.id === m.id ? { ...x, enabled: !x.enabled } : x))
            )
        } catch (err) {
            fail(err)
        }
    }

    const toggleDropin = async (m: DropinMod) => {
        try {
            const next = await modsApi.toggleDropinMod(serverId, m.fullName, m.disabled)
            setDropins((prev) =>
                prev.map((x) =>
                    x.fullName === m.fullName
                        ? { ...x, fullName: next, disabled: !x.disabled }
                        : x
                )
            )
        } catch (err) {
            fail(err)
        }
    }

    const removeDropin = async (m: DropinMod) => {
        try {
            await modsApi.deleteDropinMod(serverId, m.fullName)
            setDropins((prev) => prev.filter((x) => x.fullName !== m.fullName))
        } catch (err) {
            fail(err)
        }
    }

    const addMods = async () => {
        setBusy(true)
        setError(null)
        try {
            const picked = await open({
                multiple: true,
                filters: [{ name: 'Mods', extensions: ['jar', 'zip', 'litemod'] }]
            })
            if (picked) {
                const paths = Array.isArray(picked) ? picked : [picked]
                const n = await modsApi.addDropinMods(serverId, paths)
                if (n < paths.length) {
                    setError(`Added ${n} of ${paths.length}; the rest were not mod files.`)
                }
                refresh()
            }
        } catch (err) {
            fail(err)
        } finally {
            setBusy(false)
        }
    }

    return (
        <>
            {error && <p className="panel__error">{error}</p>}

            <h3 className="mods__heading">Server Mods</h3>
            {distro.length === 0 ? (
                <p className="panel__hint">This server declares no mods.</p>
            ) : (
                <ul className="mod-list">
                    {distro.map((m) => (
                        <li key={m.id} className="mod-list__item">
                            <label className="mod-list__label">
                                <input
                                    type="checkbox"
                                    checked={m.enabled}
                                    disabled={m.required}
                                    onChange={() => void toggleDistro(m)}
                                />
                                <span>{m.name}</span>
                                {m.required && <span className="mod-list__tag">required</span>}
                            </label>
                        </li>
                    ))}
                </ul>
            )}

            <h3 className="mods__heading">
                Drop-in Mods
                <span className="mods__actions">
                    <button className="button" disabled={busy} onClick={() => void addMods()}>
                        Add
                    </button>
                    <button
                        className="button"
                        onClick={() => void modsApi.openModsFolder(serverId).catch(fail)}
                    >
                        Open Folder
                    </button>
                    <button className="button" onClick={refresh}>
                        Refresh
                    </button>
                </span>
            </h3>
            {dropins.length === 0 ? (
                <p className="panel__hint">
                    No mods found. Use Add, or drop jars into the instance&apos;s mods folder.
                </p>
            ) : (
                <ul className="mod-list">
                    {dropins.map((m) => (
                        <li key={m.fullName} className="mod-list__item">
                            <label className="mod-list__label">
                                <input
                                    type="checkbox"
                                    checked={!m.disabled}
                                    onChange={() => void toggleDropin(m)}
                                />
                                <span className={m.disabled ? 'mod-list__off' : undefined}>
                                    {m.name}
                                </span>
                            </label>
                            <button className="button" onClick={() => void removeDropin(m)}>
                                Delete
                            </button>
                        </li>
                    ))}
                </ul>
            )}

            {shaders && shaders.packs.length > 1 && (
                <>
                    <h3 className="mods__heading">Shaderpack</h3>
                    <select
                        className="input input--inline"
                        value={shaders.selected}
                        onChange={(e) => {
                            const pack = e.target.value
                            setShaders({ ...shaders, selected: pack })
                            void modsApi.setShaderpack(serverId, pack).catch(fail)
                        }}
                    >
                        {shaders.packs.map((p) => (
                            <option key={p.fullname} value={p.fullname}>
                                {p.name}
                            </option>
                        ))}
                    </select>
                </>
            )}
        </>
    )
}
