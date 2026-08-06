import { useEffect, useState } from 'react'
import { listen } from '@tauri-apps/api/event'
import { updateApi, type UpdateInfo, type UpdateProgress } from '../lib/api'

type Phase =
    | { kind: 'offer' }
    | { kind: 'installing'; progress: UpdateProgress | null }
    | { kind: 'failed'; message: string }

/**
 * The offer the startup check defers to.
 *
 * Reuses `errorModal`'s scrim and dialog styling rather than introducing a
 * second dialog system — the two differ in content, not in shape.
 */
export function UpdateModal({
    update,
    onDismiss,
    gameRunning
}: {
    update: UpdateInfo
    onDismiss: () => void
    gameRunning: boolean
}) {
    const [phase, setPhase] = useState<Phase>({ kind: 'offer' })
    const installing = phase.kind === 'installing'

    useEffect(() => {
        if (!installing) return
        const unlisten = listen<UpdateProgress>('update://progress', (e) => {
            setPhase({ kind: 'installing', progress: e.payload })
        })
        return () => {
            void unlisten.then((f) => f())
        }
    }, [installing])

    // Escape declines, but not mid-install: dismissing then would hide a
    // download that is still running and about to replace the binary.
    useEffect(() => {
        if (installing) return
        const onKey = (e: KeyboardEvent) => {
            if (e.key === 'Escape') void decline()
        }
        window.addEventListener('keydown', onKey)
        return () => window.removeEventListener('keydown', onKey)
    })

    const decline = async () => {
        await updateApi.dismiss()
        onDismiss()
    }

    const install = async () => {
        setPhase({ kind: 'installing', progress: null })
        try {
            await updateApi.install()
            // Reached only where the installer does not replace this process.
            // On Windows the app is gone before this resolves.
        } catch (e) {
            setPhase({ kind: 'failed', message: String(e) })
        }
    }

    const progress = installing ? phase.progress : null
    const pct = progress?.percent
    const mb = (n: number) => `${(n / 1_048_576).toFixed(1)} MB`

    return (
        <div
            className="errorModal__scrim"
            role="presentation"
            // Only dismissible by click while there is a decision to make.
            onClick={installing ? undefined : () => void decline()}
        >
            <div
                className="errorModal"
                role="dialog"
                aria-modal="true"
                aria-labelledby="updateModalTitle"
                tabIndex={-1}
                onClick={(e) => e.stopPropagation()}
            >
                <h2 className="errorModal__title" id="updateModalTitle">
                    Update available
                </h2>
                <p className="errorModal__message">
                    Version {update.version} is ready to install. This launcher is{' '}
                    {update.currentVersion}.
                </p>

                {update.notes && <pre className="errorModal__detail">{update.notes}</pre>}

                {phase.kind === 'failed' && (
                    <p className="errorModal__status">{phase.message}</p>
                )}

                {gameRunning && !installing && (
                    <p className="errorModal__status">
                        Minecraft is running. Close the game first — installing now would replace
                        the launcher underneath the running session.
                    </p>
                )}

                {installing && (
                    <div className="updateProgress">
                        <div className="updateProgress__track">
                            <div
                                className={
                                    pct === undefined
                                        ? 'updateProgress__fill updateProgress__fill--indeterminate'
                                        : 'updateProgress__fill'
                                }
                                style={pct === undefined ? undefined : { width: `${pct}%` }}
                            />
                        </div>
                        <span className="errorModal__status">
                            {progress
                                ? progress.total
                                    ? `${mb(progress.downloaded)} of ${mb(progress.total)}`
                                    : // No content-length: showing a percentage
                                      // here would be inventing one.
                                      `${mb(progress.downloaded)} downloaded`
                                : 'Starting…'}
                        </span>
                    </div>
                )}

                <div className="errorModal__actions">
                    {installing ? (
                        <span className="errorModal__status">
                            Installing. The launcher will restart itself.
                        </span>
                    ) : (
                        <>
                            <button className="button" onClick={() => void decline()}>
                                Not now
                            </button>
                            <button
                                className="button button--primary"
                                onClick={() => void install()}
                                disabled={gameRunning}
                            >
                                {phase.kind === 'failed' ? 'Try again' : 'Install and restart'}
                            </button>
                        </>
                    )}
                </div>

                <p className="errorModal__note">
                    Updates are verified against a signing key built into this launcher. One that
                    fails verification is refused, not installed.
                </p>
            </div>
        </div>
    )
}
