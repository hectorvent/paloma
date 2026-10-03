# completion

```
paloma completion <bash|zsh|fish|powershell|elvish>
```

Prints a completion script to stdout.

```bash
paloma completion zsh > "${fpath[1]}/_paloma"           # zsh
paloma completion bash > ~/.local/share/bash-completion/completions/paloma
paloma completion fish > ~/.config/fish/completions/paloma.fish
```

Release archives also ship pre-generated completions and man pages (`dist-assets/`).
