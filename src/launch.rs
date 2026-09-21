//! Running a script. `$FUN_BIN/<name>` is a symlink to this very binary, so starting `<name>` starts
//! `fun` with `argv[0] = "<name>"`. We count the run, then `exec` the script: fun leaves no process
//! behind, and the script sees your arguments, stdin, stdout, exit code, and signals untouched.
//! (This is how rustup's `cargo` and busybox's `ls` work.)

use std::ffi::{OsStr, OsString};
use std::io::ErrorKind;
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Command, ExitCode};

use crate::lang;
use crate::store::{first_line, Name, Store, CLI};

/// Were we started under a script's name? Only if a script by that name exists; otherwise we're just
/// `fun` reached through some other name, and the CLI should answer. Costs nothing for plain `fun`.
pub fn detect(argv0: &OsStr) -> Option<(Store, Name)> {
    let base = Path::new(argv0).file_name()?.to_str()?;
    if base == CLI {
        return None;
    }
    let name = Name::parse(base).ok()?;
    let store = Store::from_env().ok()?;
    store.script(&name).is_file().then_some((store, name))
}

/// Count the run and become the script. Only returns if that failed.
pub fn run(store: &Store, name: &Name, args: Vec<OsString>) -> ExitCode {
    store.record_run(name);
    let script = store.script(name);
    let err = Command::new(&script).args(args).exec();

    // We only get here if the kernel refused to start the script. For a shebang that names the
    // interpreter directly, its verdict on a missing one is "No such file or directory", which is a
    // terrible way to find out that something isn't installed. We know better.
    let missing = first_line(&script)
        .ok()
        .and_then(|line| lang::missing_interpreter(&String::from_utf8_lossy(&line)));
    match missing {
        Some(interp) => eprintln!("fun: {name} needs '{interp}', which isn't installed"),
        None => eprintln!("fun: can't run {name}: {err}"),
    }
    ExitCode::from(if err.kind() == ErrorKind::PermissionDenied {
        126
    } else {
        127
    })
}
