//! Everything the user reads that isn't a script. One place, so the voice stays consistent:
//! short, lowercase, and always saying what to do next.

use std::env;
use std::io::{self, Write};
use std::path::Path;

/// A status line on stderr (stdout is reserved for data you might pipe).
pub fn note(msg: impl AsRef<str>) {
    eprintln!("{}", msg.as_ref());
}

/// `/home/me/.config/fun` -> `~/.config/fun`.
pub fn tilde(path: &Path) -> String {
    if let Some(home) = env::var_os("HOME").filter(|h| h.len() > 1) {
        if let Ok(rest) = path.strip_prefix(&home) {
            return if rest.as_os_str().is_empty() {
                "~".into()
            } else {
                format!("~/{}", rest.display())
            };
        }
    }
    path.display().to_string()
}

/// How to get `dir` onto `$PATH`, in the dialect of the user's shell.
pub fn path_hint(dir: &Path) -> String {
    let fish = env::var("SHELL").is_ok_and(|s| s.ends_with("/fish"));
    if fish {
        format!("fish_add_path {}", tilde(dir))
    } else {
        format!("export PATH=\"{}:$PATH\"", tilde(dir).replacen('~', "$HOME", 1))
    }
}

/// Ask on stderr, read stdin. EOF (no terminal) counts as "no".
pub fn confirm(prompt: &str) -> bool {
    eprint!("{prompt}");
    let _ = io::stderr().flush();
    let mut line = String::new();
    io::stdin().read_line(&mut line).is_ok() && matches!(line.trim().chars().next(), Some('y' | 'Y'))
}
