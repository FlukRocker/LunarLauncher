import { useState } from 'react'
import { api, authApi, isApiError } from '../lib/api'

type Mode = 'choose' | 'lunar' | 'waitingBrowser'

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
                    className="button button--primary"
                    disabled={busy}
                    onClick={() => void loginBrowser()}
                >
                    Log in with Microsoft
                </button>
                <p className="panel__hint">Opens in your default browser.</p>
                {error && <p className="panel__error">{error}</p>}
                <button className="button" disabled={busy} onClick={() => void loginEmbedded()}>
                    Log in with Microsoft (in-app window)
                </button>
                <button className="button" onClick={() => setMode('lunar')}>
                    Offline account
                </button>
            </div>
        </div>
    )
}
