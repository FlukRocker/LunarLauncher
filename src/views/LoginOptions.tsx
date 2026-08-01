import { useState } from 'react'
import { api, authApi, isApiError } from '../lib/api'

/** The Microsoft logo, inlined exactly as loginOptions.ejs had it. */
function MicrosoftIcon() {
    return (
        <svg xmlns="http://www.w3.org/2000/svg" width="22" height="22" viewBox="0 0 23 23">
            <path fill="#f35325" d="M1 1h10v10H1z" />
            <path fill="#81bc06" d="M12 1h10v10H12z" />
            <path fill="#05a6f0" d="M1 12h10v10H1z" />
            <path fill="#ffba08" d="M12 12h10v10H12z" />
        </svg>
    )
}

/** The Mojang logo, likewise from the original markup. */
function MojangIcon() {
    return (
        <svg xmlns="http://www.w3.org/2000/svg" width="22" height="22" viewBox="0 0 9.677 9.667">
            <path d="M2.598.022h7.07L9.665 7c-.003 1.334-1.113 2.46-2.402 2.654H0V2.542C.134 1.2 1.3.195 2.598.022z" fill="#db2331" />
            <path d="M1.54 2.844c.314-.76 1.31-.46 1.954-.528.785-.083 1.503.272 2.1.758l.164-.9c.327.345.587.756.964 1.052.28.254.655-.342.86-.013.42.864.408 1.86.54 2.795l-.788-.373C6.9 4.17 5.126 3.052 3.656 3.685c-1.294.592-1.156 2.65.06 3.255 1.354.703 2.953.51 4.405.292-.07.42-.34.87-.834.816l-4.95.002c-.5.055-.886-.413-.838-.89l.04-4.315z" fill="#fff" />
        </svg>
    )
}

type Mode = 'choose' | 'lunar' | 'mojang' | 'waitingBrowser'

/**
 * Account selection.
 *
 * Microsoft sign-in defaults to the system browser (RFC 8252). Because that
 * hands control to another application, the launcher shows an explicit
 * waiting state rather than a spinner on a disabled button — the user needs
 * to know it is waiting on *them*, and needs a way out if the browser never
 * comes back or they change their mind.
 */
export function LoginOptions({ onLoggedIn }: { onLoggedIn: () => void }) {
    const [mode, setMode] = useState<Mode>('choose')
    const [username, setUsername] = useState('')
    const [password, setPassword] = useState('')
    const [busy, setBusy] = useState(false)
    const [error, setError] = useState<string | null>(null)

    const fail = (err: unknown) => setError(isApiError(err) ? err.message : String(err))

    const submitLunar = async (e: React.FormEvent) => {
        e.preventDefault()
        const name = username.trim()
        if (!name) {
            setError('Username is required.')
            return
        }
        setBusy(true)
        setError(null)
        try {
            await api.addLunarAccount(name)
            onLoggedIn()
        } catch (err: unknown) {
            fail(err)
        } finally {
            setBusy(false)
        }
    }

    const submitMojang = async (e: React.FormEvent) => {
        e.preventDefault()
        if (!username.trim() || !password) {
            setError('Username and password are required.')
            return
        }
        setBusy(true)
        setError(null)
        try {
            await authApi.mojangLogin(username.trim(), password)
            setPassword('')
            onLoggedIn()
        } catch (err: unknown) {
            fail(err)
        } finally {
            setBusy(false)
        }
    }

    const loginBrowser = async () => {
        setError(null)
        setMode('waitingBrowser')
        try {
            await authApi.microsoftLoginBrowser()
            onLoggedIn()
        } catch (err: unknown) {
            fail(err)
            setMode('choose')
        }
    }

    const loginEmbedded = async () => {
        // Release the loopback listener first; leaving it pending would keep
        // the command alive behind the embedded flow.
        await authApi.cancelMicrosoftLogin().catch(() => false)
        setError(null)
        setBusy(true)
        try {
            await authApi.microsoftLogin()
            onLoggedIn()
        } catch (err: unknown) {
            fail(err)
            setMode('choose')
        } finally {
            setBusy(false)
        }
    }

    const cancelBrowser = async () => {
        await authApi.cancelMicrosoftLogin().catch(() => false)
        setError(null)
        setMode('choose')
    }

    if (mode === 'waitingBrowser') {
        return (
            <div className="view view--centered">
                <div className="panel">
                    <div className="spinner" role="status" aria-label="Waiting" />
                    <h2 className="panel__title">Waiting for your browser</h2>
                    <p className="panel__desc">
                        A Microsoft sign-in page should have opened in your default browser.
                        Complete it there and this window will continue automatically.
                    </p>
                    <p className="panel__hint">
                        Nothing opened? Your Microsoft account may not permit browser sign-in
                        for this launcher — use the in-app window instead.
                    </p>
                    <div className="panel__actions">
                        <button
                            className="button"
                            disabled={busy}
                            onClick={() => void cancelBrowser()}
                        >
                            Cancel
                        </button>
                        <button
                            className="button button--primary"
                            disabled={busy}
                            onClick={() => void loginEmbedded()}
                        >
                            {busy ? 'Opening…' : 'Use in-app window'}
                        </button>
                    </div>
                </div>
            </div>
        )
    }

    if (mode === 'mojang') {
        return (
            <div className="view view--centered">
                <form className="panel" onSubmit={submitMojang}>
                    <h2 className="panel__title">Mojang Login</h2>
                    <input
                        className="input"
                        placeholder="Email or Username"
                        value={username}
                        autoFocus
                        disabled={busy}
                        onChange={(e) => setUsername(e.target.value)}
                    />
                    <input
                        className="input"
                        type="password"
                        placeholder="Password"
                        value={password}
                        disabled={busy}
                        onChange={(e) => setPassword(e.target.value)}
                    />
                    <p className="panel__hint">
                        Mojang&apos;s own auth server is shut down. This signs in against the
                        Yggdrasil endpoint in LUNAR_AUTH_SERVER, for servers running
                        authlib-injector or similar.
                    </p>
                    {error && <p className="panel__error">{error}</p>}
                    <div className="panel__actions">
                        <button
                            type="button"
                            className="button"
                            disabled={busy}
                            onClick={() => {
                                setPassword('')
                                setError(null)
                                setMode('choose')
                            }}
                        >
                            Cancel
                        </button>
                        <button
                            type="submit"
                            className="button button--primary"
                            disabled={busy || !username.trim() || !password}
                        >
                            {busy ? 'Logging In…' : 'Log In'}
                        </button>
                    </div>
                </form>
            </div>
        )
    }

    if (mode === 'lunar') {
        return (
            <div className="view view--centered">
                <form className="panel" onSubmit={submitLunar}>
                    <h2 className="panel__title">Offline Login</h2>
                    <input
                        className="input"
                        placeholder="Username"
                        value={username}
                        autoFocus
                        disabled={busy}
                        onChange={(e) => setUsername(e.target.value)}
                    />
                    {error && <p className="panel__error">{error}</p>}
                    <div className="panel__actions">
                        <button
                            type="button"
                            className="button"
                            disabled={busy}
                            onClick={() => setMode('choose')}
                        >
                            Cancel
                        </button>
                        <button
                            type="submit"
                            className="button button--primary"
                            disabled={busy || username.trim() === ''}
                        >
                            {busy ? 'Logging In…' : 'Log In'}
                        </button>
                    </div>
                </form>
            </div>
        )
    }

    return (
        <div className="view view--centered">
            <div className="panel">
                <h2 className="panel__title">Choose an account type</h2>
                <button
                    className="button button--primary loginOptionButton"
                    disabled={busy}
                    onClick={() => void loginBrowser()}
                >
                    <MicrosoftIcon />
                    <span>Log in with Microsoft</span>
                </button>
                {error && <p className="panel__error">{error}</p>}
                <button
                    className="button loginOptionButton"
                    disabled={busy}
                    onClick={() => setMode('mojang')}
                >
                    <MojangIcon />
                    <span>Log in with Mojang</span>
                </button>
                <button className="button loginOptionButton" onClick={() => setMode('lunar')}>
                    <span>Offline account</span>
                </button>
            </div>
        </div>
    )
}
