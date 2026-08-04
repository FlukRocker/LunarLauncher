import { useEffect, useState } from 'react'
import { save } from '@tauri-apps/plugin-dialog'
import { writeTextFile } from '@tauri-apps/plugin-fs'
import { diagnosticsApi } from '../lib/api'

export interface LauncherError {
    /** Short, human summary. Shown large. */
    title: string
    /** What the user can do about it, when there is something. */
    message: string
    /** Raw error text. Collapsed by default — most users should not need it. */
    detail?: string
}

/**
 * Failure dialog.
 *
 * Errors were previously a line of small red text, which is easy to miss and
 * impossible to hand to anyone. This makes a failure unmissable and, more
 * usefully, exportable: the report is assembled in Rust because the context
 * that matters — resolved paths, the JVM actually chosen, recent game output —
 * only exists on that side.
 */
export function ErrorModal({
    error,
    onClose
}: {
    error: LauncherError
    onClose: () => void
}) {
    const [showDetail, setShowDetail] = useState(false)
    const [status, setStatus] = useState<string | null>(null)

    // Escape closes, and focus moves to the dialog so a keyboard user is not
    // stranded behind it.
    useEffect(() => {
        const onKey = (e: KeyboardEvent) => {
            if (e.key === 'Escape') onClose()
        }
        window.addEventListener('keydown', onKey)
        return () => window.removeEventListener('keydown', onKey)
    }, [onClose])

    const context = `${error.title}\n${error.message}${
        error.detail ? `\n\n${error.detail}` : ''
    }`

    const copyReport = async () => {
        setStatus('Building report…')
        try {
            const report = await diagnosticsApi.export(context)
            await navigator.clipboard.writeText(report)
            setStatus('Report copied to clipboard')
        } catch (e) {
            setStatus(`Could not build the report: ${String(e)}`)
        }
    }

    const saveReport = async () => {
        setStatus('Building report…')
        try {
            const report = await diagnosticsApi.export(context)
            const path = await save({
                defaultPath: `lunar-launcher-diagnostics-${Date.now()}.txt`,
                filters: [{ name: 'Text', extensions: ['txt'] }]
            })
            if (!path) {
                setStatus(null)
                return
            }
            await writeTextFile(path, report)
            setStatus(`Saved to ${path}`)
        } catch (e) {
            setStatus(`Could not save the report: ${String(e)}`)
        }
    }

    return (
        <div className="errorModal__scrim" role="presentation" onClick={onClose}>
            <div
                className="errorModal"
                role="alertdialog"
                aria-modal="true"
                aria-labelledby="errorModalTitle"
                tabIndex={-1}
                // Clicking the dialog itself must not dismiss it.
                onClick={(e) => e.stopPropagation()}
            >
                <h2 className="errorModal__title" id="errorModalTitle">
                    {error.title}
                </h2>
                <p className="errorModal__message">{error.message}</p>

                {error.detail && (
                    <>
                        <button
                            className="errorModal__disclose"
                            aria-expanded={showDetail}
                            onClick={() => setShowDetail((v) => !v)}
                        >
                            {showDetail ? 'Hide technical detail' : 'Show technical detail'}
                        </button>
                        {showDetail && <pre className="errorModal__detail">{error.detail}</pre>}
                    </>
                )}

                {status && <p className="errorModal__status">{status}</p>}

                <div className="errorModal__actions">
                    <button className="button" onClick={() => void copyReport()}>
                        Copy report
                    </button>
                    <button className="button" onClick={() => void saveReport()}>
                        Save report…
                    </button>
                    <button className="button button--primary" onClick={onClose}>
                        Close
                    </button>
                </div>

                <p className="errorModal__note">
                    The report includes your launcher version, settings and recent game output.
                    It does not include passwords or account tokens.
                </p>
            </div>
        </div>
    )
}
