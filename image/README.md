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
