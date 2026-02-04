- use `nix develop --command` to run the commands in the flake's devshell
- use conventionalcommits.org pattern when writing commit messages

## pre commit

- format code with `cargo fmt`
- check code with `cargo check`
- check clippy with `cargo clippy`
- always keep the docs and readme in sync with the code changes
- keep the table of contents in the @README.md up to date whenever a change is made

## image

The image flake uses a custom `docker.nix` from the `nix` input (github:0xferrous/nix/extra-args).

To get the nix store path of the `nix` flake input:

```bash
nix eval --raw --impure --expr '(builtins.getFlake (toString ./image)).inputs.nix'
# Returns: /nix/store/<hash>-source
```

To read the `docker.nix` file from that input:

```bash
cat $(nix eval --raw --impure --expr '(builtins.getFlake (toString ./image)).inputs.nix')/docker.nix
```

This is referenced in `image/flake.nix` as:
```nix
pkgs.callPackage "${nix}/docker.nix" { ... }
```
