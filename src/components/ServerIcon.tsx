import { useEffect, useState } from 'react'
import { iconApi } from '../lib/api'

/**
 * A pack's icon, with a lettered fallback.
 *
 * The fallback is not a placeholder for a slow load — it is the normal state
 * for a distribution that publishes no icons, which is most of them. Rendering
 * a broken-image glyph there would look like a launcher fault rather than an
 * absent field.
 */
export function ServerIcon({ serverId, name }: { serverId: string; name: string }) {
    const [src, setSrc] = useState<string | null>(null)

    useEffect(() => {
        let live = true
        void iconApi
            .forServer(serverId)
            .then((uri) => {
                if (live) setSrc(uri)
            })
            // An icon is decoration; a failure here must not surface as an
            // error over a server the user can still play.
            .catch(() => {})
        return () => {
            live = false
        }
    }, [serverId])

    if (src) {
        return <img className="serverIcon" src={src} alt="" aria-hidden="true" />
    }
    return (
        <span className="serverIcon serverIcon--fallback" aria-hidden="true">
            {name.trim().charAt(0).toUpperCase() || '?'}
        </span>
    )
}
