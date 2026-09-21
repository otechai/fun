//! The five commands.

use std::fs::{self, File, OpenOptions};
use std::io::{self, IsTerminal, Write};
use std::path::Path;
use std::process::Command;

use crate::config;
use crate::date;
use crate::error::{Context, Error, Result};
use crate::lang;
use crate::store::{first_line, make_executable, remove_if_exists, Link, Name, Store};
use crate::table::{self, Row};
use crate::ui::{confirm, note, path_hint, tilde};

// -- edit ------------------------------------------------------------------------

#[derive(PartialEq)]
enum Start {
    New,
    FromSaved,
    Resumed,
}

pub fn edit(store: &Store, name: &str, lang: Option<&str>) -> Result<()> {
    let name = Name::parse(name)?;
    store.ensure_name_is_free(&name)?;
    let settings = config::load()?; // before touching disk: a broken config shouldn't leave litter
    store.ensure_data_dirs()?;
    let (script, draft) = (store.script(&name), store.draft(&name));

    let saved = match fs::read(&script) {
        Ok(bytes) => Some(bytes),
        Err(e) if e.kind() == io::ErrorKind::NotFound => None,
        Err(e) => return Err(e).context(|| format!("can't read {}", tilde(&script))),
    };
    // Only a brand-new script needs a template (and so only then can the language be wrong).
    let template = match saved {
        Some(_) => None,
        None => Some(lang::template(lang.unwrap_or(&settings.language.value))?),
    };
    let seed = match (&saved, &template) {
        (Some(bytes), _) => bytes.clone(),
        (None, Some(t)) => t.text.clone().into_bytes(),
        (None, None) => unreachable!("a script is either saved or templated"),
    };

    // create_new: two `fun edit`s can't race, and an existing draft is never clobbered.
    let start = match OpenOptions::new().write(true).create_new(true).open(&draft) {
        Ok(mut f) => {
            f.write_all(&seed)
                .context(|| format!("can't write {}", tilde(&draft)))?;
            if saved.is_some() {
                Start::FromSaved
            } else {
                Start::New
            }
        }
        Err(e) if e.kind() == io::ErrorKind::AlreadyExists => Start::Resumed,
        Err(e) => return Err(e).context(|| format!("can't create {}", tilde(&draft))),
    };
    match (&start, &template) {
        (Start::New, Some(t)) => note(format!("new script {name} ({})", t.label)),
        (Start::FromSaved, _) => note(format!("{name} exists: editing the saved script")),
        (Start::Resumed, _) => note(format!("{name}: resuming your unsaved draft")),
        _ => {}
    }
    if let Some(lang) = lang.filter(|_| start != Start::New) {
        note(format!("ignoring '{lang}': {name} already exists"));
    }

    // A fresh template opens with the cursor where you start typing.
    let fresh = template.as_ref().filter(|_| start == Start::New);
    run_editor(&settings.editor.value, &draft, fresh.map(|t| t.cursor_line))?;

    if settings.autosave.value {
        // Closing the editor is saving: the script is a command by the time you're back at the prompt.
        let outcome = install(store, &name, &draft, fresh.map(|t| t.text.as_bytes()))?;
        report(store, &name, &outcome);
    } else {
        validate(&draft)?;
        note(format!("draft ok. install it: fun save {name}"));
    }
    Ok(())
}

/// Run the editor through `sh -c`, the way git does, so `code --wait` and quoted paths just work.
fn run_editor(editor: &str, draft: &Path, cursor: Option<usize>) -> Result<()> {
    let jump = cursor
        .filter(|_| supports_plus_line(editor))
        .map(|n| format!(" +{n}"))
        .unwrap_or_default();
    let status = Command::new("sh")
        .arg("-c")
        .arg(format!("{editor}{jump} \"$@\""))
        .arg("fun-editor") // $0
        .arg(draft)
        .status()
        .context(|| "can't run sh".into())?;
    match status.code() {
        Some(0) => Ok(()),
        Some(127) => Err(Error::fail(format!(
            "couldn't run the editor '{editor}'. Is it installed? Set `editor` in your config or $EDITOR."
        ))),
        _ => Err(Error::fail(format!(
            "the editor exited with {status}. Draft kept at {}.",
            tilde(draft)
        ))),
    }
}

/// Editors that take `+LINE` before the file (the vi/emacs/nano convention).
fn supports_plus_line(editor: &str) -> bool {
    let prog = editor.split_whitespace().next().unwrap_or("");
    let base = Path::new(prog).file_name().and_then(|n| n.to_str());
    matches!(
        base,
        Some("vim" | "vi" | "nvim" | "view" | "nano" | "emacs" | "kak")
    )
}

/// The kernel decides how to run a script from its first line, so that's what we check.
/// Returns the first line, for the interpreter check.
fn validate(draft: &Path) -> Result<String> {
    let line = match first_line(draft) {
        Ok(line) => line,
        Err(e) if e.kind() == io::ErrorKind::NotFound => Vec::new(),
        Err(e) => return Err(e).context(|| format!("can't read {}", tilde(draft))),
    };
    if line.is_empty() {
        return Err(Error::fail(format!(
            "the draft is empty. Did the editor crash, or did you? ({})",
            tilde(draft)
        )));
    }
    if !line.starts_with(b"#!") {
        return Err(Error::fail(
            "the first line must be a shebang like #!/usr/bin/env bash; without one nothing can run it. \
             The draft is kept: fix it with `fun edit`.",
        ));
    }
    Ok(String::from_utf8_lossy(&line).into_owned())
}

// -- save ------------------------------------------------------------------------

pub fn save(store: &Store, name: &str) -> Result<()> {
    let name = Name::parse(name)?;
    let draft = store.draft(&name);
    if !draft.is_file() {
        return Err(Error::fail(if store.script(&name).is_file() {
            format!("'{name}' is already saved and has no pending draft. Clean tree, nothing to commit.")
        } else {
            format!("no draft for '{name}'. Run 'fun edit {name}' first; fun doesn't do speculative fiction.")
        }));
    }
    let outcome = install(store, &name, &draft, None)?;
    report(store, &name, &outcome);
    Ok(())
}

enum Outcome {
    Installed,
    /// The draft is what's already saved.
    Unchanged,
    /// A new script that's still just the template: you opened the editor and left.
    Untouched,
}

/// Turn a draft into the saved script and make it runnable. Shared by `save` and autosave, so the
/// checks are the same either way: nothing invalid, unchanged, or blank is ever installed.
fn install(store: &Store, name: &Name, draft: &Path, untouched: Option<&[u8]>) -> Result<Outcome> {
    let shebang = validate(draft)?;
    let data = fs::read(draft).context(|| format!("can't read {}", tilde(draft)))?;
    if untouched.is_some_and(|template| same_but_for_trailing_space(&data, template)) {
        remove_if_exists(draft)?;
        return Ok(Outcome::Untouched);
    }

    store.ensure_name_is_free(name)?; // before anything moves
    let script = store.script(name);
    let existing = match fs::read(&script) {
        Ok(bytes) => Some(bytes),
        Err(e) if e.kind() == io::ErrorKind::NotFound => None,
        Err(e) => return Err(e).context(|| format!("can't read {}", tilde(&script))),
    };
    if existing.as_deref() == Some(data.as_slice()) {
        remove_if_exists(draft)?;
        if matches!(store.link_state(name), Link::Absent | Link::Legacy) {
            store.install_link(name)?; // heal a lost link, or upgrade to a run-counting one
        }
        return Ok(Outcome::Unchanged);
    }

    let predating = existing.as_ref().and_then(|_| Store::age_of(&script));
    make_executable(draft)?;
    fs::rename(draft, &script).context(|| format!("can't move the draft to {}", tilde(&script)))?; // atomic
    store.record_created(name, predating);
    store.install_link(name)?;

    if let Some(interp) = lang::missing_interpreter(&shebang) {
        note(format!(
            "note: '{interp}' isn't installed, so {name} won't run until it is"
        ));
    }
    if !store.bin_on_path() {
        note(format!(
            "note: {} isn't on $PATH. Add it: {}",
            tilde(store.bin_dir()),
            path_hint(store.bin_dir())
        ));
    }
    Ok(Outcome::Installed)
}

/// Editors love to tidy the end of a file; that isn't writing a script.
fn same_but_for_trailing_space(a: &[u8], b: &[u8]) -> bool {
    fn trimmed(x: &[u8]) -> &[u8] {
        &x[..x
            .iter()
            .rposition(|c| !c.is_ascii_whitespace())
            .map_or(0, |i| i + 1)]
    }
    trimmed(a) == trimmed(b)
}

fn report(store: &Store, name: &Name, outcome: &Outcome) {
    match outcome {
        Outcome::Installed => println!("saved {name} -> {}", tilde(&store.script(name))),
        Outcome::Unchanged => note(format!("no changes to '{name}'")),
        Outcome::Untouched => note(format!("nothing written, so '{name}' wasn't saved")),
    }
}

// -- list / show -------------------------------------------------------------------

pub fn list(store: &Store) -> Result<()> {
    let rows: Vec<Row> = store
        .saved_names()?
        .iter()
        .filter_map(|n| row(store, n))
        .collect();
    let drafts = store.draft_names()?;

    if rows.is_empty() {
        note("nothing saved yet. Try: fun edit hello");
    } else {
        // A table for people, bare tab-separated lines for pipes.
        let text = table::render(&rows, io::stdout().is_terminal());
        io::stdout()
            .lock()
            .write_all(text.as_bytes())
            .context(|| "can't write to stdout".into())?;
    }
    if !drafts.is_empty() {
        let names: Vec<String> = drafts.iter().map(Name::to_string).collect();
        note(format!(
            "unsaved drafts: {}. Resume one with: fun edit <name>",
            names.join(", ")
        ));
    }
    Ok(())
}

fn row(store: &Store, name: &Name) -> Option<Row> {
    let data = fs::read(store.script(name)).ok()?;
    let head = data.split(|&b| b == b'\n').next().unwrap_or_default();
    Some(Row {
        name: name.to_string(),
        language: lang::label(&String::from_utf8_lossy(head)),
        created: store.created_at(name).map_or_else(|| "-".into(), date::ymd),
        runs: store.run_count(name),
        lines: table::count_lines(&data),
        bytes: data.len() as u64,
    })
}

pub fn show(store: &Store, name: &str) -> Result<()> {
    let name = Name::parse(name)?;
    let path = store.script(&name);
    let mut file = File::open(&path).map_err(|e| match e.kind() {
        io::ErrorKind::NotFound => Error::fail(format!("no such script '{name}'. Checked the shelf twice.")),
        _ => Error::fail(format!("can't open {}: {e}", tilde(&path))),
    })?;
    io::copy(&mut file, &mut io::stdout().lock()).context(|| "can't write to stdout".into())?;
    Ok(())
}

// -- rm --------------------------------------------------------------------------

pub fn rm(store: &Store, names: &[String], force: bool) -> Result<()> {
    let mut failed = false;
    for raw in names {
        if let Err(e) = Name::parse(raw).and_then(|n| remove_one(store, &n, force)) {
            e.report();
            failed = true;
        }
    }
    if failed {
        Err(Error::Reported)
    } else {
        Ok(())
    }
}

fn remove_one(store: &Store, name: &Name, force: bool) -> Result<()> {
    let (script, draft) = (store.script(name), store.draft(name));
    let has_script = script.is_file();
    if !has_script && !draft.is_file() {
        return Err(Error::fail(format!(
            "no script or draft named '{name}'. Can't delete what was never there."
        )));
    }
    if !force && !confirm(&format!("delete '{name}'? [y/N] ")) {
        note("aborted. Your script lives to segfault another day.");
        return Err(Error::Reported);
    }
    // Link first, so nothing ever points at a script that's already gone. And only *our* link:
    // `rm -f ~/.local/bin/<name>` on somebody's real binary is how you lose an afternoon.
    match store.link_state(name) {
        state if state.is_ours() => remove_if_exists(&store.link(name))?,
        Link::Foreign if has_script => {
            note(format!("left {} alone: it isn't ours.", tilde(&store.link(name))))
        }
        _ => {}
    }
    remove_if_exists(&script)?;
    remove_if_exists(&draft)?;
    store.forget(name)?; // a script recreated later starts from zero
    println!("deleted {name}");
    Ok(())
}
