import { defineConfig, loadEnv } from 'vite'
import react from '@vitejs/plugin-react'

// Tauri expects a fixed port and must not fall back to another one.
export default defineConfig(({ mode }) => {
    // `LUNAR_` rather than Vite's default `VITE_` prefix, so one variable name
    // serves the Rust build, the config patch and the frontend. Only the keys
    // named below are exposed — loadEnv with a prefix would leak every
    // LUNAR_* value into the bundle, including the Azure client id.
    const env = loadEnv(mode, process.cwd(), 'LUNAR_')
    const brand = env.LUNAR_BRAND_NAME || 'Lunar Launcher'

    return {
    plugins: [react()],
    clearScreen: false,
    server: {
        port: 1420,
        strictPort: true,
        watch: {
            ignored: ['**/src-tauri/**']
        }
    },
    define: {
        __BRAND_NAME__: JSON.stringify(brand)
    },
    esbuild: {
        // Source comments are shipped verbatim inside a bundle otherwise. They
        // are written for maintainers and regularly name internals, endpoints
        // and the reasoning behind a workaround — none of which belongs in an
        // artifact handed to users.
        legalComments: 'none'
    },
    build: {
        // Safari 13 / Edge 89 are the floor for the system webviews Tauri uses.
        target: process.env.TAURI_ENV_PLATFORM === 'windows' ? 'chrome105' : 'safari13',
        minify: !process.env.TAURI_ENV_DEBUG ? 'esbuild' : false,
        // A sourcemap reconstructs the original source, comments included, so
        // it undoes the line above. Debug builds only.
        sourcemap: !!process.env.TAURI_ENV_DEBUG
    }
    }
})
