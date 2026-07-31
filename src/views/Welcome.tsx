export function Welcome({ onContinue }: { onContinue: () => void }) {
    return (
        <div className="view view--centered">
            <div className="panel">
                <h1 className="panel__title">Welcome to Lunar Launcher</h1>
                <p className="panel__desc">
                    Join modded servers without worrying about installing Java, Forge, or other
                    mods. We&apos;ll handle that for you.
                </p>
                <button className="button button--primary" onClick={onContinue}>
                    Continue
                </button>
            </div>
        </div>
    )
}
