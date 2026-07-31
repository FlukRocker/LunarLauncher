import { useEffect, useState } from 'react'
import { listen } from '@tauri-apps/api/event'
import {
    api,
    isApiError,
    launchApi,
    type Account,
    type LaunchProgress,
    type Server
} from '../lib/api'

/**
 * Main screen. PLAY runs the full pipeline in Rust — validate, download,
 * locate a JVM, spawn the game — and streams progress back over the
 * `launch://progress` event.
 */
export function Landing({ account }: { account: Account | null }) {
    const [server, setServer] = useState<Server | null>(null)
    const [error, setError] = useState<string | null>(null)
    const [progress, setProgress] = useState<LaunchProgress | null>(null)
    const [launching, setLaunching] = useState(false)

    useEffect(() => {
        api.getSelectedServer()
            .then(setServer)
            .catch((err: unknown) => setError(isApiError(err) ? err.message : String(err)))
    }, [])

    useEffect(() => {
        const unlisten = listen<LaunchProgress>('launch://progress', (e) => {
            setProgress(e.payload)
        })
        return () => {
            void unlisten.then((f) => f())
        }
    }, [])

    const play = async () => {
        setLaunching(true)
        setError(null)
        setProgress(null)
        try {
            await launchApi.launchGame()
        } catch (err: unknown) {
            setError(isApiError(err) ? err.message : String(err))
        } finally {
            setLaunching(false)
        }
    }

    return (
        <div className="view view--landing">
            <header className="landing__header">
                <div className="landing__account">{account ? account.displayName : 'No account'}</div>
            </header>

            <footer className="landing__footer">
                <div className="landing__status">
                    {error && <span className="panel__error">{error}</span>}
                    {!error && progress && (
                        <>
                            <span className="landing__detail">{progress.detail}</span>
                            <div className="progress">
                                <div
                                    className="progress__bar"
                                    style={{ width: `${progress.percent}%` }}
                                />
                            </div>
                        </>
                    )}
                    {!error && !progress && (
                        <span className="landing__server">
                            {server ? server.name : 'No server selected'}
                            {server && (
                                <span className="landing__server-meta">
                                    {' '}
                                    &bull; {server.minecraftVersion} &bull; {server.address}
                                </span>
                            )}
                        </span>
                    )}
                </div>

                <button
                    className="button button--play"
                    onClick={() => void play()}
                    disabled={launching || !server || !account}
                >
                    {launching ? 'WORKING…' : 'PLAY'}
                </button>
            </footer>
        </div>
    )
}
