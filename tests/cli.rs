//! End-to-end: every test drives the real binary against its own throwaway directory.

use std::ffi::OsStr;
use std::fs;
use std::io::{Read, Write};
use std::os::unix::fs::{symlink, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};

static COUNTER: AtomicUsize = AtomicUsize::new(0);
const FUN: &str = env!("CARGO_BIN_EXE_fun");

struct Sandbox {
    root: PathBuf,
    editor: Option<PathBuf>,
}

struct Out {
    code: i32,
    stdout: String,
    stderr: String,
}

const HELLO: &str = "#!/bin/sh\necho \"hello, $*\"\n";
const BASH_TEMPLATE: &str = "#!/usr/bin/env bash\nset -euo pipefail\n\n";

impl Sandbox {
    fn new() -> Sandbox {
        let root = std::env::temp_dir().join(format!(
            "fun-test-{}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::SeqCst)
        ));
        fs::create_dir_all(root.join("tools")).unwrap();
        fs::write(root.join("config.toml"), "").unwrap(); // FUN_CONFIG is explicit, so it must exist
        Sandbox { root, editor: None }
    }

    fn dir(&self) -> PathBuf {
        self.root.join("fun")
    }
    fn bin(&self) -> PathBuf {
        self.root.join("bin")
    }
    fn config(&self) -> PathBuf {
        self.root.join("config.toml")
    }
    fn script(&self, name: &str) -> PathBuf {
        self.dir().join("scripts").join(name)
    }
    fn draft(&self, name: &str) -> PathBuf {
        self.dir().join("drafts").join(name)
    }
    fn meta(&self, file: &str) -> PathBuf {
        self.dir().join("meta").join(file)
    }

    fn write_config(&self, text: &str) {
        fs::write(self.config(), text).unwrap();
    }

    /// Turn autosave off: the classic funced/funcsave two-step, where drafts wait for `fun save`.
    fn two_step(&self) {
        self.write_config("autosave = false\n");
    }

    /// `program` with this sandbox's environment: its own home, config, and `$PATH`.
    fn command_for(&self, program: impl AsRef<OsStr>) -> Command {
        let mut c = Command::new(program);
        let path = format!(
            "{}:{}:{}",
            self.root.join("tools").display(),
            self.bin().display(),
            std::env::var("PATH").unwrap()
        );
        c.env_clear()
            .env("PATH", path)
            .env("HOME", &self.root)
            .env("FUN_DIR", self.dir())
            .env("FUN_BIN", self.bin())
            .env("FUN_CONFIG", self.config());
        if let Some(e) = &self.editor {
            c.env("EDITOR", e);
        }
        c
    }

    fn command(&self) -> Command {
        self.command_for(FUN)
    }

    fn fun(&self, args: &[&str]) -> Out {
        self.fun_in(args, "")
    }

    fn fun_in(&self, args: &[&str], stdin: &str) -> Out {
        let mut cmd = self.command();
        cmd.args(args);
        run(&mut cmd, stdin)
    }

    /// Run an installed script by its name, the way a shell would.
    fn run_script(&self, name: &str, args: &[&str]) -> Out {
        let mut cmd = self.command_for(self.bin().join(name));
        cmd.args(args);
        run(&mut cmd, "")
    }

    /// `fun list`, parsed: one row of cells per script (stdout isn't a terminal, so it's tab-separated).
    fn table(&self) -> Vec<Vec<String>> {
        let out = self.fun(&["list"]);
        assert_eq!(out.code, 0, "{}", out.stderr);
        out.stdout
            .lines()
            .map(|l| l.split('\t').map(String::from).collect())
            .collect()
    }

    /// A fake `$EDITOR` that overwrites the draft (its last argument) with `body`; `None` leaves it alone.
    fn set_editor(&mut self, body: Option<&str>, exit: i32) {
        let path = self.root.join("fake-editor");
        write_editor(&path, body, exit, None);
        self.editor = Some(path);
    }

    /// Put a fake tool called `name` on `$PATH` that records its arguments, one per line, into `args-<name>.txt`.
    fn recording_tool(&self, name: &str) {
        write_editor(
            &self.root.join("tools").join(name),
            None,
            0,
            Some(self.args_file(name)),
        );
    }

    fn args_file(&self, tool: &str) -> PathBuf {
        self.root.join(format!("args-{tool}.txt"))
    }

    /// The arguments `tool` was last called with, or `None` if it never ran.
    fn recorded_args(&self, tool: &str) -> Option<Vec<String>> {
        fs::read_to_string(self.args_file(tool))
            .ok()
            .map(|t| t.lines().map(String::from).collect())
    }

    /// Get `name` saved with content `body` (autosave does it when the editor closes).
    fn saved(&mut self, name: &str, body: &str) {
        self.set_editor(Some(body), 0);
        let r = self.fun(&["edit", name]);
        assert_eq!(r.code, 0, "{}", r.stderr);
        assert!(self.script(name).exists());
    }
}

fn write_editor(path: &Path, body: Option<&str>, exit: i32, record: Option<PathBuf>) {
    let write = body
        .map(|b| format!("cat > \"$last\" <<'FUN_EOF'\n{b}FUN_EOF\n"))
        .unwrap_or_default();
    let record = record
        .map(|r| format!("printf '%s\\n' \"$@\" > '{}'\n", r.display()))
        .unwrap_or_default();
    fs::write(
        path,
        format!("#!/bin/sh\nfor last; do :; done\n{record}{write}exit {exit}\n"),
    )
    .unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
}

fn run(cmd: &mut Command, stdin: &str) -> Out {
    let mut child = cmd
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(stdin.as_bytes()).unwrap();
    let out = child.wait_with_output().unwrap();
    Out {
        code: out.status.code().unwrap_or(-1),
        stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
    }
}

impl Drop for Sandbox {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn read(p: &Path) -> String {
    fs::read_to_string(p).unwrap()
}

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

// -- the headline: edit, close, run ---------------------------------------------------

#[test]
fn closing_the_editor_saves_and_the_script_runs_at_once() {
    // fun edit hello  ->  (write something, quit)  ->  hello
    let mut sb = Sandbox::new();
    sb.set_editor(Some("#!/bin/sh\necho \"hello, $*\"\n"), 0);
    let r = sb.fun(&["edit", "hello"]);
    assert_eq!(r.code, 0, "{}", r.stderr);
    assert!(r.stderr.contains("new script hello (bash)"));
    assert!(r.stdout.starts_with("saved hello -> "), "{}", r.stdout);
    assert!(!sb.draft("hello").exists(), "the draft was promoted, not copied");

    let out = sb.run_script("hello", &["world"]);
    assert_eq!((out.code, out.stdout.as_str()), (0, "hello, world\n"));
}

#[test]
fn a_new_script_opens_the_editor_on_bash_and_safe_defaults() {
    let mut sb = Sandbox::new();
    sb.set_editor(
        Some(&format!(
            "{BASH_TEMPLATE}echo \"bash $BASH_VERSION\" | cut -c1-4\n"
        )),
        0,
    );
    assert_eq!(sb.fun(&["edit", "b"]).code, 0);
    assert_eq!(sb.run_script("b", &[]).stdout, "bash\n");
}

// -- edit: defaults, languages, config -----------------------------------------------

#[test]
fn out_of_the_box_you_get_vim_and_bash() {
    let sb = Sandbox::new();
    sb.recording_tool("vim"); // no config, no $EDITOR/$VISUAL: the default editor is `vim`
    let r = sb.fun(&["edit", "foo"]);
    assert_eq!(r.code, 0, "{}", r.stderr);
    assert!(r.stderr.contains("new script foo (bash)"));
    // ...and the cursor lands on the blank line where you start typing.
    assert_eq!(
        sb.recorded_args("vim").unwrap(),
        ["+3".to_string(), sb.draft("foo").display().to_string()]
    );
}

#[test]
fn plus_line_is_only_sent_to_editors_that_understand_it() {
    let mut sb = Sandbox::new();
    let ed = sb.root.join("my-editor"); // not a vi-alike, so it just gets the file
    write_editor(&ed, None, 0, Some(sb.args_file("my-editor")));
    sb.editor = Some(ed);
    assert_eq!(sb.fun(&["edit", "foo"]).code, 0);
    assert_eq!(
        sb.recorded_args("my-editor").unwrap(),
        [sb.draft("foo").display().to_string()]
    );
}

#[test]
fn plus_line_is_not_sent_when_resuming() {
    let sb = Sandbox::new();
    sb.two_step(); // so the first draft is still there to resume
    sb.recording_tool("vim");
    sb.fun(&["edit", "foo"]);
    sb.fun(&["edit", "foo"]); // resumes; opens at the top like any file
    assert_eq!(
        sb.recorded_args("vim").unwrap(),
        [sb.draft("foo").display().to_string()]
    );
}

#[test]
fn language_argument_beats_config_beats_default() {
    let mut sb = Sandbox::new();
    sb.set_editor(None, 0);
    sb.two_step(); // keep the drafts around so we can read the templates
    assert_eq!(sb.fun(&["edit", "a"]).code, 0);
    assert_eq!(read(&sb.draft("a")), BASH_TEMPLATE);

    sb.write_config("autosave = false\nlanguage = \"fish\"\n");
    sb.fun(&["edit", "b"]);
    assert_eq!(read(&sb.draft("b")), "#!/usr/bin/env fish\n\n");

    sb.fun(&["edit", "c", "python3"]);
    assert_eq!(read(&sb.draft("c")), "#!/usr/bin/env python3\n\n");
    sb.fun(&["edit", "d", "/bin/zsh"]);
    assert_eq!(read(&sb.draft("d")), "#!/bin/zsh\nset -euo pipefail\n\n");
    sb.fun(&["edit", "e", "py"]);
    assert_eq!(read(&sb.draft("e")), "#!/usr/bin/env python3\n\n");
    sb.fun(&["edit", "f", "sh"]);
    assert_eq!(read(&sb.draft("f")), "#!/usr/bin/env sh\nset -eu\n\n");
}

#[test]
fn a_bad_language_is_rejected_before_anything_is_created() {
    let mut sb = Sandbox::new();
    sb.set_editor(None, 0);
    let r = sb.fun(&["edit", "foo", "two words"]);
    assert_eq!(r.code, 1);
    assert!(r.stderr.contains("isn't a language"));
    assert!(!sb.draft("foo").exists());
}

#[test]
fn editor_precedence_is_config_then_visual_then_editor_then_vim() {
    let sb = Sandbox::new();
    let tools = ["from-config", "from-visual", "from-editor", "vim"];
    for tool in tools {
        sb.recording_tool(tool);
    }
    // Which tool ran, given what's set?
    let who_ran = |visual: bool, editor: bool| -> Vec<&str> {
        let mut cmd = sb.command();
        if visual {
            cmd.env("VISUAL", "from-visual");
        }
        if editor {
            cmd.env("EDITOR", "from-editor");
        }
        for tool in tools {
            let _ = fs::remove_file(sb.args_file(tool));
        }
        let _ = fs::remove_file(sb.draft("x"));
        assert_eq!(run(cmd.args(["edit", "x"]), "").code, 0);
        tools
            .into_iter()
            .filter(|t| sb.recorded_args(t).is_some())
            .collect()
    };
    assert_eq!(who_ran(false, false), ["vim"]);
    assert_eq!(who_ran(false, true), ["from-editor"]);
    assert_eq!(who_ran(true, true), ["from-visual"]);
    sb.write_config("editor = \"from-config\"\n");
    assert_eq!(who_ran(true, true), ["from-config"]);
}

#[test]
fn config_file_editor_wins_over_the_environment() {
    let sb = Sandbox::new();
    sb.recording_tool("from-config");
    sb.write_config("editor = from-config\n");
    let mut cmd = sb.command();
    cmd.env("EDITOR", "/nonexistent/would-fail")
        .env("VISUAL", "/nonexistent/would-fail");
    let r = run(cmd.args(["edit", "x"]), "");
    assert_eq!(r.code, 0, "{}", r.stderr);
    assert!(sb.recorded_args("from-config").is_some());
}

#[test]
fn visual_wins_over_editor() {
    let mut sb = Sandbox::new();
    sb.set_editor(Some(HELLO), 0);
    let ed = sb.editor.clone().unwrap();
    let mut cmd = sb.command();
    cmd.env("VISUAL", &ed).env("EDITOR", "/nonexistent");
    assert_eq!(run(cmd.args(["edit", "foo"]), "").code, 0);
    assert_eq!(read(&sb.script("foo")), HELLO);
}

#[test]
fn broken_config_says_where_and_leaves_no_litter() {
    let sb = Sandbox::new();
    sb.write_config("# my settings\nlanguage = \"bash\"\nlang = \"fish\"\n");
    let r = sb.fun(&["edit", "foo"]);
    assert_eq!(r.code, 1);
    assert!(
        r.stderr.contains("config.toml:3: unknown key `lang`"),
        "{}",
        r.stderr
    );
    assert!(!sb.dir().exists());
    // ...but commands that don't need the config keep working.
    assert_eq!(sb.fun(&["ls"]).code, 0);

    sb.write_config("autosave = maybe\n");
    let r = sb.fun(&["edit", "foo"]);
    assert!(
        r.stderr
            .contains("config.toml:1: `autosave` must be true or false"),
        "{}",
        r.stderr
    );
}

#[test]
fn an_explicit_config_path_must_exist_but_the_default_may_not() {
    let sb = Sandbox::new();
    fs::remove_file(sb.config()).unwrap(); // FUN_CONFIG now names a file that doesn't exist
    sb.recording_tool("vim");
    let r = sb.fun(&["edit", "foo"]);
    assert_eq!(r.code, 1);
    assert!(r.stderr.contains("can't read"));

    let mut cmd = sb.command();
    cmd.env_remove("FUN_CONFIG");
    assert_eq!(run(cmd.args(["edit", "foo"]), "").code, 0); // no file at ~/.config/fun/config.toml: fine
}

#[test]
fn config_lives_under_xdg_config_home_when_set() {
    let sb = Sandbox::new();
    let xdg = sb.root.join("xdg");
    fs::create_dir_all(xdg.join("fun")).unwrap();
    fs::write(
        xdg.join("fun/config.toml"),
        "language = \"lua\"\neditor = \"true\"\nautosave = false\n",
    )
    .unwrap();
    let mut cmd = sb.command();
    cmd.env_remove("FUN_CONFIG").env("XDG_CONFIG_HOME", &xdg);
    // editor `true` exits 0 without touching the draft
    assert_eq!(run(cmd.args(["edit", "foo"]), "").code, 0);
    assert_eq!(read(&sb.draft("foo")), "#!/usr/bin/env lua\n\n");
}

#[test]
fn a_missing_editor_says_how_to_fix_it() {
    let sb = Sandbox::new();
    sb.write_config("editor = \"no-such-editor-xyz\"\n");
    let r = sb.fun(&["edit", "foo"]);
    assert_eq!(r.code, 1);
    assert!(r.stderr.contains("couldn't run the editor 'no-such-editor-xyz'"));
    assert!(r.stderr.contains("config"));
}

// -- autosave --------------------------------------------------------------------------

#[test]
fn quitting_without_writing_saves_nothing() {
    let mut sb = Sandbox::new();
    sb.set_editor(None, 0); // opens the template and closes it again
    let r = sb.fun(&["edit", "oops"]);
    assert_eq!(r.code, 0);
    assert!(r.stderr.contains("nothing written, so 'oops' wasn't saved"));
    assert!(!sb.script("oops").exists() && !sb.draft("oops").exists());
    assert!(fs::symlink_metadata(sb.bin().join("oops")).is_err());
}

#[test]
fn editors_that_tidy_trailing_blank_lines_still_count_as_untouched() {
    let mut sb = Sandbox::new();
    sb.set_editor(Some("#!/usr/bin/env bash\nset -euo pipefail\n"), 0); // the template, minus its last blank line
    let r = sb.fun(&["edit", "oops"]);
    assert!(r.stderr.contains("nothing written"), "{}", r.stderr);
    assert!(!sb.script("oops").exists());
}

#[test]
fn editing_a_saved_script_and_leaving_it_alone_changes_nothing() {
    let mut sb = Sandbox::new();
    sb.saved("foo", HELLO);
    let before = fs::metadata(sb.script("foo")).unwrap().modified().unwrap();
    sb.set_editor(None, 0);
    let r = sb.fun(&["edit", "foo", "node"]);
    assert_eq!(r.code, 0);
    assert!(r.stderr.contains("foo exists: editing the saved script"));
    assert!(r.stderr.contains("ignoring 'node'"));
    assert!(r.stderr.contains("no changes to 'foo'"));
    assert!(!sb.draft("foo").exists());
    assert_eq!(
        fs::metadata(sb.script("foo")).unwrap().modified().unwrap(),
        before
    );
}

#[test]
fn editing_a_saved_script_replaces_it_and_keeps_its_birthday_and_run_count() {
    let mut sb = Sandbox::new();
    sb.saved("foo", HELLO);
    fs::write(sb.meta("foo.created"), "1000000000\n").unwrap();
    sb.run_script("foo", &[]);
    sb.run_script("foo", &[]);

    sb.set_editor(Some("#!/bin/sh\necho v2\n"), 0);
    let r = sb.fun(&["edit", "foo"]);
    assert_eq!(r.code, 0, "{}", r.stderr);
    assert!(r.stdout.starts_with("saved foo -> "));
    assert_eq!(sb.run_script("foo", &[]).stdout, "v2\n");
    assert_eq!(read(&sb.meta("foo.created")), "1000000000\n");
    assert_eq!(sb.table()[0][3], "3"); // two before, one just now
}

#[test]
fn an_invalid_draft_is_kept_not_installed_and_fixed_by_editing_again() {
    let mut sb = Sandbox::new();
    sb.set_editor(Some("echo no shebang\n"), 0);
    let r = sb.fun(&["edit", "foo"]);
    assert_eq!(r.code, 1);
    assert!(r.stderr.contains("shebang"));
    assert_eq!(read(&sb.draft("foo")), "echo no shebang\n");
    assert!(!sb.script("foo").exists() && fs::symlink_metadata(sb.bin().join("foo")).is_err());

    sb.set_editor(Some(HELLO), 0); // resumes the draft; the fix is autosaved
    let r = sb.fun(&["edit", "foo"]);
    assert_eq!(r.code, 0, "{}", r.stderr);
    assert!(r.stderr.contains("resuming your unsaved draft"));
    assert_eq!(sb.run_script("foo", &["again"]).stdout, "hello, again\n");
}

#[test]
fn with_autosave_off_nothing_is_installed_until_you_save() {
    let mut sb = Sandbox::new();
    sb.two_step();
    sb.set_editor(Some(HELLO), 0);
    let r = sb.fun(&["edit", "foo"]);
    assert_eq!(r.code, 0);
    assert!(r.stderr.contains("draft ok. install it: fun save foo"));
    assert_eq!(read(&sb.draft("foo")), HELLO);
    assert!(!sb.script("foo").exists() && fs::symlink_metadata(sb.bin().join("foo")).is_err());

    let r = sb.fun(&["save", "foo"]);
    assert_eq!(r.code, 0, "{}", r.stderr);
    assert!(r.stdout.starts_with("saved foo -> "));
    assert_eq!(sb.run_script("foo", &["x"]).stdout, "hello, x\n");
    let r = sb.fun(&["save", "foo"]);
    assert_eq!(r.code, 1);
    assert!(r.stderr.contains("nothing to commit"));
}

// -- edit: drafts ---------------------------------------------------------------------

#[test]
fn edit_resumes_an_existing_draft_instead_of_clobbering_it() {
    let mut sb = Sandbox::new();
    sb.two_step();
    sb.set_editor(Some(HELLO), 0);
    sb.fun(&["edit", "foo"]);
    sb.set_editor(None, 0);
    let r = sb.fun(&["edit", "foo", "python3"]);
    assert!(r.stderr.contains("resuming your unsaved draft"));
    assert!(r.stderr.contains("ignoring 'python3'"));
    assert_eq!(read(&sb.draft("foo")), HELLO);
}

#[test]
fn edit_rejects_a_draft_without_a_shebang_and_keeps_it() {
    let mut sb = Sandbox::new();
    sb.two_step();
    sb.set_editor(Some("echo no shebang\n"), 0);
    let r = sb.fun(&["edit", "foo"]);
    assert_eq!(r.code, 1);
    assert!(r.stderr.contains("shebang"));
    assert_eq!(read(&sb.draft("foo")), "echo no shebang\n");
}

#[test]
fn edit_editor_failure_and_empty_draft() {
    let mut sb = Sandbox::new();
    sb.set_editor(Some(HELLO), 3);
    let r = sb.fun(&["edit", "foo"]);
    assert_eq!(r.code, 1);
    assert!(r.stderr.contains("the editor exited"));
    assert!(!sb.script("foo").exists(), "a failed editor never autosaves");
    sb.set_editor(Some(""), 0);
    let r = sb.fun(&["edit", "bar"]);
    assert_eq!(r.code, 1);
    assert!(r.stderr.contains("draft is empty"));
}

#[test]
fn editor_may_have_arguments_and_quotes() {
    let mut sb = Sandbox::new();
    sb.set_editor(Some(HELLO), 0);
    let ed = sb.editor.clone().unwrap();
    let mut cmd = sb.command();
    cmd.env("EDITOR", format!("'{}' --flag 'quoted arg'", ed.display()));
    let r = run(cmd.args(["edit", "foo"]), "");
    assert_eq!(r.code, 0, "{}", r.stderr);
    assert_eq!(read(&sb.script("foo")), HELLO);
}

// -- save ------------------------------------------------------------------------------

#[test]
fn save_installs_a_link_to_the_fun_binary_which_runs_the_script() {
    let mut sb = Sandbox::new();
    sb.saved("foo", HELLO);
    let target = fs::read_link(sb.bin().join("foo")).unwrap();
    assert_eq!(fs::canonicalize(target).unwrap(), fs::canonicalize(FUN).unwrap());
    assert_ne!(
        fs::metadata(sb.script("foo")).unwrap().permissions().mode() & 0o100,
        0,
        "not executable"
    );
}

#[test]
fn save_refuses_when_there_is_nothing_valid_to_save() {
    let mut sb = Sandbox::new();
    let r = sb.fun(&["save", "nothing"]);
    assert_eq!(r.code, 1);
    assert!(r.stderr.contains("no draft"));

    sb.saved("foo", HELLO); // autosaved
    let r = sb.fun(&["save", "foo"]);
    assert_eq!(r.code, 1);
    assert!(r.stderr.contains("nothing to commit"));

    sb.two_step();
    sb.set_editor(Some("echo hi\n"), 0);
    sb.fun(&["edit", "bad"]);
    assert_eq!(sb.fun(&["save", "bad"]).code, 1);
    assert!(!sb.script("bad").exists() && !sb.bin().join("bad").exists());
}

#[test]
fn save_of_an_unchanged_draft_is_a_noop_that_tidies_up() {
    let mut sb = Sandbox::new();
    sb.saved("foo", HELLO);
    sb.two_step();
    fs::write(sb.draft("foo"), HELLO).unwrap();
    let r = sb.fun(&["save", "foo"]);
    assert_eq!(r.code, 0);
    assert!(r.stderr.contains("no changes to 'foo'"));
    assert!(!sb.draft("foo").exists());
}

#[test]
fn save_never_clobbers_a_real_file_or_someone_elses_symlink() {
    let mut sb = Sandbox::new();
    fs::create_dir_all(sb.bin()).unwrap();
    fs::write(sb.bin().join("real"), "precious").unwrap();
    let other = sb.root.join("other-tool");
    fs::write(&other, "x").unwrap();
    symlink(&other, sb.bin().join("linked")).unwrap();
    sb.set_editor(Some(HELLO), 0);
    for name in ["real", "linked"] {
        let r = sb.fun(&["edit", name]);
        assert_eq!(r.code, 1, "{name}");
        assert!(r.stderr.contains("isn't ours"));
    }
    assert_eq!(read(&sb.bin().join("real")), "precious");
    assert_eq!(fs::read_link(sb.bin().join("linked")).unwrap(), other);
}

#[test]
fn save_warns_about_a_missing_interpreter_but_still_saves() {
    let mut sb = Sandbox::new();
    sb.set_editor(Some("#!/usr/bin/env definitely-not-installed-xyz\necho hi\n"), 0);
    let r = sb.fun(&["edit", "foo"]);
    assert_eq!(r.code, 0);
    assert!(r
        .stderr
        .contains("'definitely-not-installed-xyz' isn't installed"));
    assert!(sb.script("foo").exists());
}

#[test]
fn save_explains_how_to_fix_a_bin_dir_that_is_not_on_path() {
    let mut sb = Sandbox::new();
    sb.set_editor(Some(HELLO), 0);
    let mut cmd = sb.command();
    cmd.env("PATH", "/usr/bin:/bin").env("SHELL", "/usr/bin/fish");
    let r = run(cmd.args(["edit", "foo"]), "");
    assert_eq!(r.code, 0, "{}", r.stderr);
    assert!(
        r.stderr.contains("isn't on $PATH") && r.stderr.contains("fish_add_path"),
        "{}",
        r.stderr
    );

    let mut cmd = sb.command();
    cmd.env("PATH", "/usr/bin:/bin").env("SHELL", "/bin/bash");
    let r = run(cmd.args(["edit", "bar"]), "");
    assert!(r.stderr.contains("export PATH=\"$HOME/"), "{}", r.stderr);
}

#[test]
fn save_leaves_no_temp_files_behind() {
    let mut sb = Sandbox::new();
    sb.saved("foo", HELLO);
    let names = |d: PathBuf| {
        fs::read_dir(d)
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .collect::<Vec<_>>()
    };
    assert_eq!(names(sb.bin()), ["foo"]);
    assert_eq!(names(sb.dir().join("scripts")), ["foo"]);
    assert!(names(sb.dir().join("drafts")).is_empty());
}

// -- duplicate names -------------------------------------------------------------------

#[test]
fn names_that_shadow_commands_or_builtins_are_refused_with_ideas_for_better_ones() {
    let mut sb = Sandbox::new();
    sb.set_editor(Some(HELLO), 0);
    let r = sb.fun(&["edit", "grep"]);
    assert_eq!(r.code, 1);
    assert!(r.stderr.contains("already a command"));
    assert!(r.stderr.contains("Try 'my-grep' or 'grep2'."), "{}", r.stderr);

    // Suggestions skip names that are taken, too.
    sb.saved("my-grep", HELLO);
    let r = sb.fun(&["edit", "grep"]);
    assert!(r.stderr.contains("Try 'grep2' or 'grep3'."), "{}", r.stderr);

    let r = sb.fun(&["edit", "cd"]);
    assert!(
        r.stderr.contains("shell builtin") && r.stderr.contains("Try 'my-cd'"),
        "{}",
        r.stderr
    );
    assert!(sb.fun(&["edit", "fun"]).stderr.contains("shell builtin"));
    assert_eq!(sb.fun(&["edit", "../etc"]).code, 1);
    assert_eq!(sb.fun(&["edit", "-x"]).code, 1); // a leading dash is never a valid name
}

#[test]
fn a_name_that_differs_only_by_case_points_at_the_existing_one() {
    let mut sb = Sandbox::new();
    sb.saved("foo", HELLO);
    let r = sb.fun(&["edit", "Foo"]);
    assert_eq!(r.code, 1);
    assert!(
        r.stderr.contains("'Foo' differs from 'foo' only by case"),
        "{}",
        r.stderr
    );
    assert!(r.stderr.contains("Use 'foo'"));
    assert!(!sb.dir().join("drafts/Foo").exists());

    // Drafts count too, and so does the other direction.
    sb.two_step();
    sb.fun(&["edit", "Draft"]);
    assert!(sb.fun(&["edit", "draft"]).stderr.contains("differs from 'Draft'"));
    assert_eq!(sb.fun(&["edit", "unrelated"]).code, 0);
}

#[test]
fn saved_names_can_be_edited_again_even_though_they_are_now_commands() {
    let mut sb = Sandbox::new();
    sb.saved("foo", HELLO);
    sb.set_editor(None, 0);
    assert_eq!(sb.fun(&["edit", "foo"]).code, 0);
}

// -- the launcher ----------------------------------------------------------------------

#[test]
fn scripts_get_their_arguments_stdin_and_exit_code() {
    let mut sb = Sandbox::new();
    sb.saved(
        "echoer",
        "#!/bin/sh\nread line\necho \"got $line / $1 / $2\"\nexit 7\n",
    );
    let mut cmd = sb.command_for(sb.bin().join("echoer"));
    cmd.args(["one", "two words"]);
    let out = run(&mut cmd, "typed\n");
    assert_eq!(out.stdout, "got typed / one / two words\n");
    assert_eq!(out.code, 7);
}

#[test]
fn scripts_die_of_sigpipe_like_any_other_program() {
    // Rust ignores SIGPIPE in its own process; the script must get the default back.
    let mut sb = Sandbox::new();
    sb.saved("spew", "#!/bin/sh\nwhile :; do echo y || exit 3; done\n");
    let cmd = format!(
        "set -o pipefail; '{}' | head -n1; echo \"status ${{PIPESTATUS[0]}}\"",
        sb.bin().join("spew").display()
    );
    let mut c = sb.command_for("bash");
    c.args(["-c", &cmd]);
    let out = run(&mut c, "");
    assert_eq!(out.stdout, "y\nstatus 141\n", "{}", out.stderr);
}

#[test]
fn a_missing_interpreter_is_reported_when_the_script_runs() {
    let mut sb = Sandbox::new();
    // Through `env`, env itself says what's missing...
    sb.saved(
        "nodeless",
        "#!/usr/bin/env definitely-not-installed-xyz\necho hi\n",
    );
    let out = sb.run_script("nodeless", &[]);
    assert_eq!(out.code, 127);
    assert!(
        out.stderr.contains("definitely-not-installed-xyz"),
        "{}",
        out.stderr
    );

    // ...and where the kernel would only say "No such file or directory", fun does.
    sb.saved("pathless", "#!/no/such/interpreter\necho hi\n");
    let out = sb.run_script("pathless", &[]);
    assert_eq!(out.code, 127);
    assert_eq!(
        out.stderr,
        "fun: pathless needs '/no/such/interpreter', which isn't installed\n"
    );
}

#[test]
fn a_link_with_no_script_behind_it_is_just_fun() {
    let sb = Sandbox::new();
    fs::create_dir_all(sb.bin()).unwrap();
    symlink(FUN, sb.bin().join("ghost")).unwrap();
    let out = sb.run_script("ghost", &[]);
    assert_eq!(out.code, 2); // no script called ghost: the CLI answers, bare, with its usage
    assert!(out.stderr.contains("usage:"));
}

#[test]
fn links_from_fun_1_0_are_still_ours_and_get_upgraded() {
    let mut sb = Sandbox::new();
    // fun 1.0 linked straight at the script.
    fs::create_dir_all(sb.dir().join("scripts")).unwrap();
    fs::create_dir_all(sb.bin()).unwrap();
    fs::write(sb.script("old"), HELLO).unwrap();
    fs::set_permissions(sb.script("old"), fs::Permissions::from_mode(0o755)).unwrap();
    symlink(sb.script("old"), sb.bin().join("old")).unwrap();
    assert_eq!(sb.run_script("old", &["a"]).stdout, "hello, a\n"); // still runs, uncounted
    assert_eq!(sb.table()[0][3], "0");

    // Opening and closing without a change heals it into a run-counting link...
    sb.set_editor(None, 0);
    let r = sb.fun(&["edit", "old"]);
    assert_eq!(r.code, 0, "{}", r.stderr);
    assert_eq!(
        fs::canonicalize(fs::read_link(sb.bin().join("old")).unwrap()).unwrap(),
        fs::canonicalize(FUN).unwrap()
    );
    sb.run_script("old", &[]);
    assert_eq!(sb.table()[0][3], "1");

    // ...and rm still recognises the old kind.
    fs::remove_file(sb.bin().join("old")).unwrap();
    symlink(sb.script("old"), sb.bin().join("old")).unwrap();
    assert_eq!(sb.fun(&["rm", "-f", "old"]).code, 0);
    assert!(fs::symlink_metadata(sb.bin().join("old")).is_err());
}

// -- list ------------------------------------------------------------------------------

#[test]
fn list_empty_keeps_stdout_clean() {
    let sb = Sandbox::new();
    let r = sb.fun(&["list"]);
    assert_eq!((r.code, r.stdout.as_str()), (0, ""));
    assert!(r.stderr.contains("nothing saved yet"));
}

#[test]
fn list_is_a_table_of_language_date_runs_lines_and_megabytes() {
    let mut sb = Sandbox::new();
    sb.saved("beta", "#!/usr/bin/env python3\nprint(1)\nprint(2)\n");
    sb.saved("alpha", HELLO);
    // Written by hand, with no newline at the end: the last line still counts.
    fs::write(sb.script("ragged"), "#!/bin/sh\necho a\necho b").unwrap();
    fs::write(sb.meta("alpha.created"), "1000000000\n").unwrap();
    fs::write(sb.meta("beta.created"), "1709164800\n").unwrap();
    fs::write(sb.meta("ragged.created"), "4102444800\n").unwrap();
    for _ in 0..3 {
        sb.run_script("alpha", &[]);
    }

    let mut cmd = sb.command();
    cmd.env("TZ", "UTC");
    let out = run(cmd.arg("list"), "");
    let rows: Vec<Vec<&str>> = out.stdout.lines().map(|l| l.split('\t').collect()).collect();
    assert_eq!(
        rows,
        [
            ["alpha", "sh", "2001-09-09", "3", "2", "<0.0001"],
            ["beta", "python3", "2024-02-29", "0", "3", "<0.0001"],
            ["ragged", "sh", "2100-01-01", "0", "3", "<0.0001"],
        ]
    );
}

#[test]
fn list_reports_megabytes_and_lines_of_a_big_script() {
    let mut sb = Sandbox::new();
    let body = format!("#!/bin/sh\n{}\n", "#".repeat(1_500_000 - 11)); // exactly 1,500,000 bytes
    sb.saved("big", &body);
    assert_eq!(fs::metadata(sb.script("big")).unwrap().len(), 1_500_000);
    let row = &sb.table()[0];
    assert_eq!((row[4].as_str(), row[5].as_str()), ("2", "1.5000"));
}

#[test]
fn list_dates_are_local_time() {
    let mut sb = Sandbox::new();
    sb.saved("foo", HELLO);
    fs::write(sb.meta("foo.created"), "1000000000\n").unwrap(); // 2001-09-09 01:46:40 UTC
    let date_in = |tz: &str| {
        let mut cmd = sb.command();
        cmd.env("TZ", tz);
        let out = run(cmd.arg("list"), "");
        out.stdout.split('\t').nth(2).unwrap().to_string()
    };
    assert_eq!(date_in("UTC"), "2001-09-09");
    assert_eq!(date_in("UTC-13"), "2001-09-09"); // UTC+13: 14:46 the same day
    assert_eq!(date_in("UTC+5"), "2001-09-08"); // UTC-5: 20:46 the day before
}

#[test]
fn list_dates_come_from_the_save_and_fall_back_to_the_file() {
    let mut sb = Sandbox::new();
    sb.saved("fresh", HELLO);
    let recorded: u64 = read(&sb.meta("fresh.created")).trim().parse().unwrap();
    assert!(now().abs_diff(recorded) < 60);

    // A script that predates the bookkeeping shows its file's date instead of nothing.
    fs::write(sb.script("ancient"), HELLO).unwrap();
    let row = sb.table().into_iter().find(|r| r[0] == "ancient").unwrap();
    assert!(
        row[2].len() == 10 && row[2].chars().nth(4) == Some('-'),
        "{row:?}"
    );
}

#[test]
fn list_counts_every_run() {
    let mut sb = Sandbox::new();
    sb.saved("foo", HELLO);
    assert_eq!(sb.table()[0][3], "0");
    for _ in 0..5 {
        sb.run_script("foo", &[]);
    }
    assert_eq!(sb.table()[0][3], "5");
}

#[test]
fn list_mentions_unsaved_drafts_on_stderr_only() {
    let mut sb = Sandbox::new();
    sb.saved("beta", HELLO);
    sb.two_step();
    sb.set_editor(Some(HELLO), 0);
    sb.fun(&["edit", "gamma"]); // never saved
    fs::write(sb.draft("beta"), "#!/bin/sh\necho v2\n").unwrap(); // pending change to a saved one
    let r = sb.fun(&["list"]);
    assert_eq!(r.stdout.lines().count(), 1);
    assert!(r.stdout.starts_with("beta\t"));
    assert!(r.stderr.contains("unsaved drafts: beta, gamma"), "{}", r.stderr);
    assert!(!sb.fun(&["ls"]).stdout.contains("gamma"));
}

#[test]
fn list_ignores_junk() {
    let mut sb = Sandbox::new();
    sb.saved("foo", HELLO);
    fs::write(sb.dir().join("scripts/.hidden"), "x").unwrap();
    fs::write(sb.dir().join("scripts/not valid"), "x").unwrap();
    fs::create_dir(sb.dir().join("scripts/subdir")).unwrap();
    let names: Vec<String> = sb.table().into_iter().map(|r| r[0].clone()).collect();
    assert_eq!(names, ["foo"]);
}

// -- show ------------------------------------------------------------------------------

#[test]
fn show_prints_the_source_verbatim() {
    let mut sb = Sandbox::new();
    sb.saved("foo", HELLO);
    assert_eq!(sb.fun(&["show", "foo"]).stdout, HELLO);
    assert_eq!(sb.fun(&["cat", "foo"]).stdout, HELLO);
    let r = sb.fun(&["show", "ghost"]);
    assert_eq!(r.code, 1);
    assert!(r.stderr.contains("no such script"));
    assert_eq!(sb.fun(&["show", "../../etc/passwd"]).code, 1);
}

#[test]
fn show_into_a_closed_pipe_exits_quietly() {
    let mut sb = Sandbox::new();
    let big = format!("#!/bin/sh\n{}", "# padding padding padding\n".repeat(200_000));
    sb.saved("big", &big);
    let mut child = sb
        .command()
        .args(["show", "big"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdout = child.stdout.take().unwrap();
    let mut one = [0u8; 1];
    stdout.read_exact(&mut one).unwrap();
    drop(stdout); // `| head -c1`
    let mut err = String::new();
    child.stderr.take().unwrap().read_to_string(&mut err).unwrap();
    assert_eq!(child.wait().unwrap().code(), Some(141));
    assert_eq!(err, "");
}

// -- rm --------------------------------------------------------------------------------

#[test]
fn rm_asks_first_and_no_tty_means_no() {
    let mut sb = Sandbox::new();
    sb.saved("foo", HELLO);
    let r = sb.fun_in(&["rm", "foo"], "n\n");
    assert_eq!(r.code, 1);
    assert!(r.stderr.contains("aborted"));
    assert_eq!(sb.fun_in(&["rm", "foo"], "").code, 1); // EOF
    assert!(sb.script("foo").exists() && sb.bin().join("foo").exists());
}

#[test]
fn rm_yes_removes_script_link_draft_and_bookkeeping() {
    let mut sb = Sandbox::new();
    sb.saved("foo", HELLO);
    sb.run_script("foo", &[]);
    fs::write(sb.draft("foo"), HELLO).unwrap();
    let r = sb.fun_in(&["del", "foo"], "Yes\n");
    assert_eq!(r.code, 0);
    assert_eq!(r.stdout, "deleted foo\n");
    assert!(!sb.script("foo").exists() && !sb.draft("foo").exists());
    assert!(fs::symlink_metadata(sb.bin().join("foo")).is_err());
    assert!(!sb.meta("foo.runs").exists() && !sb.meta("foo.created").exists());

    // A script of the same name later starts from zero.
    sb.saved("foo", HELLO);
    assert_eq!(sb.table()[0][3], "0");
}

#[test]
fn rm_force_takes_several_and_reports_the_unknown_ones() {
    let mut sb = Sandbox::new();
    sb.saved("a", HELLO);
    sb.saved("b", HELLO);
    let r = sb.fun(&["rm", "-f", "a", "ghost", "b"]);
    assert_eq!(r.code, 1);
    assert!(r.stderr.contains("fun: no script or draft named 'ghost'"));
    assert!(!sb.script("a").exists() && !sb.script("b").exists()); // it kept going
}

#[test]
fn rm_never_deletes_a_binary_that_isnt_ours() {
    // A pending draft makes `fun rm foo` legal even though foo was never saved.
    let sb = Sandbox::new();
    fs::create_dir_all(sb.dir().join("drafts")).unwrap();
    fs::write(sb.draft("foo"), HELLO).unwrap();
    fs::create_dir_all(sb.bin()).unwrap();
    fs::write(sb.bin().join("foo"), "precious").unwrap();
    assert_eq!(sb.fun(&["rm", "-f", "foo"]).code, 0);
    assert_eq!(read(&sb.bin().join("foo")), "precious");
    assert!(!sb.draft("foo").exists());
}

// -- cli -------------------------------------------------------------------------------

#[test]
fn help_shows_the_settings_you_would_get_and_why() {
    let sb = Sandbox::new();
    let r = sb.fun(&["help"]);
    assert_eq!(r.code, 0);
    assert!(r.stdout.contains("fun edit <name> [lang]"));
    assert!(r.stdout.contains("config.toml"));
    assert!(
        r.stdout.contains("language = \"bash\"  (default)"),
        "{}",
        r.stdout
    );
    assert!(r.stdout.contains("editor   = \"vim\"  (default)"));
    assert!(r.stdout.contains("autosave = true  (default)"));

    sb.write_config("language = \"python3\"\nautosave = false\n");
    let mut cmd = sb.command();
    cmd.env("EDITOR", "nano");
    let r = run(cmd.arg("help"), "");
    assert!(
        r.stdout.contains("language = \"python3\"  (config)"),
        "{}",
        r.stdout
    );
    assert!(r.stdout.contains("editor   = \"nano\"  ($EDITOR)"));
    assert!(r.stdout.contains("autosave = false  (config)"));
}

#[test]
fn help_survives_a_broken_config() {
    let sb = Sandbox::new();
    sb.write_config("nonsense\n");
    let r = sb.fun(&["help"]);
    assert_eq!(r.code, 0);
    assert!(r.stdout.contains("config.toml:1:"), "{}", r.stdout);
}

#[test]
fn help_version_and_usage_errors() {
    let sb = Sandbox::new();
    let bare = sb.fun(&[]);
    assert_eq!(bare.code, 2);
    assert!(bare.stderr.contains("usage:") && bare.stdout.is_empty());
    for h in ["help", "-h", "--help"] {
        let r = sb.fun(&[h]);
        assert_eq!(r.code, 0);
        assert!(r.stdout.contains("fun edit <name>"));
    }
    assert_eq!(sb.fun(&["edit", "-h"]).code, 0);
    assert_eq!(
        sb.fun(&["--version"]).stdout,
        format!("fun {}\n", env!("CARGO_PKG_VERSION"))
    );
    for args in [
        &["edit"][..],
        &["edit", "a", "b", "c"],
        &["save"],
        &["save", "a", "b"],
        &["list", "x"],
        &["rm"],
        &["rm", "-z", "a"],
        &["frobnicate"],
    ] {
        let r = sb.fun(args);
        assert_eq!(r.code, 2, "{args:?}");
        assert!(!r.stderr.is_empty());
    }
}

#[test]
fn xdg_data_home_is_honoured_and_fun_dir_wins() {
    let mut sb = Sandbox::new();
    sb.set_editor(Some(HELLO), 0);
    let xdg = sb.root.join("xdg");
    let mut cmd = sb.command();
    cmd.env_remove("FUN_DIR").env("XDG_DATA_HOME", &xdg);
    assert_eq!(run(cmd.args(["edit", "foo"]), "").code, 0);
    assert!(xdg.join("fun/scripts/foo").exists());
    let mut cmd = sb.command();
    cmd.env("XDG_DATA_HOME", &xdg);
    assert_eq!(run(cmd.args(["edit", "bar"]), "").code, 0);
    assert!(sb.script("bar").exists()); // FUN_DIR took precedence
}
