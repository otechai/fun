# fun completions for fish. Names come from fish itself (no process spawned per <TAB>).

function __fun_names --description 'names in fun’s scripts/ or drafts/ directory' --argument-names kind
    set -l root ~/.local/share/fun
    test -n "$XDG_DATA_HOME"; and set root $XDG_DATA_HOME/fun
    test -n "$FUN_DIR"; and set root $FUN_DIR
    for f in $root/$kind/*
        string replace -r '.*/' '' -- $f
    end
end

# True while completing the Nth argument after the subcommand: `fun edit NAME LANG` -> 1, 2.
function __fun_arg --argument-names n
    test (count (commandline -opc)) -eq (math $n + 1)
end

complete -c fun -f

complete -c fun -n __fish_use_subcommand -a edit -d 'write a script'
complete -c fun -n __fish_use_subcommand -a save -d 'install the draft on your $PATH'
complete -c fun -n __fish_use_subcommand -a list -d 'scripts and drafts'
complete -c fun -n __fish_use_subcommand -a show -d 'print a script'
complete -c fun -n __fish_use_subcommand -a rm -d 'delete scripts'
complete -c fun -n __fish_use_subcommand -a help -d 'usage and current settings'

complete -c fun -n '__fish_seen_subcommand_from edit e; and __fun_arg 1' -a '(__fun_names scripts; __fun_names drafts)'
complete -c fun -n '__fish_seen_subcommand_from edit e; and __fun_arg 2' -a 'bash sh zsh fish python3 node ruby lua' -d language
complete -c fun -n '__fish_seen_subcommand_from save s; and __fun_arg 1' -a '(__fun_names drafts)'
complete -c fun -n '__fish_seen_subcommand_from show cat; and __fun_arg 1' -a '(__fun_names scripts)'
complete -c fun -n '__fish_seen_subcommand_from rm del' -a '(__fun_names scripts)'
complete -c fun -n '__fish_seen_subcommand_from rm del' -s f -l force -d "don't ask first"
