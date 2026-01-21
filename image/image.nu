#!/usr/bin/env nix-shell
#! nix-shell -p nushell -i nu

def main [] {
    let script_dir = $env.CURRENT_FILE | path dirname
    print $"change cwd to ($script_dir)"
    cd $script_dir

    let agent = cat ~/.agent-box.toml | from toml | get agent
    let uid = id -u $agent.user
    let gid = id -g $agent.group
    $"{ uid = ($uid); gid = ($gid); uname = \"($agent.user)\"; gname = \"($agent.group)\"; }" | save -f id.nix

    cd $script_dir
    print $"building image with uid: ($uid) gid: ($gid)"
    nix build
    docker load -i ./result

    docker run --rm -ti agent-box:latest bash
}
