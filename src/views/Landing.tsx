import { useEffect, useState } from 'react'
import { ServerIcon } from '../components/ServerIcon'
import { listen } from '@tauri-apps/api/event'
import { ErrorModal, type LauncherError } from '../components/ErrorModal'
import {
    api,
    isApiError,
    launchApi,
    type Account,
    gameApi,
    newsApi,
    statusApi,
    type LaunchProgress,
    type Article,
    type Server,
    type ServerStatus,
    type SocialLinks
} from '../lib/api'

import logo from '../assets/images/LunarLogo.png'
import settingsIcon from '../assets/images/icons/settings.svg'
import linkIcon from '../assets/images/icons/link.svg'
import xIcon from '../assets/images/icons/x.svg'
import instagramIcon from '../assets/images/icons/instagram.svg'
import youtubeIcon from '../assets/images/icons/youtube.svg'
import discordIcon from '../assets/images/icons/discord.svg'
import arrowIcon from '../assets/images/icons/arrow.svg'

/**
 * Main screen. The DOM here mirrors the Electron `landing.ejs` — same ids and
 * class names — so the ported `launcher.css` styles it without changes.
 *
 * The behaviour behind it is entirely different: PLAY calls into Rust, which
 * validates, downloads, locates a JVM and spawns the game, streaming progress
 * back over the `launch://progress` event.
 */
export function Landing({
    account,
    onOpenSettings
}: {
    account: Account | null
    onOpenSettings: () => void
}) {
    const [server, setServer] = useState<Server | null>(null)
    const [error, setError] = useState<string | null>(null)
    const [modal, setModal] = useState<LauncherError | null>(null)
    const [progress, setProgress] = useState<LaunchProgress | null>(null)
    const [launching, setLaunching] = useState(false)
    const [status, setStatus] = useState<ServerStatus | null>(null)
    const [news, setNews] = useState<Article[]>([])
    const [article, setArticle] = useState(0)
    const [newsOpen, setNewsOpen] = useState(false)
    const [playing, setPlaying] = useState(false)
    const [servers, setServers] = useState<Server[]>([])
    const [picking, setPicking] = useState(false)
    const [links, setLinks] = useState<SocialLinks | null>(null)

    useEffect(() => {
        // The full list, so the selector has something to offer. A
        // distribution with one server hides the control entirely rather than
        // presenting a menu with a single entry.
        api.getDistribution()
            .then((d) => setServers(d.servers))
            .catch(() => setServers([]))
    }, [])

    useEffect(() => {
        api.getSelectedServer()
            .then((s) => {
                setServer(s)
                // Ping for the live player count, as the Electron build did.
                if (s) void statusApi.getServerStatus(s.id).then(setStatus).catch(() => {})
            })
            .catch((err: unknown) => setError(isApiError(err) ? err.message : String(err)))
    }, [])

    useEffect(() => {
        // The feed is optional; a distribution without an rss field simply
        // leaves the news button disabled.
        newsApi.getNews().then(setNews).catch(() => setNews([]))
    }, [])

    useEffect(() => {
        // Social links are optional, like rss. Absent means the icon is not
        // rendered — an <a> with no href looks clickable, does nothing, and is
        // not keyboard-focusable, which is worse than showing nothing at all.
        api.getDistribution()
            .then((d) => setLinks(d.links ?? null))
            .catch(() => setLinks(null))
    }, [])

    // News is a view within the landing view, so it needs its own flag for the
    // title bar to follow it.
    useEffect(() => {
        document.documentElement.dataset.news = newsOpen ? 'open' : 'closed'
        return () => {
            delete document.documentElement.dataset.news
        }
    }, [newsOpen])

    useEffect(() => {
        const unlisten = listen<LaunchProgress>('launch://progress', (e) =>
            setProgress(e.payload)
        )
        return () => {
            void unlisten.then((f) => f())
        }
    }, [])

    // Track the game process. The state is also read on mount, so returning to
    // this view while the game is already running still shows it as playing.
    useEffect(() => {
        void gameApi.isRunning().then(setPlaying).catch(() => {})
        const started = listen('game://started', () => setPlaying(true))
        const exited = listen('game://exited', () => {
            setPlaying(false)
            setProgress(null)
        })
        return () => {
            void started.then((f) => f())
            void exited.then((f) => f())
        }
    }, [])

    const play = async () => {
        setLaunching(true)
        setError(null)
        setProgress(null)
        try {
            await launchApi.launchGame()
        } catch (err: unknown) {
            const message = isApiError(err) ? err.message : String(err)
            setError(message)
            // A launch failure is the one the user most needs to act on and
            // report, so it gets the dialog rather than a line of red text.
            setModal({
                title: 'The game could not be launched',
                message,
                detail: isApiError(err) ? `${err.kind}: ${err.message}` : String(err)
            })
        } finally {
            setLaunching(false)
        }
    }

    // The Minecraft skin service renders a head from the account UUID. Offline
    // accounts have a synthetic (MD5) uuid, so this falls back to the default
    // skin rather than failing — same as the Electron build.
    const avatarUrl = account
        ? `https://mc-heads.net/body/${account.uuid}/right`
        : undefined

    const detailsText = error ?? progress?.detail ?? ''
    const percent = progress?.percent ?? 0

    // Only icons with a real destination. The ids and order match the Electron
    // `landing.ejs`, because launcher.css and app.css both style them by id.
    //
    // Nothing supplies these yet, so today this is empty and the whole row —
    // divider included — is omitted. That is the intended interim state: five
    // icons that look clickable and do nothing read as a broken launcher, and
    // an <a> without an href cannot be reached by keyboard either.
    const externalMedia = (
        [
            { id: 'linkURL', icon: linkIcon, label: 'Website', url: links?.website },
            { id: 'xURL', icon: xIcon, label: 'X', url: links?.x },
            { id: 'instagramURL', icon: instagramIcon, label: 'Instagram', url: links?.instagram },
            { id: 'youtubeURL', icon: youtubeIcon, label: 'YouTube', url: links?.youtube },
            { id: 'discordURL', icon: discordIcon, label: 'Discord', url: links?.discord }
        ] as const
    ).flatMap((m) => (m.url ? [{ ...m, url: m.url }] : []))

    return (
        <div id="landingContainer">
            {modal && <ErrorModal error={modal} onClose={() => setModal(null)} />}

            {picking && (
                <div
                    className="serverPicker__scrim"
                    role="presentation"
                    onClick={() => setPicking(false)}
                >
                    <div
                        className="serverPicker"
                        role="dialog"
                        aria-modal="true"
                        aria-label="Choose a server"
                        onClick={(e) => e.stopPropagation()}
                    >
                        <h2 className="serverPicker__title">Choose a server</h2>
                        {servers.length === 0 && (
                            <p className="panel__hint">
                                No servers were loaded from the distribution.
                            </p>
                        )}
                        <ul className="serverPicker__list">
                            {servers.map((s) => (
                                <li key={s.id}>
                                    <button
                                        className={`serverPicker__item${
                                            server?.id === s.id
                                                ? ' serverPicker__item--active'
                                                : ''
                                        }`}
                                        onClick={() => {
                                            setPicking(false)
                                            void api
                                                .setSelectedServer(s.id)
                                                .then(() => api.getSelectedServer())
                                                .then((sel) => {
                                                    setServer(sel)
                                                    setError(null)
                                                    // The old server's count
                                                    // must not linger.
                                                    setStatus(null)
                                                    if (sel)
                                                        void statusApi
                                                            .getServerStatus(sel.id)
                                                            .then(setStatus)
                                                            .catch(() => {})
                                                })
                                                .catch((err: unknown) =>
                                                    setError(
                                                        isApiError(err)
                                                            ? err.message
                                                            : String(err)
                                                    )
                                                )
                                        }}
                                    >
                                        <ServerIcon serverId={s.id} name={s.name} />
                                        <span className="serverPicker__text">
                                            <span className="serverPicker__name">{s.name}</span>
                                            <span className="serverPicker__meta">
                                                {s.minecraftVersion}
                                            </span>
                                        </span>
                                    </button>
                                </li>
                            ))}
                        </ul>
                        <div className="panel__actions">
                            <button className="button" onClick={() => setPicking(false)}>
                                Cancel
                            </button>
                        </div>
                    </div>
                </div>
            )}
            <div id="upper">
                <div id="left">
                    <div id="image_seal_container">
                        <img id="image_seal" src={logo} alt="" />
                        <div id="updateAvailableTooltip">Update Available</div>
                    </div>
                </div>
                <div id="content" />
                <div id="right">
                    <div id="rightContainer">
                        <div id="user_content">
                            <span id="user_text">
                                {account ? account.displayName : 'No Account'}
                            </span>
                            <div
                                id="avatarContainer"
                                style={
                                    avatarUrl
                                        ? { backgroundImage: `url('${avatarUrl}')` }
                                        : undefined
                                }
                            >
                                <button id="avatarOverlay" onClick={onOpenSettings}>
                                    Edit
                                </button>
                            </div>
                        </div>
                        <div id="mediaContent">
                            <div id="internalMedia">
                                <div className="mediaContainer" id="settingsMediaContainer">
                                    <button
                                        className="mediaButton"
                                        id="settingsMediaButton"
                                        onClick={onOpenSettings}
                                        aria-label="Settings"
                                    >
                                        <img className="mediaSVG" src={settingsIcon} alt="" />
                                    </button>
                                </div>
                            </div>
                            {externalMedia.length > 0 && (
                                <>
                                    <div className="mediaDivider" />
                                    <div id="externalMedia">
                                        {externalMedia.map((m) => (
                                            <div className="mediaContainer" key={m.id}>
                                                <a
                                                    className="mediaURL"
                                                    id={m.id}
                                                    href={m.url}
                                                    target="_blank"
                                                    rel="noreferrer noopener"
                                                    aria-label={m.label}
                                                >
                                                    <img className="mediaSVG" src={m.icon} alt="" />
                                                </a>
                                            </div>
                                        ))}
                                    </div>
                                </>
                            )}
                        </div>
                    </div>
                </div>
            </div>

            <div id="lower">
                <div id="left">
                    <div className="bot_wrapper">
                        <div id="content">
                            <div id="server_status_wrapper">
                                <span className="bot_label" id="landingPlayerLabel">
                                    {status && !status.online ? 'SERVER' : 'PLAYERS'}
                                </span>
                                <span id="player_count">
                                    {status === null
                                        ? '• • •'
                                        : status.online
                                          ? `${status.playersOnline ?? 0}/${status.playersMax ?? 0}`
                                          : 'OFFLINE'}
                                </span>
                            </div>
                            <div className="bot_divider" />
                            <div id="mojangStatusWrapper">
                                <span className="bot_label">SERVER STATUS</span>
                                <span
                                    id="mojang_status_icon"
                                    style={{
                                        color:
                                            status === null
                                                ? '#a5a5a5'
                                                : status.online
                                                  ? '#a5c325'
                                                  : '#c32625'
                                    }}
                                >
                                    &#8226;
                                </span>
                            </div>
                        </div>
                    </div>
                </div>

                <div id="center">
                    <div className="bot_wrapper">
                        <div id="content">
                            <button
                                id="newsButton"
                                disabled={news.length === 0}
                                title={
                                    news.length === 0
                                        ? 'No news feed configured for this server'
                                        : undefined
                                }
                                onClick={() => setNewsOpen((v) => !v)}
                            >
                                <img src={arrowIcon} id="newsButtonSVG" alt="" />
                                <span id="newsButtonText">NEWS</span>
                            </button>
                        </div>
                    </div>
                </div>

                <div id="right">
                    <div className="bot_wrapper">
                        <div id="launch_content" style={launching ? { display: 'none' } : undefined}>
                            <button
                                id="launch_button"
                                className={playing ? 'launch_button--playing' : undefined}
                                onClick={() => void play()}
                                disabled={!server || !account || playing}
                                title={playing ? 'The game is already running' : undefined}
                            >
                                {playing ? 'PLAYING' : 'PLAY'}
                            </button>
                            <div className="bot_divider" />
                            <button
                                id="server_selection_button"
                                className="bot_label"
                                // Switching servers mid-download would leave
                                // files half-written against the wrong
                                // instance, so this is inert while busy.
                                // Only launch state disables this. Gating on
                                // servers.length would leave the control
                                // permanently dead whenever the distribution
                                // fetch failed, with no way to tell why.
                                disabled={launching || playing}
                                title="Change server"
                                onClick={() => setPicking((v) => !v)}
                            >
                                {server ? `• ${server.name}` : '• No Server Selected'}
                            </button>


                        </div>
                        <div
                            id="launch_details"
                            style={launching ? { display: 'flex' } : { display: 'none' }}
                        >
                            <div id="launch_details_left">
                                <span id="launch_progress_label">{Math.round(percent)}%</span>
                                <div className="bot_divider" />
                            </div>
                            <div id="launch_details_right">
                                <progress id="launch_progress" value={percent} max={100} />
                                <span id="launch_details_text" className="bot_label">
                                    {detailsText}
                                </span>
                            </div>
                        </div>
                    </div>
                </div>
            </div>

            {newsOpen && news.length > 0 && (
                <div className="newsPane">
                    <div className="newsPane__head">
                        <span className="newsPane__count">
                            {article + 1} / {news.length}
                        </span>
                        <div className="newsPane__nav">
                            <button
                                className="button"
                                disabled={article === 0}
                                onClick={() => setArticle((i) => i - 1)}
                            >
                                Prev
                            </button>
                            <button
                                className="button"
                                disabled={article >= news.length - 1}
                                onClick={() => setArticle((i) => i + 1)}
                            >
                                Next
                            </button>
                            <button className="button" onClick={() => setNewsOpen(false)}>
                                Close
                            </button>
                        </div>
                    </div>
                    <h2 className="newsPane__title">{news[article].title}</h2>
                    <p className="newsPane__meta">
                        {news[article].author && `${news[article].author} · `}
                        {news[article].date}
                    </p>
                    {/*
                      Feed bodies are HTML, which is the whole point of the
                      news pane, so they are rendered as markup. This is only
                      as trustworthy as the RSS URL in the distribution index —
                      which the server operator controls, same as every other
                      field there.
                    */}
                    <div
                        className="newsPane__body"
                        dangerouslySetInnerHTML={{ __html: news[article].content }}
                    />
                    {news[article].link && (
                        <a className="newsPane__link" href={news[article].link} target="_blank">
                            Read on the website
                        </a>
                    )}
                </div>
            )}
        </div>
    )
}
