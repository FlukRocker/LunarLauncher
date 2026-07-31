import { useEffect, useState } from 'react'
import { api, isApiError, type Account, type Server } from '../lib/api'

/**
 * Main screen. The launch button is deliberately disabled: the download and
 * JVM-spawn pipeline (helios-core's FullRepair + processbuilder.js) has not
 * been ported to Rust yet. Everything shown here is real data from the Rust
 * side — config, distribution index and the selected server.
 */
export function Landing({ account }: { account: Account | null }) {
    const [server, setServer] = useState<Server | null>(null)
    const [error, setError] = useState<string | null>(null)

    useEffect(() => {
        api.getSelectedServer()
            .then(setServer)
            .catch((err: unknown) =>
                setError(isApiError(err) ? err.message : String(err))
            )
    }, [])

    return (
        <div className="view view--landing">
            <header className="landing__header">
                <div className="landing__account">
                    {account ? account.displayName : 'No account'}
                </div>
            </header>

            <footer className="landing__footer">
                <div className="landing__server">
                    {error && <span className="panel__error">{error}</span>}
                    {!error && (server ? server.name : 'No server selected')}
                    {server && (
                        <span className="landing__server-meta">
                            {' '}
                            &bull; {server.minecraftVersion} &bull; {server.address}
                        </span>
                    )}
                </div>
                <button className="button button--play" disabled title="Launch pipeline not yet ported">
                    PLAY
                </button>
            </footer>
        </div>
    )
}
