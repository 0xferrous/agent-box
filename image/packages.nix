{ pkgs, aiTools }:

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
  comma
  dua
  yazi
  bat
  delta
  glow
  procps
  podman

  aiTools.pi
  aiTools.claude-code
]
