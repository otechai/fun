//! Languages are just interpreters. All fun needs to know is what goes after `#!`.

use std::path::Path;

use crate::error::{Error, Result};
use crate::store::is_command;

/// Short names people actually type, mapped to what's actually installed.
const ALIASES: &[(&str, &str)] = &[
    ("py", "python3"),
    ("python", "python3"),
    ("js", "node"),
    ("javascript", "node"),
    ("rb", "ruby"),
];

pub struct Template {
    pub text: String,
    /// Human name for messages: `bash`, `python3`, ...
    pub label: String,
    /// 1-based line to put the cursor on, so you start typing where the script starts.
    pub cursor_line: usize,
}

/// A new script: shebang, the safety prelude for shells, and a blank line to type on.
///
/// `bash`/`zsh` get `set -euo pipefail`, POSIX shells `set -eu`, everything else just the shebang.
pub fn template(lang: &str) -> Result<Template> {
    let interp = ALIASES
        .iter()
        .find(|(alias, _)| *alias == lang)
        .map_or(lang, |(_, real)| real);
    if interp.is_empty()
        || interp.starts_with('-')
        || interp.chars().any(|c| c.is_whitespace() || c.is_control())
    {
        return Err(Error::fail(format!(
            "'{lang}' isn't a language: give one word (bash, python3, node, fish) or a full path (/bin/zsh)"
        )));
    }
    let label = Path::new(interp)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(interp)
        .to_owned();
    let shebang = if interp.starts_with('/') {
        format!("#!{interp}")
    } else {
        format!("#!/usr/bin/env {interp}")
    };
    let prelude = match label.as_str() {
        "bash" | "zsh" => "set -euo pipefail\n",
        "sh" | "dash" | "ash" | "ksh" => "set -eu\n",
        _ => "",
    };
    let text = format!("{shebang}\n{prelude}\n");
    Ok(Template {
        cursor_line: text.lines().count(),
        text,
        label,
    })
}

/// The interpreter word of a shebang line: `#!/usr/bin/env -S node --flag` gives `node`,
/// `#!/bin/bash` gives `/bin/bash`.
pub fn interpreter(shebang: &str) -> Option<&str> {
    let mut words = shebang.trim().strip_prefix("#!")?.split_whitespace();
    let prog = words.next()?;
    if Path::new(prog).file_name().is_some_and(|n| n == "env") {
        return words.find(|w| !w.starts_with('-') && !w.contains('='));
    }
    Some(prog)
}

/// What to call a script's language in a table: the interpreter's name, or `?` without a shebang.
pub fn label(shebang: &str) -> String {
    interpreter(shebang)
        .and_then(|prog| Path::new(prog).file_name())
        .and_then(|name| name.to_str())
        .unwrap_or("?")
        .to_owned()
}

/// The interpreter a shebang line asks for, if it isn't installed.
pub fn missing_interpreter(shebang: &str) -> Option<String> {
    let prog = interpreter(shebang)?;
    (!is_command(prog)).then(|| prog.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shells_get_a_safety_prelude() {
        let t = template("bash").unwrap();
        assert_eq!(t.text, "#!/usr/bin/env bash\nset -euo pipefail\n\n");
        assert_eq!((t.label.as_str(), t.cursor_line), ("bash", 3));
        assert_eq!(template("sh").unwrap().text, "#!/usr/bin/env sh\nset -eu\n\n");
        assert_eq!(
            template("/bin/zsh").unwrap().text,
            "#!/bin/zsh\nset -euo pipefail\n\n"
        );
    }

    #[test]
    fn everything_else_is_just_a_shebang() {
        let t = template("fish").unwrap();
        assert_eq!((t.text.as_str(), t.cursor_line), ("#!/usr/bin/env fish\n\n", 2));
        assert_eq!(template("lua5.4").unwrap().text, "#!/usr/bin/env lua5.4\n\n");
    }

    #[test]
    fn aliases() {
        for (alias, real) in [
            ("py", "python3"),
            ("python", "python3"),
            ("js", "node"),
            ("rb", "ruby"),
        ] {
            assert_eq!(template(alias).unwrap().label, real);
        }
    }

    #[test]
    fn nonsense_is_rejected() {
        for bad in ["", "two words", "-x", "a\nb"] {
            assert!(template(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn labels_come_from_the_interpreter() {
        assert_eq!(label("#!/usr/bin/env python3\n"), "python3");
        assert_eq!(label("#!/bin/bash"), "bash");
        assert_eq!(label("#! /usr/bin/env -S node --harmony\n"), "node");
        assert_eq!(label("#!/usr/bin/env FOO=1 ruby"), "ruby");
        assert_eq!(label("echo no shebang"), "?");
        assert_eq!(label(""), "?");
        assert_eq!(label("#!/usr/bin/env"), "?");
    }

    #[test]
    fn spots_missing_interpreters() {
        assert_eq!(
            missing_interpreter("#!/usr/bin/env definitely-not-installed-xyz\n").as_deref(),
            Some("definitely-not-installed-xyz")
        );
        assert_eq!(
            missing_interpreter("#!/usr/bin/env -S definitely-not-installed-xyz --flag").as_deref(),
            Some("definitely-not-installed-xyz")
        );
        assert_eq!(
            missing_interpreter("#!/no/such/interp").as_deref(),
            Some("/no/such/interp")
        );
        assert_eq!(missing_interpreter("#!/bin/sh"), None);
        assert_eq!(missing_interpreter("#!/usr/bin/env sh"), None);
    }
}
