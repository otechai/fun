# fun completions for bash (3.2+). Sourced by bash-completion, or by hand: source completions/fun.bash
# Names are [A-Za-z0-9_-] only, so plain word splitting is safe here.

_fun() {
    local cur=${COMP_WORDS[COMP_CWORD]} verb=${COMP_WORDS[1]}
    local root=${FUN_DIR:-${XDG_DATA_HOME:-$HOME/.local/share}/fun}
    local kinds k f
    local -a names=()

    if ((COMP_CWORD == 1)); then
        COMPREPLY=($(compgen -W "edit save list show rm help" -- "$cur"))
        return
    fi

    case $verb in
        edit | e)
            if ((COMP_CWORD == 3)); then
                COMPREPLY=($(compgen -W "bash sh zsh fish python3 node ruby lua" -- "$cur"))
                return
            fi
            ((COMP_CWORD == 2)) || return
            kinds="scripts drafts" ;;
        save | s) ((COMP_CWORD == 2)) || return; kinds=drafts ;;
        show | cat) ((COMP_CWORD == 2)) || return; kinds=scripts ;;
        rm | del)
            if [[ $cur == -* ]]; then
                COMPREPLY=($(compgen -W "-f --force" -- "$cur"))
                return
            fi
            kinds=scripts ;;
        *) return ;;
    esac

    for k in $kinds; do
        for f in "$root/$k"/*; do
            [[ -f $f ]] && names+=("${f##*/}")
        done
    done
    COMPREPLY=($(compgen -W "${names[*]}" -- "$cur"))
}
complete -F _fun fun
