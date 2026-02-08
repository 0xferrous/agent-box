# Agent Box Docker Image

Docker image built with Nix, using the official nix docker.nix as base with customizable packages on top.

## Quick Start

```bash
cd image
nix build
docker load -i result
```

Or use the helper script which auto-generates `id.nix` from current user:

```bash
cd image
./image.nu
```

## Publish to GHCR

Use the Nushell helper to build and push the image. It will use `GITHUB_REPOSITORY`, `GITHUB_ACTOR`, and `GITHUB_TOKEN` when available (handy in GitHub Actions):

```bash
cd image
./image.nu build-and-push
```

Override the target repository/tag or credentials if needed:

```bash
cd image
./image.nu build-and-push --repository ghcr.io/owner/agent-box --tag latest
./image.nu build-and-push --username my-user --token my-token
```

## Entrypoint Behavior

The image uses an entrypoint that wraps commands with `nix develop` when a `flake.nix` is present:

**With `flake.nix` found (searching up the directory tree):**
- **No arguments**: Runs `nix develop --command bash` (interactive shell in the flake's devshell)
- **With arguments**: Runs `nix develop --command <args...>` (executes the provided command in the devshell)

**Without `flake.nix` (searching up the directory tree):**
- **No arguments**: Runs `bash` directly
- **With arguments**: Runs `<args...>` directly

This means projects with a `flake.nix` automatically get their development environment activated, while other directories work normally.

**Examples:**
```bash
# Interactive shell in devshell (if flake.nix exists)
docker run -it agent-box

# Run a specific command in devshell
docker run -it agent-box cargo build

# Override entrypoint for plain bash (bypass nix develop even with flake.nix)
docker run -it --entrypoint /bin/bash agent-box
```

## How It Works

- **Nix-based**: Uses the official nix docker.nix from the nix flake
- **User configuration**: Reads uid/gid/uname/gname from `id.nix` file
- **Extensible packages**: Add packages via `packages` parameter (passed as `extraPkgs` to nix docker.nix)

## Default Packages

The default image includes (on top of nix base):
- **Shell & Utils**: bash
- **Network**: curl, wget
- **Text Processing**: jq, ripgrep, fd, tree, gnused, gawk, diffutils
- **Editor**: neovim
- **VCS**: jujutsu
- **Debug**: strace, lsof, unixtools.netstat
- **Languages**: nodejs_24, python315
- **Build**: gnumake
- **Security**: gnupg
- **AI Tools**: pi, claude-code (from nix-ai-tools)

## Configuration

Create `id.nix` in the image directory to configure user/group:

```nix
{
  uid = 1000;
  gid = 1000;
  uname = "myuser";
  gname = "mygroup";
}
```

## Customizing Packages

Edit `flake.nix` and modify the `defaultPackages` list:

```nix
defaultPackages = with pkgs; [
  bash
  git
  # Add your packages here
  python3
  nodejs
];
```

## Setting Environment Variables

You can persist environment variables directly in the Docker image by passing the `env` parameter to `buildImage`:

```nix
default = buildImage {
  packages = defaultPackages;
  directories = [ "${userHome}/.local" ];
  env = {
    MY_VAR = "my_value";
    API_KEY = "secret";
    LOG_LEVEL = "debug";
  };
};
```

These environment variables are set directly in the Docker image's `Env` configuration and are available to all processes running in the container.

## Building with Custom Configuration

Use the `custom` output to build with custom packages:

```bash
nix build .#custom --impure --expr '
  (builtins.getFlake (toString ./.)).packages.x86_64-linux.custom {
    packages = [ pkgs.python3 pkgs.nodejs ];
  }
'
```

## Parameters

The `buildImage` function accepts:

- `packages` (list): Packages to include on top of nix base
- `directories` (list): Directories to create in the image (can be strings or attrsets with `path` and `mode`)
- `env` (attrset): Environment variables to set in the Docker image

User configuration is read from `id.nix`:
- `uid` (int): User ID
- `gid` (int): Group ID
- `uname` (string): Username
- `gname` (string): Group name

## Architecture Support

- x86_64-linux
- aarch64-linux

## Inputs

The flake uses these inputs:
- `nixpkgs`: NixOS unstable packages
- `nix`: Nix 2.33.1 (provides docker.nix base)
- `nix-ai-tools`: AI tools (pi, claude-code)
