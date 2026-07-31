import { useEffect, useState } from 'react'
import { listen } from '@tauri-apps/api/event'
import {
    api,
    isApiError,
    launchApi,
    type Account,
    type LaunchProgress,
    type Server
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
    const [progress, setProgress] = useState<LaunchProgress | null>(null)
    const [launching, setLaunching] = useState(false)

    useEffect(() => {
        api.getSelectedServer()
            .then(setServer)
            .catch((err: unknown) => setError(isApiError(err) ? err.message : String(err)))
    }, [])

    useEffect(() => {
        const unlisten = listen<LaunchProgress>('launch://progress', (e) =>
            setProgress(e.payload)
        )
        return () => {
            void unlisten.then((f) => f())
        }
    }, [])

    const play = async () => {
        setLaunching(true)
        setError(null)
        setProgress(null)
        try {
            await launchApi.launchGame()
        } catch (err: unknown) {
            setError(isApiError(err) ? err.message : String(err))
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

    return (
        <div id="landingContainer">
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
                            <div id="avatarContainer">
                                <button
                                    id="avatarOverlay"
                                    onClick={onOpenSettings}
                                    style={
                                        avatarUrl
                                            ? { backgroundImage: `url('${avatarUrl}')` }
                                            : undefined
                                    }
                                >
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
                            <div className="mediaDivider" />
                            <div id="externalMedia">
                                {[
                                    { id: 'linkURL', icon: linkIcon, label: 'Website' },
                                    { id: 'xURL', icon: xIcon, label: 'X' },
                                    { id: 'instagramURL', icon: instagramIcon, label: 'Instagram' },
                                    { id: 'youtubeURL', icon: youtubeIcon, label: 'YouTube' },
                                    { id: 'discordURL', icon: discordIcon, label: 'Discord' }
                                ].map((m) => (
                                    <div className="mediaContainer" key={m.id}>
                                        <a className="mediaURL" id={m.id} aria-label={m.label}>
                                            <img className="mediaSVG" src={m.icon} alt="" />
                                        </a>
                                    </div>
                                ))}
                            </div>
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
                                    SERVER
                                </span>
                                <span id="player_count">
                                    {server ? server.minecraftVersion : '• • •'}
                                </span>
                            </div>
                            <div className="bot_divider" />
                            <div id="mojangStatusWrapper">
                                <span className="bot_label">MOJANG STATUS</span>
                                <span id="mojang_status_icon">&#8226;</span>
                            </div>
                        </div>
                    </div>
                </div>

                <div id="center">
                    <div className="bot_wrapper">
                        <div id="content">
                            <button id="newsButton" disabled title="News feed not yet ported">
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
                                onClick={() => void play()}
                                disabled={!server || !account}
                            >
                                PLAY
                            </button>
                            <div className="bot_divider" />
                            <button id="server_selection_button" className="bot_label">
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
        </div>
    )
}
