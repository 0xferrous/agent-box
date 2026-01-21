# Agent Box Docker Image

Multi-layered Docker image with nix as base and customizable packages on top.

## Quick Start

```bash
cd image
nix build
docker load -i result
```

## How It Works

- **Multi-layered**: Uses `buildLayeredImage` - automatically creates up to 100 layers
- **Nix included**: `pkgs.nix` gets its own layer(s)
- **Package layers**: Each package gets its own layer for efficient caching
- **User setup**: Configurable uid/gid (default: 1000:1000, user: agent)

## Default Packages

Layered on top of nix base image:
- bash, coreutils
- git, curl, wget
- jq, ripgrep, fd, tree
- neovim
- binSh, caCertificates

## Customizing Packages

Edit `flake.nix` and modify the `defaultPackages` list (image/flake.nix:59-70):

```nix
defaultPackages = with pkgs; [
  bash
  git
  # Add your packages here
  python3
  nodejs
];
```

## Building with Custom Configuration

```bash
nix build --impure --expr '
  let
    flake = builtins.getFlake (toString ./.);
    nixpkgs = import <nixpkgs> {};
  in
  flake.packages.x86_64-linux.custom {
    packages = with nixpkgs; [ python3 nodejs rustc ];
    uid = 1000;
    gid = 1000;
    uname = "myuser";
    gname = "mygroup";
  }
'
```

## Parameters

- `packages` (list): Packages to add on top of nix base
- `uid` (int): User ID (default: 1000)
- `gid` (int): Group ID (default: 1000)
- `uname` (string): Username (default: "agent")
- `gname` (string): Group name (default: "agent")

## Architecture Support

- x86_64-linux
- aarch64-linux
