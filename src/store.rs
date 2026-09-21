//! Where things live, and the rules about who owns what.
//!
//! ```text
//! $FUN_DIR/scripts/<name>        saved scripts (executable)
//! $FUN_DIR/drafts/<name>         in-progress edits
//! $FUN_DIR/meta/<name>.created   when the script was first saved (unix seconds)
//! $FUN_DIR/meta/<name>.runs      one byte per run: the file's size is the run count
//! $FUN_BIN/<name>                symlink -> the `fun` binary, which counts the run and execs the script
//! ```
//! Drafts sit next to the scripts so that saving is a single atomic `rename`. Run counts are an
//! append-only file so concurrent runs can't lose updates: a one-byte `O_APPEND` write is atomic.

use std::env;
use std::fmt;
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::os::unix::fs::{symlink, PermissionsExt};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::error::{Context, Error, Result};
use crate::ui::tilde;

/// The name of this program. Links that point at a file with this name are ours.
pub const CLI: &str = "fun";
const MAX_NAME: usize = 64;

/// Shell builtins/keywords that aren't also binaries (those are caught by the $PATH scan).
const RESERVED: &[&str] = &[
    "fun",
    "cd",
    "set",
    "source",
    "alias",
    "abbr",
    "function",
    "functions",
    "builtin",
    "command",
    "exec",
    "exit",
    "export",
    "eval",
    "read",
    "type",
    "unset",
    "if",
    "for",
    "while",
    "end",
    "switch",
    "case",
    "return",
    "break",
    "continue",
];

/// A validated script name: `[A-Za-z0-9_-]{1,64}`, not starting with `-`.
/// Being a filename *and* a command, it can't contain anything that needs escaping anywhere.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Name(String);

impl Name {
    pub fn parse(s: &str) -> Result<Name> {
        if s.is_empty() {
            return Err(Error::fail(
                "no name given. I'm a script manager, not a psychic hotline.",
            ));
        }
        if s.len() > MAX_NAME {
            return Err(Error::fail(format!(
                "that name is absurdly long. {MAX_NAME} chars, tops."
            )));
        }
        let ok = !s.starts_with('-')
            && s.bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-');
        if !ok {
            return Err(Error::fail(format!(
                "'{s}': letters, digits, '_' and '-' only, and no leading '-'. \
                 It's a filename and a command; keep it boring."
            )));
        }
        Ok(Name(s.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Name {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// What's at `$FUN_BIN/<name>`.
#[derive(Debug, PartialEq, Eq)]
pub enum Link {
    Absent,
    /// A symlink to the `fun` binary: runs are counted. What we create.
    Launcher,
    /// A symlink straight to the script (how fun 1.0 did it). Still ours; upgraded on the next save.
    Legacy,
    /// A real file, or a symlink to somewhere else. Hands off.
    Foreign,
}

impl Link {
    pub fn is_ours(&self) -> bool {
        matches!(self, Link::Launcher | Link::Legacy)
    }
}

pub struct Store {
    scripts: PathBuf,
    drafts: PathBuf,
    meta: PathBuf,
    bin: PathBuf,
}

impl Store {
    pub fn from_env() -> Result<Store> {
        let var = |k: &str| env::var_os(k).filter(|v| !v.is_empty()).map(PathBuf::from);
        let home =
            || var("HOME").ok_or_else(|| Error::fail("$HOME isn't set; set it, or set FUN_DIR and FUN_BIN"));
        let root = match (var("FUN_DIR"), var("XDG_DATA_HOME")) {
            (Some(dir), _) => dir,
            (None, Some(xdg)) => xdg.join("fun"),
            (None, None) => home()?.join(".local/share/fun"),
        };
        let bin = match var("FUN_BIN") {
            Some(bin) => bin,
            None => home()?.join(".local/bin"),
        };
        Ok(Store::new(absolute(root)?, absolute(bin)?))
    }

    pub fn new(root: PathBuf, bin: PathBuf) -> Store {
        Store {
            scripts: root.join("scripts"),
            drafts: root.join("drafts"),
            meta: root.join("meta"),
            bin,
        }
    }

    pub fn script(&self, n: &Name) -> PathBuf {
        self.scripts.join(n.as_str())
    }

    pub fn draft(&self, n: &Name) -> PathBuf {
        self.drafts.join(n.as_str())
    }

    pub fn link(&self, n: &Name) -> PathBuf {
        self.bin.join(n.as_str())
    }

    pub fn bin_dir(&self) -> &Path {
        &self.bin
    }

    fn created_file(&self, n: &Name) -> PathBuf {
        self.meta.join(format!("{n}.created"))
    }

    fn runs_file(&self, n: &Name) -> PathBuf {
        self.meta.join(format!("{n}.runs"))
    }

    pub fn saved_names(&self) -> Result<Vec<Name>> {
        names_in(&self.scripts)
    }

    pub fn draft_names(&self) -> Result<Vec<Name>> {
        names_in(&self.drafts)
    }

    pub fn ensure_data_dirs(&self) -> Result<()> {
        for dir in [&self.scripts, &self.drafts] {
            fs::create_dir_all(dir).context(|| format!("can't create {}", tilde(dir)))?;
        }
        Ok(())
    }

    pub fn bin_on_path(&self) -> bool {
        env::var_os("PATH").is_some_and(|p| env::split_paths(&p).any(|d| d == self.bin))
    }

    // -- the symlink contract ------------------------------------------------

    pub fn link_state(&self, n: &Name) -> Link {
        match fs::read_link(self.link(n)) {
            Ok(target) if target.file_name().is_some_and(|f| f == CLI) => Link::Launcher,
            Ok(target) if target == self.script(n) => Link::Legacy,
            Ok(_) => Link::Foreign,
            Err(e) if e.kind() == io::ErrorKind::NotFound => Link::Absent,
            Err(_) => Link::Foreign, // exists but isn't a symlink
        }
    }

    /// Refuse names that would clobber or be confused with something else, and say what to use instead.
    /// Names we already own were vetted when first saved, so they skip the $PATH scan.
    pub fn ensure_name_is_free(&self, n: &Name) -> Result<()> {
        if RESERVED.contains(&n.as_str()) {
            return Err(Error::fail(format!(
                "'{n}' is a shell builtin/keyword. Shadow it and future-you files a bug against past-you at 3am.{}",
                self.alternatives(n)
            )));
        }
        if let Some(other) = self.case_clash(n) {
            return Err(Error::fail(format!(
                "'{n}' differs from '{other}' only by case. Two names that look alike will bite you \
                 (and are one file on case-insensitive disks). Use '{other}'."
            )));
        }
        match self.link_state(n) {
            Link::Launcher | Link::Legacy => return Ok(()),
            Link::Foreign => {
                return Err(Error::fail(format!(
                    "{} exists and isn't ours. Refusing to clobber it.{}",
                    tilde(&self.link(n)),
                    self.alternatives(n)
                )))
            }
            Link::Absent => {}
        }
        if let Some(existing) = command_on_path(n.as_str()) {
            return Err(Error::fail(format!(
                "'{n}' is already a command ({}). Saving would shadow it, or be shadowed by it.{}",
                tilde(&existing),
                self.alternatives(n)
            )));
        }
        Ok(())
    }

    /// An existing script or draft whose name differs from `n` only in case.
    fn case_clash(&self, n: &Name) -> Option<Name> {
        let lower = n.as_str().to_ascii_lowercase();
        let (saved, drafts) = (self.saved_names().ok()?, self.draft_names().ok()?);
        saved
            .into_iter()
            .chain(drafts)
            .find(|o| o != n && o.as_str().to_ascii_lowercase() == lower)
    }

    /// Would `candidate` pass `ensure_name_is_free` and not exist yet?
    fn is_free(&self, candidate: &str) -> bool {
        let Ok(n) = Name::parse(candidate) else {
            return false;
        };
        !RESERVED.contains(&candidate)
            && self.case_clash(&n).is_none()
            && !self.script(&n).exists()
            && !self.draft(&n).exists()
            && self.link_state(&n) == Link::Absent
            && command_on_path(candidate).is_none()
    }

    /// ` Try 'my-ls' or 'ls2'.`: the first free names of a few obvious shapes, or nothing.
    fn alternatives(&self, n: &Name) -> String {
        let base = n.as_str();
        let candidates = [
            format!("my-{base}"),
            format!("{base}2"),
            format!("{base}3"),
            format!("{base}4"),
        ];
        let free: Vec<String> = candidates
            .into_iter()
            .filter(|c| self.is_free(c))
            .take(2)
            .map(|c| format!("'{c}'"))
            .collect();
        if free.is_empty() {
            String::new()
        } else {
            format!(" Try {}.", free.join(" or "))
        }
    }

    /// Point `$FUN_BIN/<name>` at the `fun` binary, atomically: there's never an instant without a link.
    pub fn install_link(&self, n: &Name) -> Result<()> {
        let target = launcher_target()?;
        fs::create_dir_all(&self.bin).context(|| format!("can't create {}", tilde(&self.bin)))?;
        let tmp = self.bin.join(format!(".{n}.{}.tmp", std::process::id()));
        let _ = fs::remove_file(&tmp); // leftover from a crashed run with a recycled pid
        symlink(target, &tmp).context(|| format!("can't create {}", tilde(&tmp)))?;
        if let Err(e) = fs::rename(&tmp, self.link(n)) {
            let _ = fs::remove_file(&tmp);
            return Err(e).context(|| format!("can't link {}", tilde(&self.link(n))));
        }
        Ok(())
    }

    // -- metadata: creation date and run counts --------------------------------

    /// Count one run. Best effort: a script must never fail to start because bookkeeping did.
    pub fn record_run(&self, n: &Name) {
        let path = self.runs_file(n);
        let open = || OpenOptions::new().append(true).create(true).open(&path);
        let file = open().or_else(|_| {
            fs::create_dir_all(&self.meta)?;
            open()
        });
        if let Ok(mut f) = file {
            let _ = f.write_all(b".");
        }
    }

    pub fn run_count(&self, n: &Name) -> u64 {
        fs::metadata(self.runs_file(n)).map_or(0, |m| m.len())
    }

    /// When the script was first saved. Scripts that predate this bookkeeping fall back to their mtime.
    pub fn created_at(&self, n: &Name) -> Option<u64> {
        let recorded = fs::read_to_string(self.created_file(n))
            .ok()
            .and_then(|s| s.trim().parse().ok());
        recorded.or_else(|| secs(fs::metadata(self.script(n)).ok()?.modified().ok()))
    }

    /// Remember the creation date, once. `predating` is the age of an already-saved script that has
    /// no record yet, so replacing it doesn't reset its birthday. Best effort.
    pub fn record_created(&self, n: &Name, predating: Option<u64>) {
        let when = predating.or_else(|| secs(Some(SystemTime::now()))).unwrap_or(0);
        let _ = fs::create_dir_all(&self.meta);
        if let Ok(mut f) = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(self.created_file(n))
        {
            let _ = writeln!(f, "{when}");
        }
    }

    /// The birth (or, failing that, modification) time of a file, for `record_created`.
    pub fn age_of(path: &Path) -> Option<u64> {
        let meta = fs::metadata(path).ok()?;
        secs(meta.created().or_else(|_| meta.modified()).ok())
    }

    pub fn forget(&self, n: &Name) -> Result<()> {
        remove_if_exists(&self.runs_file(n))?;
        remove_if_exists(&self.created_file(n))
    }
}

fn secs(t: Option<SystemTime>) -> Option<u64> {
    t?.duration_since(UNIX_EPOCH).ok().map(|d| d.as_secs())
}

/// The first line of a file, as raw bytes (at most 1 KiB of it).
pub fn first_line(path: &Path) -> io::Result<Vec<u8>> {
    use std::io::{BufRead, BufReader, Read};
    let mut line = Vec::new();
    BufReader::new(fs::File::open(path)?.take(1024)).read_until(b'\n', &mut line)?;
    Ok(line)
}

/// `chmod +x`, without ignoring the umask: every read bit grows an exec bit.
pub fn make_executable(path: &Path) -> Result<()> {
    let ctx = || format!("can't chmod {}", tilde(path));
    let mode = fs::metadata(path).context(ctx)?.permissions().mode();
    let mode = (mode & 0o777) | ((mode & 0o444) >> 2);
    fs::set_permissions(path, fs::Permissions::from_mode(mode)).context(ctx)
}

pub fn remove_if_exists(path: &Path) -> Result<()> {
    match fs::remove_file(path) {
        Err(e) if e.kind() != io::ErrorKind::NotFound => {
            Err(e).context(|| format!("can't remove {}", tilde(path)))
        }
        _ => Ok(()),
    }
}

fn names_in(dir: &Path) -> Result<Vec<Name>> {
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e).context(|| format!("can't read {}", tilde(dir))),
    };
    let mut names: Vec<Name> = entries
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_ok_and(|t| t.is_file()))
        .filter_map(|e| Name::parse(e.file_name().to_str()?).ok())
        .collect();
    names.sort();
    Ok(names)
}

fn is_executable(path: &Path) -> bool {
    fs::metadata(path).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
}

fn command_on_path(name: &str) -> Option<PathBuf> {
    let path = env::var_os("PATH")?;
    env::split_paths(&path)
        .map(|d| d.join(name))
        .find(|p| is_executable(p))
}

/// Would the shell find `prog`? A bare name is looked up on `$PATH`; anything with a `/` is a path.
pub fn is_command(prog: &str) -> bool {
    if prog.contains('/') {
        is_executable(Path::new(prog))
    } else {
        command_on_path(prog).is_some()
    }
}

/// Where a script's symlink should point: this binary, by the most stable path we can find.
/// The path you type (`/opt/homebrew/bin/fun`) beats the one it resolves to (a versioned directory
/// that the next upgrade deletes), provided both are the same file.
fn launcher_target() -> Result<PathBuf> {
    let exe = env::current_exe().context(|| "can't locate the fun binary".into())?;
    if exe.file_name().map_or(true, |f| f != CLI) {
        return Err(Error::fail(format!(
            "this binary is called {}, but it must be called '{CLI}' so that your scripts can find it",
            tilde(&exe)
        )));
    }
    if let Some(found) = command_on_path(CLI) {
        if let (Ok(a), Ok(b)) = (fs::canonicalize(&found), fs::canonicalize(&exe)) {
            if a == b {
                return absolute(found);
            }
        }
    }
    Ok(exe)
}

/// Like `std::path::absolute` (1.79+), minus the normalisation: we compare link targets exactly.
fn absolute(p: PathBuf) -> Result<PathBuf> {
    if p.is_absolute() {
        return Ok(p);
    }
    let cwd = env::current_dir().context(|| "can't determine the current directory".into())?;
    Ok(cwd.join(p))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_names() {
        for s in [
            "a",
            "hello",
            "my-script",
            "_x",
            "Foo_bar9",
            "9lives",
            &"a".repeat(64),
        ] {
            assert!(Name::parse(s).is_ok(), "{s}");
        }
    }

    #[test]
    fn invalid_names() {
        for s in [
            "",
            "-rf",
            "a b",
            "a/b",
            "..",
            ".hidden",
            "a.b",
            "é",
            "x\n",
            &"a".repeat(65),
        ] {
            assert!(Name::parse(s).is_err(), "{s:?}");
        }
    }
}
