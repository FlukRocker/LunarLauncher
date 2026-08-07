import { useEffect, useState } from 'react'
import { open } from '@tauri-apps/plugin-dialog'
import { api, type Server, type Settings } from '../lib/api'
import logo from '../assets/images/CyberLogo.png'

/**
 * First-run setup, in the Cyber Network design.
 *
 * The design was drawn as a Windows installer. It is used here instead because
 * NSIS cannot render it — no gradients on controls, no glows, no web fonts,
 * and now no wizard pages at all, since the installer is one-click. The
 * launcher is a webview, so the design renders exactly as authored rather
 * than approximated in bitmaps.
 *
 * The steps are the design's, mapped onto what the launcher actually has to
 * settle before it can do anything: terms, which pack, where it goes, and a
 * confirmation of the three.
 */

const STEPS = [
    { key: 'licence', num: '01', name: 'Licence', eyebrow: '// 01_LICENCE_AGREEMENT' },
    { key: 'pack', num: '02', name: 'Pack', eyebrow: '// 02_SELECT_PACK' },
    { key: 'target', num: '03', name: 'Target', eyebrow: '// 03_SELECT_TARGET' },
    { key: 'review', num: '04', name: 'Review', eyebrow: '// 04_REVIEW_AND_WRITE' }
] as const

function Chevron() {
    return (
        <svg width="16" height="16" viewBox="0 0 24 24" fill="none" aria-hidden="true">
            <path d="M9 6l6 6-6 6" stroke="currentColor" strokeWidth="2" />
        </svg>
    )
}

function Check() {
    return (
        <svg width="12" height="12" viewBox="0 0 24 24" fill="none" aria-hidden="true">
            <path d="M4 12.5l5.5 5.5L20 6.5" stroke="currentColor" strokeWidth="3" />
        </svg>
    )
}

export function FirstRun({ onDone }: { onDone: () => void }) {
    const [step, setStep] = useState(0)
    const [accepted, setAccepted] = useState(false)
    const [servers, setServers] = useState<Server[]>([])
    const [packId, setPackId] = useState<string | null>(null)
    const [settings, setSettings] = useState<Settings | null>(null)
    const [busy, setBusy] = useState(false)
    const [error, setError] = useState<string | null>(null)

    useEffect(() => {
        void api
            .getDistribution()
            .then((d) => {
                setServers(d.servers)
                // Preselect the distribution's main server, the way the
                // launcher would have if the user never opened this flow.
                const main = d.servers.find((s) => s.mainServer) ?? d.servers[0]
                if (main) setPackId(main.id)
            })
            .catch((e: unknown) => setError(String(e)))
        // Catching matters: without settings the target step has no folder to
        // show, but that is a degraded step rather than a broken app, and an
        // unhandled rejection here would take the whole view down.
        void api
            .getConfig()
            .then((c) => setSettings(c.settings))
            .catch((e: unknown) => setError(String(e)))
    }, [])

    const current = STEPS[step]
    const pack = servers.find((s) => s.id === packId) ?? null
    const dataDir = settings?.launcher.dataDirectory ?? ''

    const canAdvance =
        (step === 0 && accepted) ||
        (step === 1 && pack !== null) ||
        step === 2 ||
        (step === 3 && !busy)

    const chooseFolder = async () => {
        const picked = await open({ directory: true, defaultPath: dataDir || undefined })
        if (typeof picked !== 'string' || !settings) return
        setSettings({ ...settings, launcher: { ...settings.launcher, dataDirectory: picked } })
    }

    const finish = async () => {
        if (!settings || !pack) return
        setBusy(true)
        setError(null)
        try {
            // Saved together so a user who quits here does not come back to a
            // half-applied setup — a pack selected but written somewhere else.
            await api.saveSettings(settings)
            await api.setSelectedServer(pack.id)
            onDone()
        } catch (e) {
            setError(String(e))
            setBusy(false)
        }
    }

    return (
        <div className="cyber">
            <div className="cyber__frame" role="dialog" aria-label="First-run setup">
                <header className="cyber__header">
                    <img className="cyber__mark" src={logo} alt="" aria-hidden="true" />
                    <div className="cyber__brand">
                        <span className="cyber__brandName">Cyber Network</span>
                        <span className="cyber__brandSub">
                            Modpack setup{pack ? ` ${pack.minecraftVersion}` : ''}
                        </span>
                    </div>
                    <div className="cyber__pill">
                        <span
                            className={servers.length ? 'cyber__dot' : 'cyber__dot cyber__dot--off'}
                        />
                        {servers.length ? `${servers.length} packs` : 'No distribution'}
                    </div>
                </header>

                <nav className="cyber__rail" aria-label="Setup steps">
                    {STEPS.map((s, i) => (
                        <div
                            key={s.key}
                            className={
                                'cyber__step' +
                                (i === step ? ' cyber__step--current' : '') +
                                (i < step ? ' cyber__step--done' : '')
                            }
                            aria-current={i === step ? 'step' : undefined}
                        >
                            {i > 0 && (
                                <span className="cyber__chev">
                                    <Chevron />
                                </span>
                            )}
                            <div className="cyber__num">{i < step ? <Check /> : s.num}</div>
                            <span className="cyber__stepName">{s.name}</span>
                        </div>
                    ))}
                    <span className="cyber__counter">
                        Step {step + 1} of {STEPS.length}
                    </span>
                </nav>

                <div className="cyber__body">
                    <span className="cyber__eyebrow">{current.eyebrow}</span>

                    {step === 0 && (
                        <>
                            <h2 className="cyber__title">Terms of service</h2>
                            <div className="cyber__inset">
                                <div className="cyber__monoLabel">Modpack EULA · v4</div>
                                <p className="cyber__p" style={{ marginTop: 12 }}>
                                    <strong>1. Grant.</strong> You get a personal, non-exclusive
                                    licence to install and run the modpack on machines you control.
                                    The pack is free — it is not sold, resold or sublicensed.
                                </p>
                                <p className="cyber__p">
                                    <strong>2. Mojang.</strong> Not affiliated with Mojang AB or
                                    Microsoft. A valid Minecraft: Java Edition account is required
                                    and the Minecraft EULA applies at all times.
                                </p>
                                <p className="cyber__p">
                                    <strong>3. Fair play.</strong> Nothing in this pack reads
                                    memory, automates input or modifies combat. Running it next to
                                    an unauthorised client is a permanent network ban.
                                </p>
                                <p className="cyber__p">
                                    <strong>4. Data.</strong> The launcher sends your username and
                                    pack version to the network for matchmaking. Your account
                                    tokens are encrypted on this machine and never leave it.
                                </p>
                                <p className="cyber__p">
                                    <strong>5. Third-party mods.</strong> Mods ship under their own
                                    licences, reproduced in the install folder.
                                </p>
                                <p className="cyber__p">
                                    <strong>6. No warranty.</strong> The pack is provided as is.
                                    Back up your worlds before installing.
                                </p>
                            </div>
                            <button
                                className="cyber__check"
                                role="checkbox"
                                aria-checked={accepted}
                                onClick={() => setAccepted((v) => !v)}
                            >
                                <span className="cyber__box">{accepted && <Check />}</span>
                                <span className="cyber__checkText">
                                    I accept the terms of the licence agreement
                                    {accepted ? '' : ' — required to continue'}
                                </span>
                            </button>
                        </>
                    )}

                    {step === 1 && (
                        <>
                            <h2 className="cyber__title">Choose what gets written</h2>
                            <div className="cyber__inset" style={{ padding: 0 }}>
                                {servers.length === 0 && (
                                    <p className="cyber__p" style={{ padding: '14px 16px' }}>
                                        No packs were loaded from the distribution.
                                    </p>
                                )}
                                {servers.map((s) => (
                                    <button
                                        key={s.id}
                                        className={
                                            'cyber__row' +
                                            (s.id === packId ? ' cyber__row--active' : '')
                                        }
                                        onClick={() => setPackId(s.id)}
                                    >
                                        <span
                                            className={
                                                'cyber__box' +
                                                (s.id === packId ? ' cyber__box--on' : '')
                                            }
                                        >
                                            {s.id === packId && <Check />}
                                        </span>
                                        <span className="cyber__rowName">{s.name}</span>
                                        <span className="cyber__rowMeta">
                                            {s.minecraftVersion} · {s.modules.length} modules
                                        </span>
                                    </button>
                                ))}
                            </div>
                        </>
                    )}

                    {step === 2 && (
                        <>
                            <h2 className="cyber__title">Where it gets written</h2>
                            <div className="cyber__inset">
                                <div className="cyber__monoLabel">Install target</div>
                                <p className="cyber__p" style={{ marginTop: 12 }}>
                                    Packs, mods, assets and your worlds are written here. Assets
                                    and libraries are shared between packs, so a second pack costs
                                    far less than the first.
                                </p>
                                <div className="cyber__kv" style={{ marginTop: 14 }}>
                                    <span className="cyber__k">Folder</span>
                                    <span className="cyber__v">{dataDir || '—'}</span>
                                </div>
                                <button
                                    className="cyber__btn"
                                    style={{ marginTop: 14 }}
                                    onClick={() => void chooseFolder()}
                                >
                                    Change folder
                                </button>
                            </div>
                        </>
                    )}

                    {step === 3 && (
                        <>
                            <h2 className="cyber__title">Ready to write</h2>
                            <div className="cyber__inset">
                                <div className="cyber__kv">
                                    <span className="cyber__k">Pack</span>
                                    <span className="cyber__v">{pack?.name ?? '—'}</span>
                                </div>
                                <div className="cyber__kv">
                                    <span className="cyber__k">Minecraft</span>
                                    <span className="cyber__v">{pack?.minecraftVersion ?? '—'}</span>
                                </div>
                                <div className="cyber__kv">
                                    <span className="cyber__k">Modules</span>
                                    <span className="cyber__v">{pack?.modules.length ?? 0}</span>
                                </div>
                                <div className="cyber__kv">
                                    <span className="cyber__k">Folder</span>
                                    <span className="cyber__v">{dataDir || '—'}</span>
                                </div>
                                <div className="cyber__kv" style={{ borderBottom: 0 }}>
                                    <span className="cyber__k">Licence</span>
                                    <span className="cyber__v">Accepted</span>
                                </div>
                                <p className="cyber__p" style={{ marginTop: 12 }}>
                                    Nothing is downloaded yet. Files are fetched the first time you
                                    press play, so this step is instant.
                                </p>
                            </div>
                        </>
                    )}

                    {error && (
                        <p className="cyber__p" style={{ color: 'var(--cnm-redstone)' }}>
                            {error}
                        </p>
                    )}
                </div>

                <footer className="cyber__footer">
                    {step > 0 ? (
                        <button
                            className="cyber__btn"
                            onClick={() => setStep((s) => s - 1)}
                            disabled={busy}
                        >
                            Back
                        </button>
                    ) : (
                        <span className="cyber__hint">Setup runs once</span>
                    )}
                    <button
                        className="cyber__btn cyber__btn--primary"
                        disabled={!canAdvance}
                        onClick={() => {
                            if (step === STEPS.length - 1) void finish()
                            else setStep((s) => s + 1)
                        }}
                    >
                        {step === STEPS.length - 1 ? (busy ? 'Saving…' : 'Finish') : 'Continue'}
                    </button>
                </footer>
            </div>
        </div>
    )
}
