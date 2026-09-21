//! The completion scripts, run by the real shells (fish tests are skipped where fish isn't installed).

use std::fs;
use std::path::PathBuf;
use std::process::Command;

fn root() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("fun-comp-{}", std::process::id()));
    fs::create_dir_all(dir.join("scripts")).unwrap();
    fs::create_dir_all(dir.join("drafts")).unwrap();
    for f in ["scripts/alpha", "scripts/beta", "drafts/gamma"] {
        fs::write(dir.join(f), "").unwrap();
    }
    dir
}

fn completions(file: &str) -> String {
    format!("{}/completions/{file}", env!("CARGO_MANIFEST_DIR"))
}

fn have(shell: &str) -> bool {
    Command::new(shell).arg("--version").output().is_ok()
}

fn shell(shell: &str, args: &[&str], script: &str, fun_dir: &PathBuf) -> String {
    let out = Command::new(shell)
        .args(args)
        .arg(script)
        .env("FUN_DIR", fun_dir)
        .output()
        .unwrap_or_else(|e| panic!("can't run {shell}: {e}"));
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8_lossy(&out.stdout).into_owned()
}

#[test]
fn bash_completes_verbs_names_languages_and_flags() {
    let dir = root();
    let script = format!(
        r#"source {}
c() {{ COMP_WORDS=("$@"); COMP_CWORD=$(( ${{#COMP_WORDS[@]}} - 1 )); COMPREPLY=(); _fun; echo "${{COMPREPLY[*]}}"; }}
c fun ''
c fun edit ''
c fun edit x py
c fun save ''
c fun show a
c fun rm -
c fun save gamma ''
"#,
        completions("fun.bash")
    );
    let out = shell("bash", &["-c"], &script, &dir);
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(
        lines,
        [
            "edit save list show rm help",
            "alpha beta gamma",
            "python3",
            "gamma",
            "alpha",
            "-f --force",
            ""
        ]
    );
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn fish_completes_verbs_names_languages_and_flags() {
    if !have("fish") {
        eprintln!("fish not installed; skipping");
        return;
    }
    let dir = root();
    let script = format!(
        "source {}\n\
         function c; echo (complete -C \"$argv[1]\" | string replace -r '\\t.*' '' | string join ' '); end\n\
         c 'fun '\nc 'fun edit '\nc 'fun edit x '\nc 'fun save '\nc 'fun show '\nc 'fun rm --'\nc 'fun save gamma '\n",
        completions("fun.fish")
    );
    let out = shell("fish", &["--no-config", "-c"], &script, &dir);
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(
        lines,
        [
            "edit help list rm save show",
            "alpha beta gamma",
            "bash fish lua node python3 ruby sh zsh",
            "gamma",
            "alpha beta",
            "--force",
            ""
        ]
    );
    let _ = fs::remove_dir_all(dir);
}
