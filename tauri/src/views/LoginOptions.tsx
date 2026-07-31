import { useState } from 'react'
import { api, authApi, isApiError } from '../lib/api'

/**
 * Account selection.
 *
 * Note this is where the Electron app had a latent bug: the Lunar button was
 * commented out of loginOptions.ejs while loginOptions.js still bound its
 * onclick, throwing and killing the rest of the script. Here the button either
 * exists or it doesn't, and there is no shared global scope to corrupt.
 */
export function LoginOptions({ onLoggedIn }: { onLoggedIn: () => void }) {
    const [mode, setMode] = useState<'choose' | 'lunar'>('choose')
    const [msftBusy, setMsftBusy] = useState(false)
    const [username, setUsername] = useState('')
    const [busy, setBusy] = useState(false)
    const [error, setError] = useState<string | null>(null)

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
            setError(isApiError(err) ? err.message : String(err))
        } finally {
            setBusy(false)
        }
    }

    const loginMicrosoft = async () => {
        setMsftBusy(true)
        setError(null)
        try {
            await authApi.microsoftLogin()
            onLoggedIn()
        } catch (err: unknown) {
            setError(isApiError(err) ? err.message : String(err))
        } finally {
            setMsftBusy(false)
        }
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
                    disabled={msftBusy}
                    onClick={() => void loginMicrosoft()}
                >
                    {msftBusy ? 'Waiting for Microsoft…' : 'Log in with Microsoft'}
                </button>
                {error && <p className="panel__error">{error}</p>}
                <button className="button" onClick={() => setMode('lunar')}>
                    Offline account
                </button>
            </div>
        </div>
    )
}
