//! Port of `helios-core`'s JavaGuard: locating installed JVMs, reading their
//! real properties, and filtering them against a server's supported range.
//!
//! The approach matches the JS: enumerate candidate *roots*, ask each one for
//! its HotSpot settings by actually executing it, then filter. Executing the
//! binary is deliberate — a directory name says nothing reliable about the
//! architecture or vendor of what's inside it.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::distribution::node_arch;
use crate::error::Result;

// ---------------------------------------------------------------------------
// Paths
// ---------------------------------------------------------------------------

/// Resolve the java executable inside a JVM root directory.
/// `javaExecFromRoot` in JavaGuard.js.
pub fn java_exec_from_root(root: &Path) -> PathBuf {
    if cfg!(target_os = "windows") {
        root.join("bin").join("javaw.exe")
    } else if cfg!(target_os = "macos") {
        root.join("Contents").join("Home").join("bin").join("java")
    } else {
        root.join("bin").join("java")
    }
}

/// Given any java path, walk back up to the JVM root.
/// `ensureJavaDirIsRoot` in JavaGuard.js.
pub fn ensure_java_dir_is_root(dir: &Path) -> PathBuf {
    let s = dir.to_string_lossy().to_string();
    if cfg!(target_os = "macos") {
        match s.find("/Contents/Home") {
            Some(i) => PathBuf::from(&s[..i]),
            None => dir.to_path_buf(),
        }
    } else {
        // Matches the JS, which searched for the platform-joined "/bin/java".
        let needle = if cfg!(target_os = "windows") {
            "\\bin\\java"
        } else {
            "/bin/java"
        };
        match s.find(needle) {
            Some(i) => PathBuf::from(&s[..i]),
            None => dir.to_path_buf(),
        }
    }
}

/// Where the launcher installs JDKs it downloads itself.
pub fn launcher_runtime_dir(data_dir: &Path) -> PathBuf {
    data_dir.join("runtime").join(node_arch())
}

// ---------------------------------------------------------------------------
// Version parsing
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub struct JavaVersion {
    pub major: u32,
    pub minor: u32,
    pub patch: u32,
}

impl std::fmt::Display for JavaVersion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

/// `parseJavaRuntimeVersion` — legacy `1.8.0_152-b16` form when the string
/// starts with "1.", otherwise the Java 9+ `10.0.2+13` form.
pub fn parse_java_runtime_version(ver: &str) -> Option<JavaVersion> {
    if ver.starts_with("1.") {
        parse_legacy(ver)
    } else {
        parse_semver(ver)
    }
}

/// `1.{major}.{minor}_{patch}` e.g. 1.8.0_152-b16
fn parse_legacy(ver: &str) -> Option<JavaVersion> {
    let rest = ver.strip_prefix("1.")?;
    let mut it = rest.split(['.', '_', '-']);
    let major = it.next()?.parse().ok()?;
    let minor = it.next()?.parse().ok()?;
    let patch = it.next().and_then(|p| p.parse().ok()).unwrap_or(0);
    Some(JavaVersion { major, minor, patch })
}

/// `{major}.{minor}.{patch}` with an optional `+build` or `.build` suffix.
fn parse_semver(ver: &str) -> Option<JavaVersion> {
    let mut it = ver.split(['.', '+']);
    let major = it.next()?.parse().ok()?;
    let minor = it.next().and_then(|p| p.parse().ok()).unwrap_or(0);
    let patch = it.next().and_then(|p| p.parse().ok()).unwrap_or(0);
    Some(JavaVersion { major, minor, patch })
}

// ---------------------------------------------------------------------------
// Range matching
// ---------------------------------------------------------------------------

/// Does `version` satisfy a distribution `supported` range?
///
/// The JS delegated to node-semver. Distribution indexes in practice use a
/// small subset — `8.x`, `>=17.x`, `>=21.x`, occasionally a two-sided range —
/// so this implements that subset rather than pulling in a full semver range
/// parser whose wildcard semantics differ from node's anyway.
///
/// Supported syntax: comparators `>=`, `<=`, `>`, `<`, `=` followed by a
/// version whose parts may be `x`/`*`; space-separated terms are ANDed;
/// `||` separates ORed groups.
pub fn version_satisfies(version: JavaVersion, range: &str) -> bool {
    range
        .split("||")
        .any(|group| group.split_whitespace().all(|term| term_matches(version, term)))
}

fn term_matches(version: JavaVersion, term: &str) -> bool {
    let term = term.trim();
    if term.is_empty() || term == "*" || term == "x" {
        return true;
    }

    let (op, rest) = if let Some(r) = term.strip_prefix(">=") {
        (">=", r)
    } else if let Some(r) = term.strip_prefix("<=") {
        ("<=", r)
    } else if let Some(r) = term.strip_prefix('>') {
        (">", r)
    } else if let Some(r) = term.strip_prefix('<') {
        ("<", r)
    } else if let Some(r) = term.strip_prefix('=') {
        ("=", r)
    } else {
        ("=", term)
    };

    // Parse the bound, tracking how many parts were concrete. A wildcard part
    // means everything after it is unconstrained: "8.x" pins only the major.
    let mut parts: Vec<Option<u32>> = Vec::new();
    for p in rest.trim().split('.') {
        if p == "x" || p == "X" || p == "*" {
            parts.push(None);
        } else {
            match p.split(['+', '-']).next().unwrap_or(p).parse::<u32>() {
                Ok(n) => parts.push(Some(n)),
                Err(_) => parts.push(None),
            }
        }
    }

    let bound_major = parts.first().copied().flatten();
    let bound_minor = parts.get(1).copied().flatten();
    let bound_patch = parts.get(2).copied().flatten();

    let Some(bmajor) = bound_major else {
        return true; // e.g. "x" — unconstrained
    };

    let actual = (version.major, version.minor, version.patch);
    let bound = (bmajor, bound_minor.unwrap_or(0), bound_patch.unwrap_or(0));

    match op {
        ">=" => actual >= bound,
        ">" => actual > bound,
        "<=" => actual <= bound,
        "<" => actual < bound,
        // Equality only compares the parts that were concrete, so "8.x"
        // matches any 8.y.z — this is the wildcard behaviour node-semver has.
        _ => {
            version.major == bmajor
                && bound_minor.is_none_or(|m| version.minor == m)
                && bound_patch.is_none_or(|p| version.patch == p)
        }
    }
}

// ---------------------------------------------------------------------------
// Discovery
// ---------------------------------------------------------------------------

/// A validated JVM installation.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JvmDetails {
    pub path: PathBuf,
    pub version: JavaVersion,
    pub version_str: String,
    pub vendor: String,
    pub arch: String,
}

/// Run `java -XshowSettings:properties -version` and parse the property table.
///
/// The JVM writes these to stderr. Returns None for anything that isn't a
/// working java binary, which is how invalid candidates get dropped.
async fn hotspot_settings(exec: &Path) -> Option<HashMap<String, String>> {
    if !exec.exists() {
        return None;
    }
    let output = tokio::process::Command::new(exec)
        .args(["-XshowSettings:properties", "-version"])
        .output()
        .await
        .ok()?;

    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );

    let mut props = HashMap::new();
    for line in text.lines() {
        let line = line.trim();
        // Property lines look like "java.version = 17.0.9". Continuation
        // lines for list properties have no '=' and are ignored; nothing we
        // read is a list property.
        if let Some((k, v)) = line.split_once(" = ") {
            props.insert(k.trim().to_string(), v.trim().to_string());
        }
    }
    if props.is_empty() {
        None
    } else {
        Some(props)
    }
}

/// Inspect one JVM root, returning its details if it is usable.
///
/// Applies the same hard filters as `filterApplicableJavaPaths`: 64-bit only,
/// and on arm64 hosts reject x86 JVMs so we don't launch under Rosetta.
pub async fn inspect_jvm(root: &Path) -> Option<JvmDetails> {
    let exec = java_exec_from_root(root);
    let props = hotspot_settings(&exec).await?;

    let data_model = props.get("sun.arch.data.model").map(String::as_str).unwrap_or("");
    if data_model != "64" {
        tracing::debug!(path = %root.display(), "Skipping non-64-bit JVM");
        return None;
    }

    let os_arch = props.get("os.arch").cloned().unwrap_or_default();
    if node_arch() == "arm64" && os_arch != "aarch64" {
        tracing::debug!(path = %root.display(), %os_arch, "Skipping non-arm JVM on arm host");
        return None;
    }

    let raw_version = props.get("java.version")?;
    let version = match parse_java_runtime_version(raw_version) {
        Some(v) => v,
        None => {
            tracing::error!(path = %root.display(), %raw_version, "Failed to parse JDK version");
            return None;
        }
    };

    Some(JvmDetails {
        path: root.to_path_buf(),
        version,
        version_str: version.to_string(),
        vendor: props.get("java.vendor").cloned().unwrap_or_default(),
        arch: os_arch,
    })
}

/// Directories that conventionally contain JVM roots as immediate children.
fn platform_jvm_parents() -> Vec<PathBuf> {
    let home = dirs::home_dir();
    if cfg!(target_os = "macos") {
        let mut v = vec![
            PathBuf::from("/Library/Java/JavaVirtualMachines"),
            PathBuf::from("/System/Library/Java/JavaVirtualMachines"),
        ];
        if let Some(h) = home {
            v.push(h.join("Library/Java/JavaVirtualMachines"));
        }
        v
    } else if cfg!(target_os = "windows") {
        vec![
            PathBuf::from(r"C:\Program Files\Java"),
            PathBuf::from(r"C:\Program Files\Eclipse Adoptium"),
            PathBuf::from(r"C:\Program Files\Amazon Corretto"),
            PathBuf::from(r"C:\Program Files\Microsoft"),
        ]
    } else {
        let mut v = vec![
            PathBuf::from("/usr/lib/jvm"),
            PathBuf::from("/usr/java"),
            PathBuf::from("/opt/java"),
        ];
        if let Some(h) = home {
            v.push(h.join(".sdkman/candidates/java"));
        }
        v
    }
}

/// Candidate JVM roots from the environment and conventional locations.
fn candidate_roots(data_dir: &Path) -> Vec<PathBuf> {
    let mut roots: Vec<PathBuf> = Vec::new();

    // JVMs the launcher installed itself take priority.
    let runtime = launcher_runtime_dir(data_dir);
    if let Ok(entries) = std::fs::read_dir(&runtime) {
        for e in entries.flatten() {
            roots.push(e.path());
        }
    }

    // Env vars point directly at a root (or near one).
    for var in ["JAVA_HOME", "JRE_HOME", "JDK_HOME"] {
        if let Some(val) = std::env::var_os(var) {
            roots.push(ensure_java_dir_is_root(Path::new(&val)));
        }
    }

    for parent in platform_jvm_parents() {
        if let Ok(entries) = std::fs::read_dir(&parent) {
            for e in entries.flatten() {
                roots.push(e.path());
            }
        }
    }

    roots.sort();
    roots.dedup();
    roots
}

/// Discover every usable JVM on this machine.
pub async fn discover_jvms(data_dir: &Path) -> Vec<JvmDetails> {
    let roots = candidate_roots(data_dir);
    tracing::info!(candidates = roots.len(), "Scanning for JVMs");

    // Inspecting spawns a process each; run them concurrently.
    let mut found: Vec<JvmDetails> = futures::future::join_all(
        roots.iter().map(|r| async move { inspect_jvm(r).await }),
    )
    .await
    .into_iter()
    .flatten()
    .collect();

    // Newest first, so "best match" is just the first that satisfies.
    found.sort_by(|a, b| b.version.cmp(&a.version));
    found.dedup_by(|a, b| a.path == b.path);
    tracing::info!(found = found.len(), "JVM scan complete");
    found
}

/// Best JVM satisfying `supported`, or None if nothing matches.
/// Equivalent to the JS `validateSelectedJvm` over discovered paths.
pub async fn select_jvm(data_dir: &Path, supported: &str) -> Option<JvmDetails> {
    discover_jvms(data_dir)
        .await
        .into_iter()
        .find(|j| version_satisfies(j.version, supported))
}

/// Validate a specific java executable the user configured by hand.
pub async fn validate_jvm(exec: &Path, supported: &str) -> Result<Option<JvmDetails>> {
    let root = ensure_java_dir_is_root(exec);
    Ok(inspect_jvm(&root)
        .await
        .filter(|j| version_satisfies(j.version, supported)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(major: u32, minor: u32, patch: u32) -> JavaVersion {
        JavaVersion { major, minor, patch }
    }

    #[test]
    fn parses_legacy_java8_versions() {
        assert_eq!(parse_java_runtime_version("1.8.0_152-b16"), Some(v(8, 0, 152)));
        assert_eq!(parse_java_runtime_version("1.8.0_432"), Some(v(8, 0, 432)));
    }

    #[test]
    fn parses_modern_versions() {
        assert_eq!(parse_java_runtime_version("17.0.9"), Some(v(17, 0, 9)));
        assert_eq!(parse_java_runtime_version("10.0.2+13"), Some(v(10, 0, 2)));
        assert_eq!(parse_java_runtime_version("21"), Some(v(21, 0, 0)));
    }

    #[test]
    fn wildcard_major_range_matches_any_minor() {
        assert!(version_satisfies(v(8, 0, 152), "8.x"));
        assert!(version_satisfies(v(8, 5, 1), "8.x"));
        assert!(!version_satisfies(v(11, 0, 1), "8.x"));
    }

    #[test]
    fn gte_ranges_used_by_modern_distributions() {
        assert!(version_satisfies(v(17, 0, 9), ">=17.x"));
        assert!(version_satisfies(v(21, 0, 1), ">=17.x"));
        assert!(!version_satisfies(v(11, 0, 2), ">=17.x"));
        assert!(version_satisfies(v(21, 0, 0), ">=21.x"));
        assert!(!version_satisfies(v(17, 0, 9), ">=21.x"));
    }

    #[test]
    fn two_sided_and_or_ranges() {
        assert!(version_satisfies(v(17, 0, 1), ">=17.x <18.x"));
        assert!(!version_satisfies(v(18, 0, 1), ">=17.x <18.x"));
        assert!(version_satisfies(v(8, 0, 1), "8.x || >=17.x"));
        assert!(version_satisfies(v(21, 0, 1), "8.x || >=17.x"));
        assert!(!version_satisfies(v(11, 0, 1), "8.x || >=17.x"));
    }

    #[test]
    fn ensure_root_strips_platform_suffixes() {
        if cfg!(target_os = "macos") {
            assert_eq!(
                ensure_java_dir_is_root(Path::new("/Library/Java/JavaVirtualMachines/jdk-17.jdk/Contents/Home/bin/java")),
                PathBuf::from("/Library/Java/JavaVirtualMachines/jdk-17.jdk")
            );
        }
        // A path already at the root is returned unchanged.
        let root = PathBuf::from("/opt/jdk-17");
        assert_eq!(ensure_java_dir_is_root(&root), root);
    }

    #[test]
    fn exec_path_is_platform_correct() {
        let root = Path::new("/opt/jdk");
        let exec = java_exec_from_root(root);
        if cfg!(target_os = "macos") {
            assert!(exec.ends_with("Contents/Home/bin/java"));
        } else if cfg!(target_os = "windows") {
            assert!(exec.ends_with("javaw.exe"));
        } else {
            assert!(exec.ends_with("bin/java"));
        }
    }
}

#[cfg(test)]
mod integration {
    use super::*;

    /// Exercises real discovery against whatever JVMs this machine has.
    /// Ignored by default so CI without a JDK stays green.
    #[tokio::test]
    #[ignore]
    async fn discovers_real_jvms() {
        let data = dirs::home_dir().unwrap();
        let jvms = discover_jvms(&data).await;
        for j in &jvms {
            println!("{} | {} | {} | {}", j.version_str, j.vendor, j.arch, j.path.display());
        }
        println!("total: {}", jvms.len());
    }
}
