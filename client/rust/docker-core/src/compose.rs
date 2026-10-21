//! Compose projects as the engine sees them: where their files are (from the
//! labels `docker compose` puts on every container) and the command lines
//! that validate and apply them on the engine host. Pure functions; reading,
//! writing and running live in the app core.
use serde::Serialize;
use std::collections::BTreeMap;

pub const LABEL_PROJECT: &str = "com.docker.compose.project";
pub const LABEL_CONFIG_FILES: &str = "com.docker.compose.project.config_files";
pub const LABEL_WORKING_DIR: &str = "com.docker.compose.project.working_dir";
pub const LABEL_ENV_FILE: &str = "com.docker.compose.project.environment_file";

/// Longest accepted path (bytes).
const MAX_PATH: usize = 4096;

/// Where a compose project lives on the engine host.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ComposeLocation {
    pub project: String,
    /// Absolute project directory (`--project-directory`).
    pub working_dir: String,
    /// Absolute compose files in the order they were given (`-f`).
    pub config_files: Vec<String>,
    /// Absolute `--env-file` paths (empty: compose's default `.env`).
    pub env_files: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum ComposeLocateError {
    /// No container carries the project label.
    #[error("no containers of the project")]
    NoContainers,
    /// The containers do not say which files created them (old compose or
    /// created by another tool).
    #[error("the compose files are unknown")]
    FilesUnknown,
    /// A label holds a path that is not absolute or not printable.
    #[error("unusable path in the compose labels")]
    InvalidPath,
}

/// Compose project names: `[a-z0-9][a-z0-9_-]*` (what compose itself
/// accepts), so a name is never taken for an option.
pub fn valid_project_name(name: &str) -> bool {
    let bytes = name.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= 255
        && (bytes[0].is_ascii_lowercase() || bytes[0].is_ascii_digit())
        && bytes
            .iter()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'_' | b'-'))
}

/// An absolute POSIX path without control characters.
pub fn valid_host_path(path: &str) -> bool {
    path.starts_with('/')
        && path.len() <= MAX_PATH
        && !path.chars().any(char::is_control)
        && !path.split('/').any(|part| part == "..")
}

fn split_paths(value: &str, base: &str) -> Result<Vec<String>, ComposeLocateError> {
    let mut paths = Vec::new();
    for part in value.split(',').map(str::trim).filter(|p| !p.is_empty()) {
        // Compose v1 recorded paths as given; they are relative to the
        // project directory.
        let path = if part.starts_with('/') {
            part.to_owned()
        } else {
            join(base, part.trim_start_matches("./"))
        };
        if !valid_host_path(&path) {
            return Err(ComposeLocateError::InvalidPath);
        }
        if !paths.contains(&path) {
            paths.push(path);
        }
    }
    Ok(paths)
}

/// `dir` + `/` + `name` without doubling the slash.
pub fn join(dir: &str, name: &str) -> String {
    if dir.ends_with('/') {
        format!("{dir}{name}")
    } else {
        format!("{dir}/{name}")
    }
}

/// The directory part of an absolute path (`/` for top-level files).
pub fn parent(path: &str) -> &str {
    match path.rfind('/') {
        Some(0) | None => "/",
        Some(index) => &path[..index],
    }
}

/// The last component of a path.
pub fn file_name(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

/// Reads the location from the labels of the project's containers (newest
/// first): the first container that names its files wins.
pub fn locate(
    project: &str,
    labels: &[BTreeMap<String, String>],
) -> Result<ComposeLocation, ComposeLocateError> {
    if labels.is_empty() {
        return Err(ComposeLocateError::NoContainers);
    }
    let chosen = labels
        .iter()
        .find(|set| {
            set.get(LABEL_CONFIG_FILES)
                .is_some_and(|files| !files.trim().is_empty())
        })
        .ok_or(ComposeLocateError::FilesUnknown)?;
    let working_dir = chosen
        .get(LABEL_WORKING_DIR)
        .map(|dir| dir.trim().trim_end_matches('/').to_owned())
        .filter(|dir| !dir.is_empty());
    let files_label = &chosen[LABEL_CONFIG_FILES];
    let working_dir = match working_dir {
        Some(dir) => dir,
        None => {
            let first = files_label.split(',').map(str::trim).next().unwrap_or("");
            if !first.starts_with('/') {
                return Err(ComposeLocateError::FilesUnknown);
            }
            parent(first).to_owned()
        }
    };
    if !valid_host_path(&working_dir) {
        return Err(ComposeLocateError::InvalidPath);
    }
    let config_files = split_paths(files_label, &working_dir)?;
    if config_files.is_empty() {
        return Err(ComposeLocateError::FilesUnknown);
    }
    let env_files = match chosen.get(LABEL_ENV_FILE) {
        Some(value) => split_paths(value, &working_dir)?,
        None => Vec::new(),
    };
    Ok(ComposeLocation {
        project: project.to_owned(),
        working_dir,
        config_files,
        env_files,
    })
}

/// Which compose front end the host has.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ComposeCli {
    /// `docker compose` (the CLI plugin).
    Plugin,
    /// The standalone `docker-compose` binary.
    Standalone,
}

impl ComposeCli {
    pub fn program(self) -> Vec<String> {
        match self {
            Self::Plugin => vec!["docker".into(), "compose".into()],
            Self::Standalone => vec!["docker-compose".into()],
        }
    }
}

/// Full argument vector: `<cli> -p <project> -f <file>… --project-directory
/// <dir> [--env-file <file>…] <command…>`. `files` replaces the location's
/// files (a candidate standing in for an edited file).
pub fn compose_argv(
    cli: ComposeCli,
    location: &ComposeLocation,
    files: &[String],
    command: &[&str],
) -> Vec<String> {
    let mut argv = cli.program();
    argv.push("-p".into());
    argv.push(location.project.clone());
    for file in files {
        argv.push("-f".into());
        argv.push(file.clone());
    }
    argv.push("--project-directory".into());
    argv.push(location.working_dir.clone());
    for env in &location.env_files {
        argv.push("--env-file".into());
        argv.push(env.clone());
    }
    argv.extend(command.iter().map(|part| (*part).to_owned()));
    argv
}

/// One POSIX shell word: safe words stay bare, everything else is single
/// quoted (`'` becomes `'\''`). Never interpolates.
pub fn shell_quote(arg: &str) -> String {
    let safe = !arg.is_empty()
        && arg
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_./=:@%+,-".contains(&b));
    if safe && !arg.starts_with('=') {
        return arg.to_owned();
    }
    let mut quoted = String::with_capacity(arg.len() + 2);
    quoted.push('\'');
    for ch in arg.chars() {
        if ch == '\'' {
            quoted.push_str("'\\''");
        } else {
            quoted.push(ch);
        }
    }
    quoted.push('\'');
    quoted
}

/// A command line for `sh -c`/an SSH exec request: every word quoted.
pub fn shell_command(argv: &[String]) -> String {
    argv.iter()
        .map(|arg| shell_quote(arg))
        .collect::<Vec<_>>()
        .join(" ")
}

/// Removes ANSI escape sequences (colours, cursor moves) and carriage
/// returns from CLI output.
pub fn strip_ansi(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(ch) = chars.next() {
        match ch {
            '\u{1b}' => match chars.peek() {
                Some('[') => {
                    chars.next();
                    // Parameters and intermediates, then one final byte.
                    for next in chars.by_ref() {
                        if ('@'..='~').contains(&next) {
                            break;
                        }
                    }
                }
                Some(']') => {
                    chars.next();
                    // OSC: up to BEL or ESC \.
                    while let Some(next) = chars.next() {
                        if next == '\u{7}' {
                            break;
                        }
                        if next == '\u{1b}' {
                            chars.next();
                            break;
                        }
                    }
                }
                Some(_) => {
                    chars.next();
                }
                None => {}
            },
            '\r' => {}
            _ => out.push(ch),
        }
    }
    out
}

/// `<file>.bak-YYYYmmdd-HHMMSS` (with `-N` when that name is taken).
pub fn backup_path(path: &str, stamp: &str, attempt: u32) -> String {
    if attempt == 0 {
        format!("{path}.bak-{stamp}")
    } else {
        format!("{path}.bak-{stamp}-{attempt}")
    }
}

/// A hidden candidate next to the original: `.<name>.consolecrypt-<tag>.tmp`.
/// Same directory, so relative paths inside the file resolve the same way
/// and the final rename stays on one file system.
pub fn candidate_path(path: &str, tag: &str) -> String {
    join(
        parent(path),
        &format!(".{}.consolecrypt-{tag}.tmp", file_name(path)),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn labels(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect()
    }

    #[test]
    fn project_names_follow_compose_rules() {
        assert!(valid_project_name("consolecrypt-production"));
        assert!(valid_project_name("shop_2"));
        assert!(valid_project_name("0x"));
        assert!(!valid_project_name(""));
        assert!(!valid_project_name("-p"));
        assert!(!valid_project_name("_x"));
        assert!(!valid_project_name("Shop"));
        assert!(!valid_project_name("a b"));
        assert!(!valid_project_name("a;rm -rf /"));
    }

    #[test]
    fn locates_files_working_dir_and_env_files() {
        let set = labels(&[
            (LABEL_PROJECT, "shop"),
            (
                LABEL_CONFIG_FILES,
                "/srv/shop/compose.yaml, /srv/shop/compose.prod.yaml,/srv/shop/compose.yaml",
            ),
            (LABEL_WORKING_DIR, "/srv/shop/"),
            (LABEL_ENV_FILE, "/srv/shop/.env,/srv/shop/prod.env"),
        ]);
        let location = locate("shop", &[set]).unwrap();
        assert_eq!(location.working_dir, "/srv/shop");
        assert_eq!(
            location.config_files,
            ["/srv/shop/compose.yaml", "/srv/shop/compose.prod.yaml"]
        );
        assert_eq!(location.env_files, ["/srv/shop/.env", "/srv/shop/prod.env"]);
    }

    #[test]
    fn relative_v1_paths_resolve_against_the_working_dir() {
        let set = labels(&[
            (LABEL_CONFIG_FILES, "docker-compose.yml,./override.yml"),
            (LABEL_WORKING_DIR, "/opt/app"),
        ]);
        let location = locate("app", &[set]).unwrap();
        assert_eq!(
            location.config_files,
            ["/opt/app/docker-compose.yml", "/opt/app/override.yml"]
        );
        assert!(location.env_files.is_empty());
    }

    #[test]
    fn missing_working_dir_falls_back_to_the_first_file() {
        let set = labels(&[(LABEL_CONFIG_FILES, "/opt/app/compose.yml")]);
        assert_eq!(locate("app", &[set]).unwrap().working_dir, "/opt/app");
    }

    #[test]
    fn first_container_naming_files_wins() {
        let bare = labels(&[(LABEL_PROJECT, "app")]);
        let named = labels(&[
            (LABEL_CONFIG_FILES, "/a/compose.yml"),
            (LABEL_WORKING_DIR, "/a"),
        ]);
        assert_eq!(
            locate("app", &[bare.clone(), named]).unwrap().config_files,
            ["/a/compose.yml"]
        );
        assert_eq!(
            locate("app", &[bare]),
            Err(ComposeLocateError::FilesUnknown)
        );
        assert_eq!(locate("app", &[]), Err(ComposeLocateError::NoContainers));
    }

    #[test]
    fn unusable_paths_are_refused() {
        for files in ["/a/../etc/passwd", "/a/compose\n.yml", "relative.yml\u{0}"] {
            let set = labels(&[(LABEL_CONFIG_FILES, files), (LABEL_WORKING_DIR, "/a")]);
            assert_eq!(
                locate("app", &[set]),
                Err(ComposeLocateError::InvalidPath),
                "{files:?}"
            );
        }
        let set = labels(&[(LABEL_CONFIG_FILES, "/a/c.yml"), (LABEL_WORKING_DIR, "rel")]);
        assert_eq!(locate("app", &[set]), Err(ComposeLocateError::InvalidPath));
    }

    #[test]
    fn shell_quoting_never_lets_text_escape() {
        assert_eq!(shell_quote("compose"), "compose");
        assert_eq!(
            shell_quote("/srv/shop/compose.yaml"),
            "/srv/shop/compose.yaml"
        );
        assert_eq!(shell_quote(""), "''");
        assert_eq!(shell_quote("a b"), "'a b'");
        assert_eq!(shell_quote("$(id)"), "'$(id)'");
        assert_eq!(shell_quote("`id`"), "'`id`'");
        assert_eq!(shell_quote("it's"), "'it'\\''s'");
        assert_eq!(shell_quote("x;rm -rf /"), "'x;rm -rf /'");
        assert_eq!(shell_quote("*"), "'*'");
        assert_eq!(shell_quote("~/x"), "'~/x'");
        assert_eq!(shell_quote("=x"), "'=x'");
        assert_eq!(shell_quote("a\nb"), "'a\nb'");
    }

    #[cfg(unix)]
    #[test]
    fn quoted_words_reach_the_program_unchanged() {
        let words = [
            "plain",
            "with space",
            "it's",
            "$(echo pwned)",
            "`echo pwned`",
            "\"dq\"",
            "back\\slash",
            "semi;colon",
            "*",
            "",
            "line\nbreak",
        ];
        let mut argv = vec!["printf".to_owned(), "%s\\0".to_owned()];
        argv.extend(words.iter().map(|w| (*w).to_owned()));
        let output = std::process::Command::new("/bin/sh")
            .arg("-c")
            .arg(shell_command(&argv))
            .output()
            .unwrap();
        assert!(output.status.success());
        let got: Vec<&str> = std::str::from_utf8(&output.stdout)
            .unwrap()
            .split('\0')
            .collect();
        assert_eq!(&got[..words.len()], &words);
    }

    #[test]
    fn argv_lists_every_file_and_env_file() {
        let location = ComposeLocation {
            project: "shop".into(),
            working_dir: "/srv/shop".into(),
            config_files: vec!["/srv/shop/a.yml".into(), "/srv/shop/b.yml".into()],
            env_files: vec!["/srv/shop/.env".into()],
        };
        let candidate = vec!["/srv/shop/a.yml".into(), "/srv/shop/.b.yml.tmp".into()];
        assert_eq!(
            compose_argv(ComposeCli::Plugin, &location, &candidate, &["config", "-q"]),
            [
                "docker",
                "compose",
                "-p",
                "shop",
                "-f",
                "/srv/shop/a.yml",
                "-f",
                "/srv/shop/.b.yml.tmp",
                "--project-directory",
                "/srv/shop",
                "--env-file",
                "/srv/shop/.env",
                "config",
                "-q"
            ]
        );
        assert_eq!(
            compose_argv(
                ComposeCli::Standalone,
                &location,
                &location.config_files,
                &["up", "-d"]
            )[..3],
            ["docker-compose", "-p", "shop"]
        );
    }

    #[test]
    fn ansi_sequences_are_removed() {
        assert_eq!(
            strip_ansi("\u{1b}[32m✔\u{1b}[0m Container web  \u{1b}[1mStarted\u{1b}[0m\r"),
            "✔ Container web  Started"
        );
        assert_eq!(strip_ansi("\u{1b}]0;title\u{7}ok"), "ok");
        assert_eq!(strip_ansi("\u{1b}[?25l\u{1b}[2Kline"), "line");
    }

    #[test]
    fn backup_and_candidate_names_stay_next_to_the_file() {
        assert_eq!(
            backup_path("/srv/compose.yml", "20261021-091500", 0),
            "/srv/compose.yml.bak-20261021-091500"
        );
        assert_eq!(
            backup_path("/srv/compose.yml", "20261021-091500", 2),
            "/srv/compose.yml.bak-20261021-091500-2"
        );
        assert_eq!(
            candidate_path("/srv/app/compose.yml", "ab12"),
            "/srv/app/.compose.yml.consolecrypt-ab12.tmp"
        );
        assert_eq!(candidate_path("/c.yml", "x"), "/.c.yml.consolecrypt-x.tmp");
    }
}
