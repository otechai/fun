//! `~/.config/fun/config.toml`: three settings, a TOML-shaped file, no parser dependency.
//!
//! ```toml
//! language = "bash"   # shebang for new scripts: bash, python3, node, fish, /bin/zsh, ...
//! editor   = "vim"    # any command; arguments are fine: "code --wait"
//! autosave = true     # save (and so make runnable) the moment the editor closes
//! ```
//! Values are taken literally (no escapes); a bare word needs no quotes. Unknown keys are errors,
//! so a typo is caught on the line where it happens instead of being silently ignored.

use std::env;
use std::fs;
use std::io;
use std::path::PathBuf;

use crate::error::{Context, Error, Result};
use crate::ui::tilde;

pub const DEFAULT_LANGUAGE: &str = "bash";
pub const DEFAULT_EDITOR: &str = "vim";
const KEYS: &[&str] = &["language", "editor", "autosave"];

#[derive(Debug, Default, PartialEq, Eq)]
pub struct Raw {
    pub language: Option<String>,
    pub editor: Option<String>,
    pub autosave: Option<bool>,
}

/// Where a setting came from, so `fun help` can say why you're getting what you're getting.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    Default,
    Config,
    Env(&'static str),
}

impl Source {
    fn label(self) -> String {
        match self {
            Source::Default => "default".into(),
            Source::Config => "config".into(),
            Source::Env(var) => format!("${var}"),
        }
    }
}

#[derive(Debug)]
pub struct Setting<T> {
    pub value: T,
    pub source: Source,
}

#[derive(Debug)]
pub struct Settings {
    pub language: Setting<String>,
    pub editor: Setting<String>,
    pub autosave: Setting<bool>,
}

impl Settings {
    /// Precedence. language: config, then bash. editor: config, then $VISUAL, then $EDITOR, then vim.
    /// autosave: config, then on.
    pub fn resolve(raw: Raw) -> Settings {
        let env_var = |k: &'static str| {
            env::var(k)
                .ok()
                .filter(|v| !v.trim().is_empty())
                .map(|v| (v, Source::Env(k)))
        };
        let language = match raw.language {
            Some(value) => Setting {
                value,
                source: Source::Config,
            },
            None => Setting {
                value: DEFAULT_LANGUAGE.into(),
                source: Source::Default,
            },
        };
        let editor = raw
            .editor
            .map(|v| (v, Source::Config))
            .or_else(|| env_var("VISUAL"))
            .or_else(|| env_var("EDITOR"))
            .map_or_else(
                || Setting {
                    value: DEFAULT_EDITOR.into(),
                    source: Source::Default,
                },
                |(value, source)| Setting { value, source },
            );
        let autosave = match raw.autosave {
            Some(value) => Setting {
                value,
                source: Source::Config,
            },
            None => Setting {
                value: true,
                source: Source::Default,
            },
        };
        Settings {
            language,
            editor,
            autosave,
        }
    }
}

/// `$FUN_CONFIG`, else `$XDG_CONFIG_HOME/fun/config.toml`, else `~/.config/fun/config.toml`.
/// The bool says whether the path was asked for explicitly (then it must exist).
pub fn path() -> Result<(PathBuf, bool)> {
    let var = |k: &str| env::var_os(k).filter(|v| !v.is_empty()).map(PathBuf::from);
    if let Some(p) = var("FUN_CONFIG") {
        return Ok((p, true));
    }
    let base = match (var("XDG_CONFIG_HOME"), var("HOME")) {
        (Some(xdg), _) => xdg,
        (None, Some(home)) => home.join(".config"),
        (None, None) => {
            return Err(Error::fail(
                "$HOME isn't set; set it, or point FUN_CONFIG at your config",
            ))
        }
    };
    Ok((base.join("fun/config.toml"), false))
}

pub fn load() -> Result<Settings> {
    let (path, explicit) = path()?;
    let raw = match fs::read_to_string(&path) {
        Ok(text) => {
            parse(&text).map_err(|(line, msg)| Error::fail(format!("{}:{line}: {msg}", tilde(&path))))?
        }
        Err(e) if e.kind() == io::ErrorKind::NotFound && !explicit => Raw::default(),
        Err(e) => return Err(e).context(|| format!("can't read {}", tilde(&path))),
    };
    Ok(Settings::resolve(raw))
}

/// The config block of `fun help`. Never fails: help is what you read when things are broken.
pub fn describe() -> String {
    let Ok((path, _)) = path() else {
        return String::new();
    };
    let shown = tilde(&path);
    match load() {
        Ok(s) => format!(
            "\nconfig  {shown}\n        language = \"{}\"  ({})\n        editor   = \"{}\"  ({})\n        autosave = {}  ({})\n",
            s.language.value,
            s.language.source.label(),
            s.editor.value,
            s.editor.source.label(),
            s.autosave.value,
            s.autosave.source.label()
        ),
        Err(e) => format!("\nconfig  {e}\n"),
    }
}

/// Parse the file. Errors are `(line number, message)`.
pub fn parse(text: &str) -> std::result::Result<Raw, (usize, String)> {
    let mut raw = Raw::default();
    for (i, line) in text.lines().enumerate() {
        let n = i + 1;
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            return Err((n, format!("expected `key = \"value\"`, got `{line}`")));
        };
        let key = key.trim();
        let value = value_of(value.trim()).map_err(|m| (n, m))?;
        let twice = || (n, format!("`{key}` is set twice"));
        match key {
            "language" if raw.language.is_some() => return Err(twice()),
            "editor" if raw.editor.is_some() => return Err(twice()),
            "autosave" if raw.autosave.is_some() => return Err(twice()),
            "language" => raw.language = Some(value),
            "editor" => raw.editor = Some(value),
            "autosave" => {
                raw.autosave = Some(match value.as_str() {
                    "true" => true,
                    "false" => false,
                    other => return Err((n, format!("`autosave` must be true or false, not `{other}`"))),
                });
            }
            _ => {
                return Err((
                    n,
                    format!("unknown key `{key}` (expected one of: {})", KEYS.join(", ")),
                ))
            }
        }
    }
    Ok(raw)
}

fn value_of(s: &str) -> std::result::Result<String, String> {
    let (value, rest) = match s.chars().next() {
        Some(q @ ('"' | '\'')) => {
            let inner = &s[1..];
            let end = inner.find(q).ok_or("missing closing quote")?;
            (&inner[..end], &inner[end + 1..])
        }
        Some(_) => {
            let end = s.find(|c: char| c.is_whitespace() || c == '#').unwrap_or(s.len());
            (&s[..end], &s[end..])
        }
        None => return Err("missing value".into()),
    };
    let rest = rest.trim();
    if !(rest.is_empty() || rest.starts_with('#')) {
        return Err(format!(
            "unexpected `{rest}` after the value; quote values that contain spaces"
        ));
    }
    if value.trim().is_empty() {
        return Err("the value is empty".into());
    }
    Ok(value.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ok(text: &str) -> Raw {
        parse(text).unwrap()
    }
    fn err(text: &str) -> (usize, String) {
        parse(text).unwrap_err()
    }

    #[test]
    fn empty_and_comments_are_fine() {
        assert_eq!(ok(""), Raw::default());
        assert_eq!(ok("# nothing to see\n\n   \n"), Raw::default());
    }

    #[test]
    fn quoting_styles_and_trailing_comments() {
        let raw = ok("language = \"python3\"\neditor = 'code --wait'   # my editor\n");
        assert_eq!(raw.language.as_deref(), Some("python3"));
        assert_eq!(raw.editor.as_deref(), Some("code --wait"));
        assert_eq!(
            ok("language = fish # bare word\n").language.as_deref(),
            Some("fish")
        );
        assert_eq!(ok("  editor=nvim").editor.as_deref(), Some("nvim"));
        assert_eq!(
            ok("editor = \"vim -u NONE\"").editor.as_deref(),
            Some("vim -u NONE")
        );
    }

    #[test]
    fn autosave_is_a_boolean() {
        assert_eq!(ok("autosave = false").autosave, Some(false));
        assert_eq!(ok("autosave = true # default").autosave, Some(true));
        assert_eq!(ok("autosave = \"false\"").autosave, Some(false));
        assert!(err("autosave = yes").1.contains("true or false"));
        assert!(err("autosave = true\nautosave = false").1.contains("twice"));
    }

    #[test]
    fn errors_name_the_line() {
        assert_eq!(err("language = bash\nlang = bash").0, 2);
        assert!(err("lang = bash").1.contains("unknown key `lang`"));
        assert!(err("[section]").1.contains("expected `key"));
        assert!(err("editor = \"vim").1.contains("closing quote"));
        assert!(err("editor = code --wait").1.contains("quote values"));
        assert!(err("editor =").1.contains("missing value"));
        assert!(err("editor = \"\"").1.contains("empty"));
        assert!(err("editor = vim\neditor = nano").1.contains("twice"));
    }

    #[test]
    fn defaults_are_bash_vim_and_autosave() {
        // Env-dependent precedence is covered end to end in tests/cli.rs.
        let s = Settings::resolve(Raw {
            language: Some("fish".into()),
            editor: Some("nano".into()),
            autosave: Some(false),
        });
        assert_eq!(
            (s.language.value.as_str(), s.language.source),
            ("fish", Source::Config)
        );
        assert_eq!(
            (s.editor.value.as_str(), s.editor.source),
            ("nano", Source::Config)
        );
        assert_eq!((s.autosave.value, s.autosave.source), (false, Source::Config));
        assert!(Settings::resolve(Raw::default()).autosave.value);
    }
}
