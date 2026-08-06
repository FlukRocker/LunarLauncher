// `.env` parsing, shared by the build script and the running app.
//
// Regular comments, not `//!`: `build.rs` pulls this file in with `include!`,
// which splices it partway through that file, and an inner doc comment is only
// legal at the top of a module. The module-level documentation therefore lives
// on the `pub mod env_file;` declaration in lib.rs.
//
// For the same reason this file must stay std-only and free of `crate::`
// references — inside the build script neither the crate's modules nor its
// dependencies exist yet.

/// Parse `.env` contents into ordered key/value pairs.
///
/// Supports comments, blank lines, a leading `export`, and single- or
/// double-quoted values. Inline comments are *not* stripped from unquoted
/// values: a `#` is legal in a URL, and silently truncating one would produce
/// a subtly wrong endpoint rather than an obvious error.
pub fn parse(contents: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();

    for line in contents.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let line = line.strip_prefix("export ").unwrap_or(line).trim_start();

        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let key = key.trim();
        if key.is_empty() || !key.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
            continue;
        }

        let value = value.trim();
        let value = match (value.strip_prefix('"'), value.strip_prefix('\'')) {
            (Some(rest), _) => rest.strip_suffix('"').unwrap_or(rest),
            (_, Some(rest)) => rest.strip_suffix('\'').unwrap_or(rest),
            _ => value,
        };

        out.push((key.to_string(), value.to_string()));
    }

    out
}

/// Apply a `.env` file to the process environment.
///
/// A variable already set in the real environment always wins, so an explicit
/// `LUNAR_DISTRO_URL=… npm run app:dev` overrides the file rather than being
/// silently ignored — the opposite would make a one-off test unexplainable.
///
/// Returns the keys actually applied.
pub fn apply(path: &std::path::Path) -> Vec<String> {
    let Ok(contents) = std::fs::read_to_string(path) else {
        return Vec::new();
    };

    let mut applied = Vec::new();
    for (key, value) in parse(&contents) {
        if std::env::var_os(&key).is_some() {
            continue;
        }
        // SAFETY: called once during startup, before any thread that reads the
        // environment has been spawned. `set_var` is unsound only when it
        // races a concurrent read.
        unsafe { std::env::set_var(&key, &value) };
        applied.push(key);
    }
    applied
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kv(s: &str) -> Vec<(String, String)> {
        parse(s)
    }

    #[test]
    fn plain_assignments_are_read() {
        assert_eq!(
            kv("LUNAR_DISTRO_URL=https://example.test/d.json"),
            [(
                "LUNAR_DISTRO_URL".to_string(),
                "https://example.test/d.json".to_string()
            )]
        );
    }

    #[test]
    fn comments_and_blank_lines_are_skipped() {
        let got = kv("# a comment\n\n  # indented\nA=1\n");
        assert_eq!(got, [("A".to_string(), "1".to_string())]);
    }

    #[test]
    fn a_leading_export_is_tolerated() {
        assert_eq!(kv("export A=1"), [("A".to_string(), "1".to_string())]);
    }

    #[test]
    fn quotes_are_stripped_but_inner_spaces_kept() {
        assert_eq!(kv(r#"A="a b""#), [("A".to_string(), "a b".to_string())]);
        assert_eq!(kv("A='a b'"), [("A".to_string(), "a b".to_string())]);
    }

    /// A `#` is legal in a URL. Stripping it as an inline comment would yield
    /// a truncated endpoint that still parses, which fails much later and far
    /// from the cause.
    #[test]
    fn a_hash_inside_an_unquoted_value_is_not_a_comment() {
        assert_eq!(
            kv("A=https://x.test/p#frag"),
            [("A".to_string(), "https://x.test/p#frag".to_string())]
        );
    }

    #[test]
    fn an_empty_value_is_kept_so_it_can_clear_a_default() {
        assert_eq!(kv("A="), [("A".to_string(), String::new())]);
    }

    #[test]
    fn malformed_lines_are_ignored_rather_than_aborting_the_file() {
        // A line with no `=`, and a key that is not an identifier.
        let got = kv("this is not an assignment\nnot a key=1\nGOOD=2");
        assert_eq!(got, [("GOOD".to_string(), "2".to_string())]);
    }

    #[test]
    fn later_duplicates_are_preserved_in_order_for_the_caller_to_resolve() {
        assert_eq!(
            kv("A=1\nA=2"),
            [
                ("A".to_string(), "1".to_string()),
                ("A".to_string(), "2".to_string())
            ]
        );
    }

    /// The precedence that makes a one-off override work.
    #[test]
    fn the_real_environment_wins_over_the_file() {
        let dir = std::env::temp_dir().join("lunar-env-precedence");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(".env");
        std::fs::write(&path, "LUNAR_TEST_PRECEDENCE=from-file\n").unwrap();

        unsafe { std::env::set_var("LUNAR_TEST_PRECEDENCE", "from-shell") };
        let applied = apply(&path);

        assert!(!applied.contains(&"LUNAR_TEST_PRECEDENCE".to_string()));
        assert_eq!(
            std::env::var("LUNAR_TEST_PRECEDENCE").unwrap(),
            "from-shell"
        );
        unsafe { std::env::remove_var("LUNAR_TEST_PRECEDENCE") };
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_missing_file_is_not_an_error() {
        assert!(apply(std::path::Path::new("/nonexistent/.env")).is_empty());
    }
}
