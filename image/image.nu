#!/usr/bin/env nix-shell
#! nix-shell -p nushell -i nu

def main [] {
    let script_dir = $env.CURRENT_FILE | path dirname
    print $"change cwd to ($script_dir)"
    cd $script_dir

    let uid = id -u
    let uname = id -un
    let gid = id -g
    let gname = id -gn
    $"{ uid = ($uid); gid = ($gid); uname = \"($uname)\"; gname = \"($gname)\"; }" | save -f id.nix

    cd $script_dir
    print $"building image with uid: ($uid) gid: ($gid)"
    nix build
    docker load -i ./result
    podman load -i ./result

    # docker run --rm -ti agent-box:latest bash
}
