//! What NSIS used to do: unpack, shortcut, register, uninstall.
//!
//! Everything here is per-user. Nothing writes to Program Files, HKLM, or
//! anywhere else needing elevation — which is what lets the installer run with
//! no UAC prompt, and lets the launcher update itself later without one.
//!
//! Deliberately synchronous and blocking. It runs on a worker thread and
//! reports progress over a channel; making it async would add a runtime to a
//! binary whose whole job is to copy files once.

use std::io::Read;
use std::path::{Path, PathBuf};

/// Windows registry key under which Add/Remove Programs looks for per-user
/// entries. HKCU, not HKLM — the machine-wide equivalent needs admin.
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
pub const UNINSTALL_KEY: &str =
    r"Software\Microsoft\Windows\CurrentVersion\Uninstall\LunarLauncher";

pub const PRODUCT_NAME: &str = "Lunar Launcher";
pub const MAIN_EXE: &str = "lunarlauncher.exe";
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// The archive of everything to install, embedded at build time.
///
/// Embedded rather than downloaded: it makes the installer work offline and on
/// a locked-down network, and removes a whole class of failure — a stub that
/// cannot reach its payload host is useless, and that is exactly the moment a
/// user is least willing to debug anything.
pub const PAYLOAD: &[u8] = include_bytes!("../payload.zip");

#[derive(Debug, Clone)]
pub enum Progress {
    /// 0.0 to 1.0, with the file currently being written.
    Step(f32, String),
    Done(PathBuf),
    Failed(String),
}

pub fn install_dir() -> PathBuf {
    // %LOCALAPPDATA%\Lunar Launcher. Not Program Files: per-user is what makes
    // this installable and updatable without elevation.
    let base = std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    base.join(PRODUCT_NAME)
}

/// Unpack the payload, then register. Reports progress as it goes.
pub fn run(report: impl Fn(Progress)) {
    match install(&report) {
        Ok(exe) => report(Progress::Done(exe)),
        Err(err) => report(Progress::Failed(err)),
    }
}

fn install(report: &impl Fn(Progress)) -> Result<PathBuf, String> {
    let dir = install_dir();

    // An existing install is replaced, not merged into.
    //
    // Merging is what leaves a machine running one version's exe beside
    // another version's resources — which fails at runtime, far from here, and
    // looks like a corrupt download rather than a bad upgrade. Old files are
    // cleared first so what remains is exactly this payload.
    if let Some(previous) = existing_install() {
        report(Progress::Step(0.0, "Removing the previous version".into()));
        if is_running(&previous.join(MAIN_EXE)) {
            return Err(format!(
                "{PRODUCT_NAME} is already running. Close it and run this installer again — \
                 its files cannot be replaced while it is open."
            ));
        }
        clear_install(&previous);
    }

    std::fs::create_dir_all(&dir).map_err(|e| format!("could not create {}: {e}", dir.display()))?;

    let reader = std::io::Cursor::new(PAYLOAD);
    let mut zip = zip::ZipArchive::new(reader).map_err(|e| format!("payload unreadable: {e}"))?;
    let total = zip.len();
    if total == 0 {
        return Err("payload is empty — this installer was built without one".into());
    }

    for i in 0..total {
        let mut entry = zip
            .by_index(i)
            .map_err(|e| format!("payload entry {i} unreadable: {e}"))?;

        // `enclosed_name` is what rejects `..` and absolute paths. The payload
        // is ours, but an archive that writes outside its target directory is
        // the classic zip-slip, and the check costs nothing.
        let Some(rel) = entry.enclosed_name() else {
            return Err(format!("payload contains an unsafe path: {}", entry.name()));
        };
        let dest = dir.join(&rel);

        report(Progress::Step(
            i as f32 / total as f32,
            rel.file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default(),
        ));

        if entry.is_dir() {
            std::fs::create_dir_all(&dest).map_err(|e| format!("{}: {e}", dest.display()))?;
            continue;
        }
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
        }

        let mut buf = Vec::with_capacity(entry.size() as usize);
        entry
            .read_to_end(&mut buf)
            .map_err(|e| format!("reading {}: {e}", rel.display()))?;

        // Written to a temp name and renamed, so an install interrupted midway
        // leaves no half-written executable that looks complete.
        let tmp = dest.with_extension("part");
        std::fs::write(&tmp, &buf).map_err(|e| format!("writing {}: {e}", dest.display()))?;
        std::fs::rename(&tmp, &dest).map_err(|e| format!("finalising {}: {e}", dest.display()))?;
    }

    let exe = dir.join(MAIN_EXE);
    if !exe.exists() {
        return Err(format!("{MAIN_EXE} is missing from the payload"));
    }

    report(Progress::Step(0.97, "Registering".into()));
    register(&dir, &exe)?;

    Ok(exe)
}

/// Where a previous install put itself, if there is one.
///
/// Read from the registry rather than assumed to be `install_dir()`: an older
/// build may have installed elsewhere, and guessing would leave that copy on
/// disk still registered in Add/Remove Programs.
#[cfg(target_os = "windows")]
fn existing_install() -> Option<PathBuf> {
    use winreg::enums::*;
    use winreg::RegKey;
    let key = RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey(UNINSTALL_KEY)
        .ok()?;
    let loc: String = key.get_value("InstallLocation").ok()?;
    let path = PathBuf::from(loc);
    path.is_dir().then_some(path)
}

#[cfg(not(target_os = "windows"))]
fn existing_install() -> Option<PathBuf> {
    None
}

/// Whether an executable is running, tested by trying to open it for writing.
///
/// Windows holds a mandatory lock on a running image, so this is the check
/// that matters: without it the install fails partway through with a per-file
/// permission error rather than one sentence naming the cause.
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
fn is_running(exe: &Path) -> bool {
    exe.exists()
        && std::fs::OpenOptions::new()
            .write(true)
            .open(exe)
            .is_err()
}

/// Remove a previous install's files, keeping the uninstaller in place — on an
/// upgrade it may be the process doing the removing.
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
fn clear_install(dir: &Path) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.file_name().is_some_and(|n| n == "uninstall.exe") {
            continue;
        }
        let _ = if path.is_dir() {
            std::fs::remove_dir_all(&path)
        } else {
            std::fs::remove_file(&path)
        };
    }
}

#[cfg(target_os = "windows")]
fn register(dir: &Path, exe: &Path) -> Result<(), String> {
    use winreg::enums::*;
    use winreg::RegKey;

    // Copy ourselves in as the uninstaller. The running installer is the same
    // binary in `--uninstall` mode, so there is nothing else to ship — and
    // copying rather than referencing the download means uninstall still works
    // after the user clears their Downloads folder.
    let uninstaller = dir.join("uninstall.exe");
    if let Ok(self_exe) = std::env::current_exe() {
        let _ = std::fs::copy(&self_exe, &uninstaller);
    }

    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let (key, _) = hkcu
        .create_subkey(UNINSTALL_KEY)
        .map_err(|e| format!("registry: {e}"))?;

    let set = |name: &str, value: &str| key.set_value(name, &value.to_string());
    set("DisplayName", PRODUCT_NAME).map_err(|e| e.to_string())?;
    set("DisplayVersion", VERSION).map_err(|e| e.to_string())?;
    set("Publisher", "Cyber Network Group").map_err(|e| e.to_string())?;
    set("InstallLocation", &dir.to_string_lossy()).map_err(|e| e.to_string())?;
    set("DisplayIcon", &exe.to_string_lossy()).map_err(|e| e.to_string())?;
    set(
        "UninstallString",
        &format!("\"{}\" --uninstall", uninstaller.display()),
    )
    .map_err(|e| e.to_string())?;
    // Marks the entry as per-user so Add/Remove Programs does not offer it to
    // other accounts on the machine, which could not uninstall it anyway.
    key.set_value("NoModify", &1u32).map_err(|e| e.to_string())?;
    key.set_value("NoRepair", &1u32).map_err(|e| e.to_string())?;

    // Size in KiB, which is the unit Add/Remove Programs expects. Reported
    // wrong it shows an absurd figure rather than failing.
    let bytes: u64 = walk_size(dir);
    key.set_value("EstimatedSize", &((bytes / 1024) as u32))
        .map_err(|e| e.to_string())?;

    shortcut(exe, &start_menu_dir().join(format!("{PRODUCT_NAME}.lnk")))?;
    shortcut(exe, &desktop_dir().join(format!("{PRODUCT_NAME}.lnk")))?;
    Ok(())
}

#[cfg(not(target_os = "windows"))]
fn register(_dir: &Path, _exe: &Path) -> Result<(), String> {
    // The UI is developed on macOS; only Windows has anything to register.
    Ok(())
}

#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
fn walk_size(dir: &Path) -> u64 {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return 0;
    };
    entries
        .flatten()
        .map(|e| match e.file_type() {
            Ok(t) if t.is_dir() => walk_size(&e.path()),
            _ => e.metadata().map(|m| m.len()).unwrap_or(0),
        })
        .sum()
}

#[cfg(target_os = "windows")]
fn start_menu_dir() -> PathBuf {
    PathBuf::from(std::env::var("APPDATA").unwrap_or_default())
        .join(r"Microsoft\Windows\Start Menu\Programs")
}

#[cfg(target_os = "windows")]
fn desktop_dir() -> PathBuf {
    PathBuf::from(std::env::var("USERPROFILE").unwrap_or_default()).join("Desktop")
}

/// Create a `.lnk` through COM.
///
/// There is no file format to write here: a shortcut is a serialised COM
/// object, and the supported way to produce one is IShellLink + IPersistFile.
#[cfg(target_os = "windows")]
fn shortcut(target: &Path, link: &Path) -> Result<(), String> {
    use windows::core::{Interface, PCWSTR};
    use windows::Win32::System::Com::*;
    use windows::Win32::UI::Shell::*;

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    unsafe {
        // Ignored if already initialised on this thread — the installer may
        // have done so already, and failing here would lose a shortcut over
        // bookkeeping.
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);

        let sl: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER)
            .map_err(|e| format!("shell link: {e}"))?;

        let target_w = wide(&target.to_string_lossy());
        sl.SetPath(PCWSTR(target_w.as_ptr()))
            .map_err(|e| format!("link target: {e}"))?;

        if let Some(dir) = target.parent() {
            let dir_w = wide(&dir.to_string_lossy());
            let _ = sl.SetWorkingDirectory(PCWSTR(dir_w.as_ptr()));
        }
        let desc = wide("Modded Minecraft launcher");
        let _ = sl.SetDescription(PCWSTR(desc.as_ptr()));

        let persist: IPersistFile = sl.cast().map_err(|e| format!("persist: {e}"))?;
        let link_w = wide(&link.to_string_lossy());
        persist
            .Save(PCWSTR(link_w.as_ptr()), true)
            .map_err(|e| format!("saving {}: {e}", link.display()))?;
    }
    Ok(())
}

/// Remove everything `register` created, and the install directory.
///
/// Deliberately does **not** touch `%APPDATA%\Lunar Launcher` or
/// `%APPDATA%\.lunarlauncher`: those hold the user's accounts, settings and
/// downloaded packs. Uninstalling the launcher should not destroy several
/// gigabytes of game data and force a re-login, and a reinstall should find
/// everything where it was.
#[cfg(target_os = "windows")]
pub fn uninstall() -> Result<(), String> {
    use winreg::enums::*;
    use winreg::RegKey;

    let dir = install_dir();
    let _ = std::fs::remove_file(start_menu_dir().join(format!("{PRODUCT_NAME}.lnk")));
    let _ = std::fs::remove_file(desktop_dir().join(format!("{PRODUCT_NAME}.lnk")));

    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let _ = hkcu.delete_subkey_all(UNINSTALL_KEY);

    // The uninstaller is running from inside the directory it is deleting, so
    // its own file cannot be removed yet. Everything else goes; the directory
    // and the stale uninstall.exe are cleaned up by the next install.
    if let Ok(entries) = std::fs::read_dir(&dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.file_name().is_some_and(|n| n == "uninstall.exe") {
                continue;
            }
            let _ = if path.is_dir() {
                std::fs::remove_dir_all(&path)
            } else {
                std::fs::remove_file(&path)
            };
        }
    }
    Ok(())
}

#[cfg(not(target_os = "windows"))]
pub fn uninstall() -> Result<(), String> {
    Err("uninstall is only implemented on Windows".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_install_directory_is_per_user_not_program_files() {
        let dir = install_dir();
        let s = dir.to_string_lossy().to_lowercase();
        assert!(
            !s.contains("program files"),
            "per-user install must not target Program Files: {s}"
        );
        assert!(dir.ends_with(PRODUCT_NAME));
    }

    /// The placeholder payload is a valid but empty archive, so a build with
    /// no payload fails with a sentence rather than a parse error that reads
    /// like corruption.
    #[test]
    fn an_empty_payload_reports_itself_clearly() {
        let mut zip = zip::ZipArchive::new(std::io::Cursor::new(PAYLOAD)).expect("valid archive");
        if zip.len() == 0 {
            let err = install(&|_| {}).unwrap_err();
            assert!(err.contains("empty"), "{err}");
        }
    }

    #[test]
    fn walking_a_missing_directory_is_zero_not_a_panic() {
        assert_eq!(walk_size(Path::new("/nonexistent-lunar-path")), 0);
    }
}
