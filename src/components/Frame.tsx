import { getCurrentWindow } from '@tauri-apps/api/window'

/**
 * Custom titlebar, ported from the Electron `frame.ejs` so `launcher.css`
 * styles it unchanged.
 *
 * Two differences from the original:
 *
 *  - Dragging. The Electron markup relied on `-webkit-app-region: drag`,
 *    which is Chromium-only and does nothing in the system webviews Tauri
 *    renders in. Those declarations were stripped from the stylesheet; the
 *    `data-tauri-drag-region` attribute below replaces them and is handled
 *    natively by the window manager.
 *  - Platform branch. EJS picked between the macOS and Windows titlebars at
 *    render time in the main process. Here it is a runtime check, since one
 *    bundle ships to every platform.
 */
const isMac = navigator.userAgent.includes('Mac')

// Substituted at build time from LUNAR_BRAND_NAME; see vite.config.ts. The
// Windows titlebar draws its own title, so a branded build would otherwise
// still say the wrong name in the one place the user always sees.
declare const __BRAND_NAME__: string

export function Frame() {
    const win = getCurrentWindow()

    const minimize = () => void win.minimize()
    const restoreDown = () => void win.toggleMaximize()
    const close = () => void win.close()

    return (
        <div id="frameBar" data-tauri-drag-region>
            <div id="frameResizableTop" className="frameDragPadder" />
            <div id="frameMain" data-tauri-drag-region>
                <div className="frameResizableVert frameDragPadder" />
                {isMac ? (
                    <div id="frameContentDarwin" data-tauri-drag-region>
                        <div id="frameButtonDockDarwin">
                            <button
                                className="frameButtonDarwin fCb"
                                id="frameButtonDarwin_close"
                                tabIndex={-1}
                                aria-label="Close"
                                onClick={close}
                            />
                            <button
                                className="frameButtonDarwin fMb"
                                id="frameButtonDarwin_minimize"
                                tabIndex={-1}
                                aria-label="Minimize"
                                onClick={minimize}
                            />
                            <button
                                className="frameButtonDarwin fRb"
                                id="frameButtonDarwin_restoredown"
                                tabIndex={-1}
                                aria-label="Maximize"
                                onClick={restoreDown}
                            />
                        </div>
                    </div>
                ) : (
                    <div id="frameContentWin" data-tauri-drag-region>
                        <div id="frameTitleDock" data-tauri-drag-region>
                            <span id="frameTitleText">{__BRAND_NAME__}</span>
                        </div>
                        <div id="frameButtonDockWin">
                            <button
                                className="frameButton fMb"
                                id="frameButton_minimize"
                                tabIndex={-1}
                                aria-label="Minimize"
                                onClick={minimize}
                            >
                                <svg width="10" height="10" viewBox="0 0 12 12">
                                    <rect stroke="#ffffff" fill="#ffffff" width="10" height="1" x="1" y="6" />
                                </svg>
                            </button>
                            <button
                                className="frameButton fRb"
                                id="frameButton_restoredown"
                                tabIndex={-1}
                                aria-label="Maximize"
                                onClick={restoreDown}
                            >
                                <svg width="10" height="10" viewBox="0 0 12 12">
                                    <rect width="9" height="9" x="1.5" y="1.5" fill="none" stroke="#ffffff" strokeWidth="1.4px" />
                                </svg>
                            </button>
                            <button
                                className="frameButton fCb"
                                id="frameButton_close"
                                tabIndex={-1}
                                aria-label="Close"
                                onClick={close}
                            >
                                <svg width="10" height="10" viewBox="0 0 12 12">
                                    <polygon
                                        stroke="#ffffff"
                                        fill="#ffffff"
                                        fillRule="evenodd"
                                        points="11 1.576 6.583 6 11 10.424 10.424 11 6 6.583 1.576 11 1 10.424 5.417 6 1 1.576 1.576 1 6 5.417 10.424 1"
                                    />
                                </svg>
                            </button>
                        </div>
                    </div>
                )}
                <div className="frameResizableVert frameDragPadder" />
            </div>
        </div>
    )
}
