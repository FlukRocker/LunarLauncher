import { useEffect, useState } from 'react'
import { api, isApiError, type Bootstrap } from './lib/api'
import { Frame } from './components/Frame'
import { FatalError } from './views/FatalError'
import { Landing } from './views/Landing'
import { Loading } from './views/Loading'
import { LoginOptions } from './views/LoginOptions'
import { Welcome } from './views/Welcome'

/**
 * The views the launcher can show.
 *
 * In the Electron app this was the `VIEWS` map in uibinder.js, with jQuery
 * fading container IDs in and out. Here it is ordinary React state — one view
 * is mounted at a time.
 */
export type View = 'loading' | 'welcome' | 'loginOptions' | 'landing' | 'fatal'

export function App() {
    const [view, setView] = useState<View>('loading')
    const [boot, setBoot] = useState<Bootstrap | null>(null)
    const [fatal, setFatal] = useState<string | null>(null)

    useEffect(() => {
        let cancelled = false

        api.bootstrap()
            .then((result) => {
                if (cancelled) return
                setBoot(result)

                if (!result.distributionLoaded) {
                    // Same terminal state as the Electron launcher: without a
                    // distribution index there is nothing the app can do.
                    setView('fatal')
                    return
                }
                if (result.firstLaunch) {
                    setView('welcome')
                } else if (result.selectedAccount == null) {
                    setView('loginOptions')
                } else {
                    setView('landing')
                }
            })
            .catch((err: unknown) => {
                if (cancelled) return
                setFatal(isApiError(err) ? err.message : String(err))
                setView('fatal')
            })

        return () => {
            cancelled = true
        }
    }, [])

    return (
        <>
            <Frame />
            <main id="main">
                {view === 'loading' && <Loading />}
                {view === 'welcome' && <Welcome onContinue={() => setView('loginOptions')} />}
                {view === 'loginOptions' && (
                    <LoginOptions onLoggedIn={() => setView('landing')} />
                )}
                {view === 'landing' && boot && <Landing account={boot.selectedAccount} />}
                {view === 'fatal' && <FatalError message={fatal} />}
            </main>
        </>
    )
}
