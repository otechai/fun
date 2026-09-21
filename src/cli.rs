//! Argument parsing. The grammar is five verbs; a hand-rolled parser is smaller than a
//! dependency and starts faster.

use crate::config;
use crate::error::{Error, Result};

const HELP: &str = "\
fun - funced/funcsave for any script language

usage:
  fun edit <name> [lang]   write a script; it's saved and runnable when the editor closes
  fun save <name>          install a pending draft (autosave off, or a draft that failed a check)
  fun list                 a table: language, created, runs, lines, size
  fun show <name>          print a script
  fun rm [-f] <name>...    delete scripts (asks first)

aliases: e s ls cat del        lang: bash, python3, node, fish, /bin/zsh, ...
";

/// The full help text, ending with the settings you'd get right now and where they come from.
pub fn help() -> String {
    format!("{HELP}{}", config::describe())
}

#[derive(Debug, PartialEq, Eq)]
pub enum Cmd {
    Edit { name: String, lang: Option<String> },
    Save { name: String },
    List,
    Show { name: String },
    Rm { names: Vec<String>, force: bool },
    Help,
    Version,
}

pub fn parse(args: &[String]) -> Result<Cmd> {
    let Some((verb, rest)) = args.split_first() else {
        return Err(Error::Usage(help()));
    };
    let rest: Vec<&str> = rest.iter().map(String::as_str).collect();
    if rest.iter().any(|a| matches!(*a, "-h" | "--help")) {
        return Ok(Cmd::Help); // `fun edit -h` works like everywhere else
    }
    match verb.as_str() {
        "edit" | "e" => match rest.as_slice() {
            [name] => Ok(Cmd::Edit {
                name: name.to_string(),
                lang: None,
            }),
            [name, lang] => Ok(Cmd::Edit {
                name: name.to_string(),
                lang: Some(lang.to_string()),
            }),
            _ => usage("edit <name> [lang]"),
        },
        "save" | "s" => match rest.as_slice() {
            [name] => Ok(Cmd::Save {
                name: name.to_string(),
            }),
            _ => usage("save <name>"),
        },
        "list" | "ls" => match rest.as_slice() {
            [] => Ok(Cmd::List),
            _ => usage("list"),
        },
        "show" | "cat" => match rest.as_slice() {
            [name] => Ok(Cmd::Show {
                name: name.to_string(),
            }),
            _ => usage("show <name>"),
        },
        "rm" | "del" => parse_rm(&rest),
        "help" | "-h" | "--help" => Ok(Cmd::Help),
        "version" | "-V" | "--version" => Ok(Cmd::Version),
        other => Err(Error::Usage(format!(
            "unknown command '{other}'. Try 'fun help'."
        ))),
    }
}

fn parse_rm(args: &[&str]) -> Result<Cmd> {
    let (mut force, mut names, mut options_done) = (false, Vec::new(), false);
    for &arg in args {
        match arg {
            "--" if !options_done => options_done = true,
            "-f" | "--force" if !options_done => force = true,
            _ if arg.starts_with('-') && arg.len() > 1 && !options_done => {
                return Err(Error::Usage(format!(
                    "rm: unknown option '{arg}'\nusage: fun rm [-f] <name>..."
                )));
            }
            _ => names.push(arg.to_string()),
        }
    }
    if names.is_empty() {
        return usage("rm [-f] <name>...");
    }
    Ok(Cmd::Rm { names, force })
}

fn usage<T>(synopsis: &str) -> Result<T> {
    Err(Error::Usage(format!("usage: fun {synopsis}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(args: &[&str]) -> Result<Cmd> {
        parse(&args.iter().map(|s| s.to_string()).collect::<Vec<_>>())
    }

    #[test]
    fn verbs_and_aliases() {
        assert_eq!(
            p(&["e", "x"]).unwrap(),
            Cmd::Edit {
                name: "x".into(),
                lang: None
            }
        );
        assert_eq!(
            p(&["edit", "x", "python3"]).unwrap(),
            Cmd::Edit {
                name: "x".into(),
                lang: Some("python3".into())
            }
        );
        assert_eq!(p(&["s", "x"]).unwrap(), Cmd::Save { name: "x".into() });
        assert_eq!(p(&["ls"]).unwrap(), Cmd::List);
        assert_eq!(p(&["cat", "x"]).unwrap(), Cmd::Show { name: "x".into() });
        assert_eq!(p(&["--version"]).unwrap(), Cmd::Version);
    }

    #[test]
    fn help_anywhere() {
        for args in [
            &["help"][..],
            &["-h"],
            &["edit", "-h"],
            &["rm", "--help"],
            &["save", "x", "--help"],
        ] {
            assert_eq!(p(args).unwrap(), Cmd::Help, "{args:?}");
        }
    }

    #[test]
    fn rm_flags() {
        assert_eq!(
            p(&["rm", "-f", "a", "b"]).unwrap(),
            Cmd::Rm {
                names: vec!["a".into(), "b".into()],
                force: true
            }
        );
        assert_eq!(
            p(&["del", "--", "-f"]).unwrap(),
            Cmd::Rm {
                names: vec!["-f".into()],
                force: false
            }
        );
        assert!(p(&["rm"]).is_err());
        assert!(p(&["rm", "-x", "a"]).is_err());
    }

    #[test]
    fn usage_errors_exit_2() {
        for args in [
            &[][..],
            &["edit"],
            &["edit", "a", "b", "c"],
            &["save", "a", "b"],
            &["list", "x"],
            &["bogus"],
        ] {
            assert_eq!(p(args).unwrap_err().code(), 2, "{args:?}");
        }
    }
}
