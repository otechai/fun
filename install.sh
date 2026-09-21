#!/bin/sh
# Build and install fun, plus shell completions for whichever of fish/bash you use.
# Run from inside the checkout:  ./install.sh
set -eu

here=$(cd "$(dirname "$0")" && pwd)

if ! command -v cargo >/dev/null 2>&1; then
    echo "install.sh: need cargo (https://rustup.rs)" >&2
    exit 1
fi

cargo install --path "$here" --locked --force

if command -v fish >/dev/null 2>&1 || [ -d "${XDG_CONFIG_HOME:-$HOME/.config}/fish" ]; then
    dir="${XDG_CONFIG_HOME:-$HOME/.config}/fish/completions"
    mkdir -p "$dir"
    cp "$here/completions/fun.fish" "$dir/fun.fish"
    echo "fish completions -> $dir/fun.fish"
fi

if command -v bash >/dev/null 2>&1; then
    dir="${XDG_DATA_HOME:-$HOME/.local/share}/bash-completion/completions"
    mkdir -p "$dir"
    cp "$here/completions/fun.bash" "$dir/fun"
    echo "bash completions -> $dir/fun (needs the bash-completion package, or: source it from ~/.bashrc)"
fi

case ":$PATH:" in
    *":${CARGO_HOME:-$HOME/.cargo}/bin:"*) ;;
    *) echo "add cargo's bin dir to your PATH:  ${CARGO_HOME:-\$HOME/.cargo}/bin" ;;
esac
case ":$PATH:" in
    *":$HOME/.local/bin:"*) ;;
    *) echo "fun puts your scripts in ~/.local/bin, which isn't on your PATH yet" ;;
esac
echo "fun installed. Try: fun help"
