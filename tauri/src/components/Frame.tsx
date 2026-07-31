import { getCurrentWindow } from '@tauri-apps/api/window'

/**
 * Custom titlebar for the frameless window.
 *
 * The Electron version relied on `-webkit-app-region: drag`, which only works
 * in Chromium. Tauri's equivalent is the `data-tauri-drag-region` attribute,
 * handled natively by the window manager, so it works across WKWebView,
 * WebView2 and WebKitGTK alike.
 */
export function Frame() {
    const win = getCurrentWindow()

    return (
        <div className="frame" data-tauri-drag-region>
            <div className="frame__title" data-tauri-drag-region>
                Lunar Launcher
            </div>
            <div className="frame__buttons">
                <button
                    className="frame__button"
                    aria-label="Minimize"
                    onClick={() => void win.minimize()}
                >
                    &#8211;
                </button>
                <button
                    className="frame__button"
                    aria-label="Maximize"
                    onClick={() => void win.toggleMaximize()}
                >
                    &#9633;
                </button>
                <button
                    className="frame__button frame__button--close"
                    aria-label="Close"
                    onClick={() => void win.close()}
                >
                    &#10005;
                </button>
            </div>
        </div>
    )
}
