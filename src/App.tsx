import { useEffect, useState } from 'react'
import { api, authApi, discordApi, isApiError, type Account, type Bootstrap } from './lib/api'
import { Frame } from './components/Frame'
import { FatalError } from './views/FatalError'
import { Landing } from './views/Landing'
import { Loading } from './views/Loading'
import { Settings } from './views/Settings'
import { LoginOptions } from './views/LoginOptions'
import { Welcome } from './views/Welcome'

/**
 * The views the launcher can show.
 *
 * In the Electron app this was the `VIEWS` map in uibinder.js, with jQuery
 * fading container IDs in and out. Here it is ordinary React state — one view
 * is mounted at a time.
 */
export type View = 'loading' | 'welcome' | 'loginOptions' | 'landing' | 'settings' | 'fatal'

// The Electron main process picked a random background per launch and passed
// it to EJS as `bkid`. Same behaviour, resolved in the renderer now.
const BACKGROUND_COUNT = 8
const backgroundUrl = new URL(
    `./assets/images/backgrounds/${Math.floor(Math.random() * BACKGROUND_COUNT)}.jpg`,
    import.meta.url
).href

export function App() {
    const [view, setView] = useState<View>('loading')
    const [boot, setBoot] = useState<Bootstrap | null>(null)
    const [fatal, setFatal] = useState<string | null>(null)
    const [accounts, setAccounts] = useState<Account[]>([])
    const [selected, setSelected] = useState<Account | null>(null)
    const [serverId, setServerId] = useState<string | null>(null)

    const refreshAccounts = () => {
        void api.getAccounts().then((list) => {
            setAccounts(list)
            // Removing the last account leaves nothing to launch with, so send
            // the user back to sign in rather than stranding them on a landing
            // view whose PLAY button can never work.
            if (list.length === 0) {
                setSelected(null)
                setView('loginOptions')
            }
        })
        void api.getConfig().then((c) => {
            setServerId(c.selectedServer)
            const uuid = c.selectedAccount
            setSelected(uuid ? (c.authenticationDatabase[uuid] ?? null) : null)
        })
    }

    useEffect(() => {
        document.body.style.backgroundImage = `url('${backgroundUrl}')`
    }, [])

    // The title bar sits above every pane (z-index 100 in launcher.css), so it
    // cannot inherit their backdrop. Helios solved this by assigning
    // frameBar.style.backgroundColor from JS; here the current view is
    // reflected onto the root element and CSS keys off it, so the bar tracks
    // whatever surface is open instead of staying on the base scrim.
    useEffect(() => {
        document.documentElement.dataset.view = view
        return () => {
            delete document.documentElement.dataset.view
        }
    }, [view])

    useEffect(() => {
        let cancelled = false

        api.bootstrap()
            .then((result) => {
                if (cancelled) return
                setBoot(result)
                setAccounts(result.accounts)
                setSelected(result.selectedAccount)
                void api.getConfig().then((c) => setServerId(c.selectedServer))
                // Presence is decorative; never let it block startup.
                void discordApi.connect().catch(() => {})

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
                    // An expired Microsoft token means the account cannot
                    // launch; send the user back to sign in rather than
                    // failing later with a confusing error.
                    void authApi
                        .validateSelected()
                        .then((ok) => setView(ok ? 'landing' : 'loginOptions'))
                        .catch(() => setView('landing'))
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
                    <LoginOptions
                        onLoggedIn={() => {
                            refreshAccounts()
                            setView('landing')
                        }}
                    />
                )}
                {view === 'landing' && boot && selected === null && (
                    <LoginOptions
                        onLoggedIn={() => {
                            refreshAccounts()
                            setView('landing')
                        }}
                    />
                )}
                {view === 'landing' && boot && selected !== null && (
                    <Landing
                        account={selected}
                        onOpenSettings={() => setView('settings')}
                    />
                )}
                {view === 'settings' && (
                    <Settings
                        serverId={serverId}
                        accounts={accounts}
                        onClose={() => setView('landing')}
                        onAccountsChanged={refreshAccounts}
                    />
                )}
                {view === 'fatal' && <FatalError message={fatal} />}
            </main>
        </>
    )
}
