{ pkgs, aiTools, nix-index-database-pkgs }:

with pkgs; [
  bash
  curl
  wget
  jq
  ripgrep
  fd
  tree
  neovim
  jujutsu
  strace
  gnused
  gawk
  diffutils
  nodejs_24
  python315
  gnumake
  lsof
  unixtools.netstat
  gnupg
  tokei
  file
  dua
  yazi
  bat
  delta
  glow
  bun
  uv
  ty
  mypy
  nix-search-tv
  gh
  direnv

  aiTools.pi
  aiTools.claude-code
  aiTools.tuicr

  nix-index-database-pkgs.nix-index-with-db
  nix-index-database-pkgs.comma-with-db
]
