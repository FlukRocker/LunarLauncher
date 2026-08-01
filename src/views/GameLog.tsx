import { useEffect, useRef, useState } from 'react'
import { listen } from '@tauri-apps/api/event'
import { gameApi } from '../lib/api'

/**
 * The game's stdout/stderr, replacing the console the Electron build exposed
 * through devtools.
 *
 * Rust pumps the child's pipes into a ring buffer and emits each line, so this
 * shows history on open rather than only what arrives afterwards — which
 * matters because a crash usually happens before anyone thinks to look.
 */
export function GameLog() {
    const [lines, setLines] = useState<string[]>([])
    const [running, setRunning] = useState(false)
    const [follow, setFollow] = useState(true)
    const boxRef = useRef<HTMLDivElement>(null)

    useEffect(() => {
        void gameApi.getLog().then(setLines).catch(() => {})
        void gameApi.isRunning().then(setRunning).catch(() => {})

        const onLog = listen<string>('game://log', (e) =>
            setLines((prev) => [...prev.slice(-1999), e.payload])
        )
        const onStart = listen('game://started', () => {
            setRunning(true)
            setLines([])
        })
        const onExit = listen<number>('game://exited', (e) => {
            setRunning(false)
            setLines((prev) => [...prev, `— process exited with code ${e.payload} —`])
        })
        return () => {
            void onLog.then((f) => f())
            void onStart.then((f) => f())
            void onExit.then((f) => f())
        }
    }, [])

    // Only auto-scroll while following, so reading back through the log is not
    // yanked to the bottom by every new line.
    useEffect(() => {
        if (follow && boxRef.current) {
            boxRef.current.scrollTop = boxRef.current.scrollHeight
        }
    }, [lines, follow])

    const copy = () => void navigator.clipboard.writeText(lines.join('\n'))

    return (
        <div className="gameLog">
            <div className="gameLog__actions">
                <span
                    className={`gameLog__state${running ? ' gameLog__state--live' : ''}`}
                >
                    {running
                        ? `Running · ${lines.length} lines`
                        : lines.length > 0
                          ? `Not running · ${lines.length} lines`
                          : 'No output yet. Launch the game to see its log here.'}
                </span>
                <label className="gameLog__follow">
                    <input
                        type="checkbox"
                        checked={follow}
                        onChange={(e) => setFollow(e.target.checked)}
                    />
                    <span>Follow</span>
                </label>
                <button
                    className="settingsFileSelButton"
                    onClick={copy}
                    disabled={lines.length === 0}
                >
                    Copy
                </button>
                <button
                    className="settingsFileSelButton"
                    disabled={lines.length === 0}
                    onClick={() => {
                        void gameApi.clearLog()
                        setLines([])
                    }}
                >
                    Clear
                </button>
            </div>
            <div className="gameLog__output" ref={boxRef}>
                {lines.map((l, i) => (
                    <div
                        key={i}
                        className={
                            /error|exception|caused by|\bat java|FATAL/i.test(l)
                                ? 'gameLog__line--err'
                                : undefined
                        }
                    >
                        {l}
                    </div>
                ))}
            </div>
        </div>
    )
}
