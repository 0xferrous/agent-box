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

  aiTools.pi
  aiTools.claude-code
]
