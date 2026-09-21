# fun

`funced` and `funcsave` for any scripting language. Write a script, close the editor,
and it's a command. No project, no `chmod +x`, no `mv` into `$PATH`.

```console
$ fun edit hello              # your editor opens on a fresh bash script; write something, quit
new script hello (bash)
saved hello -> ~/.local/share/fun/scripts/hello
$ hello                       # it's already a command
hello, world
$ fun edit report python3     # any language: it's only a shebang
```

**Why `fun`?** It's the three letters left of `function`, the word fish's `funced` and
`funcsave` grew from, and throwaway scripts are the fun kind.

## Install

```sh
git clone https://github.com/otechai/fun.git
cd fun
./install.sh        # cargo install, plus fish and bash completions
```

Needs Rust 1.70+. There are no dependencies, and the result is one ~0.5 MB binary.
Keep the `fun` binary on a stable path (`~/.cargo/bin` is), because your scripts link to it.
Put `~/.local/bin` on your `$PATH`; fun tells you how, in your shell's dialect, the first
time it matters.

## Commands

Five, and that's all of them.

| command | does | alias |
| --- | --- | --- |
| `fun edit <name> [lang]` | write a script; it's saved and runnable when the editor closes | `e` |
| `fun save <name>` | install a pending draft (see autosave below) | `s` |
| `fun list` | a table: language, created, runs, lines, size | `ls` |
| `fun show <name>` | print a script | `cat` |
| `fun rm [-f] <name>...` | delete scripts (asks first unless `-f`) | `del` |

## Autosave

In fish, `funced` defines the function and you can call it right away; `funcsave` only makes
it permanent. `fun` folds both into closing the editor: the moment it exits, the script is
checked, saved, and on your `$PATH`. Type its name.

It is careful about what it saves:

- **Nothing written, nothing saved.** Open the editor on a new script and quit without
  typing, and no script is created (an editor tidying the last blank line doesn't count).
- **No changes, no save.** Editing a saved script and leaving it alone touches nothing.
- **Invalid drafts are kept, not installed.** No shebang, or the editor failed? The draft
  stays; `fun edit` resumes it, and it saves when it's valid.

If you'd rather review before installing, turn it off (`autosave = false` below) and use
`fun save` when you're ready: the classic two-step.

## Configuration

`~/.config/fun/config.toml` (or `$XDG_CONFIG_HOME/fun/config.toml`, or `$FUN_CONFIG`):

```toml
language = "bash"   # the shebang for new scripts: bash, python3, node, fish, /bin/zsh, ...
editor   = "vim"    # any command, arguments allowed: "code --wait", "nvim -u NONE"
autosave = true     # save the moment the editor closes
```

Those are the defaults: **bash** and **vim** are on practically every machine.

- **language**: config, else `bash`. `fun edit x python3` overrides it for one script.
- **editor**: config, else `$VISUAL`, else `$EDITOR`, else `vim`.
- The file is a small TOML subset: `key = "value"`, `#` comments, values taken literally.
  A mistake is reported with its line (`config.toml:3: unknown key 'lang'`) instead of being
  ignored, and only `fun edit` ever reads the file.
- `fun help` prints the settings you'd get right now and where each one comes from.

### Languages

A language is just an interpreter name (`env` finds it) or a full path. There's no list to
keep up to date, so anything with a shebang works: `bash`, `zsh`, `fish`, `python3`, `node`,
`ruby`, `lua`, `awk`, `/opt/thing/bin/run`. Shorthands: `py` and `python` mean `python3`,
`js` and `javascript` mean `node`, `rb` means `ruby`.

New shell scripts start safe: `bash` and `zsh` get `set -euo pipefail`, POSIX shells
`set -eu`, anything else just the shebang. With vim, nvim, nano, emacs or kak, the cursor
starts on the first empty line.

## Names

A name is a command, so fun protects the ones already taken:

- **It exists as a script:** `fun edit hello` edits that script (you're told so).
- **It's already a command or a shell builtin** (`ls`, `grep`, `cd`): refused, with free names
  to use instead: `'grep' is already a command (/usr/bin/grep). Try 'my-grep' or 'grep2'.`
- **It differs only by case from a script or draft** (`Hello` next to `hello`): refused, and
  pointed at the one you have. Two look-alike names are a trap, and on case-insensitive disks
  they're the same file.
- **Something else owns `~/.local/bin/<name>`:** never touched.

## `fun list`

```console
$ fun list
NAME    LANGUAGE  CREATED     RUNS  LINES  SIZE (MB)
backup  bash      2026-09-20    12      4     0.0001
hello   python3   2026-09-19     3      3     0.0001
js      node      2026-09-20     1      3    <0.0001
```

- **LANGUAGE** is read from the shebang, **CREATED** is the local date the script was first
  saved (editing doesn't change it), **RUNS** counts every time you run it, **LINES** counts
  lines as an editor would, and **SIZE** is megabytes (10^6 bytes), or `<0.0001` for a script
  too small to show.
- Piped, it's headerless tab-separated lines, ready for `cut` and `awk`. Unsaved drafts are
  mentioned on stderr, so they never pollute the data.

### How runs are counted

`~/.local/bin/<name>` is a symlink to the `fun` binary. Started under a script's name, fun
appends a byte to a counter file, then `exec`s your script, so no `fun` process lingers, and
arguments, stdin, stdout, exit codes and signals behave as if you'd run the script directly.
The cost is about half a millisecond per run. The counter is an append-only file, so parallel
runs can't lose counts. A few things follow:

- `$0` inside a script is its real path (`~/.local/share/fun/scripts/<name>`), not `<name>`.
- If you set `$FUN_DIR`, set it wherever your scripts run (cron, other shells), not just in
  one shell.
- If the interpreter is missing, you're told which one instead of "No such file or directory".

## Guarantees

- **Never clobbers what isn't its own.** fun only replaces or deletes
  `~/.local/bin/<name>` when it's *its own link*. A real file or a link into another tool
  stops the save, and `fun rm` follows the same rule.
- **Nothing unrunnable is installed.** The first line must be a shebang. If its interpreter
  isn't installed, fun still saves and tells you.
- **Never loses an edit.** An unsaved draft is resumed, never overwritten.
- **Atomic.** Saving is one `rename`, and the link is swapped with a rename too. A crash or
  ^C can't leave a half-written script or a missing link.
- **Deleting is clean.** `fun rm` removes the script, its link and its counters, so a script
  recreated later starts from zero.

## Where things live

```
~/.local/share/fun/scripts/<name>        saved scripts   ($FUN_DIR, or $XDG_DATA_HOME/fun)
~/.local/share/fun/drafts/<name>         edits in progress
~/.local/share/fun/meta/<name>.created   when it was first saved
~/.local/share/fun/meta/<name>.runs      one byte per run
~/.local/bin/<name>                      link to the fun binary   ($FUN_BIN)
```

## Completion

`install.sh` installs completions for fish and bash. They complete commands, script and draft
names (`fun save <TAB>` offers only drafts), languages for `fun edit x <TAB>`, and `rm -f`.
Names are read by the shell itself, so no process is spawned per `<TAB>`.

## Coming from pf 3.0 or fun 1.0

Scripts saved by `pf`/`fun 1.0` keep working, and fun still recognises their links as its
own. They just aren't counted yet: open one with `fun edit <name>` and quit, and it's
relinked. To move over from `pf`:

```sh
mv ~/.local/share/pf ~/.local/share/fun
for f in ~/.local/share/fun/scripts/*; do ln -sf "$f" ~/.local/bin/"${f##*/}"; done
cargo uninstall pf
```

(If you had set `$XDG_DATA_HOME`, the first path is `$XDG_DATA_HOME/pf`.)

## Design

Five verbs, one file per concern, no dependencies:

```
src/main.rs     dispatch and exit codes: 0 ok, 1 failed, 2 usage, 141 closed pipe
src/cli.rs      hand-rolled argument parser (five verbs don't need clap)
src/cmd.rs      edit, save, list, show, rm; one `install` shared by save and autosave
src/launch.rs   what happens when a script's link is run: count, then exec
src/store.rs    Name (a validated newtype), paths, link ownership, counters, name checks
src/config.rs   the config file: parsing, precedence, and `fun help`'s summary
src/lang.rs     languages -> shebang + template; reading interpreters back out
src/table.rs    the `fun list` table: pretty for terminals, tab-separated for pipes
src/date.rs     local dates from the C library, with a plain-arithmetic fallback
src/ui.rs       user-facing message helpers, so the voice stays consistent
src/error.rs    one Error enum; `.context()` for io errors
```

Status messages go to stderr and data to stdout, so `fun ls | wc -l` and `fun show x | head`
behave. A `fun` call takes about a millisecond, most of it process startup.

## Development

```sh
cargo test                                   # unit tests, plus end-to-end tests of the real binary
cargo clippy --all-targets -- -D warnings
```

The end-to-end tests give every test its own home, config, and `$PATH`, with stand-in editors;
the completion tests run real bash (and real fish, when installed).

## License

See `LICENSE` (the GNU AGPL v3 text).
