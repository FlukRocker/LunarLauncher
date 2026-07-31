import { getCurrentWindow } from '@tauri-apps/api/window'

/**
 * Terminal startup failure, matching the Electron launcher's overlay when the
 * distribution index cannot be loaded from the remote or from disk.
 */
export function FatalError({ message }: { message: string | null }) {
    return (
        <div className="view view--centered">
            <div className="overlay">
                <h2 className="overlay__title">Fatal Error: Unable to Load Distribution Index</h2>
                <p className="overlay__desc">
                    A connection could not be established to our servers to download the
                    distribution index. No local copies were available to load.
                </p>
                <p className="overlay__desc">
                    The distribution index is an essential file which provides the latest server
                    information. The launcher is unable to start without it. Ensure you are
                    connected to the internet and relaunch the application.
                </p>
                {message && <pre className="overlay__detail">{message}</pre>}
                <button
                    className="button"
                    onClick={() => void getCurrentWindow().close()}
                >
                    Close
                </button>
            </div>
        </div>
    )
}
