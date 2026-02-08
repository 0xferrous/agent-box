#!/usr/bin/env nix-shell
#! nix-shell -p nushell -i nu

def build-image [] {
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

def build-and-push [
    --repository (-r): string = null
    --tag (-t): string = "latest"
    --username (-u): string = null
    --token (-k): string = null
] {
    let repo = if $repository == null {
        let repo_env = $env.GITHUB_REPOSITORY?
        if $repo_env == null {
            error make {
                msg: "Set --repository or GITHUB_REPOSITORY to push to GHCR."
            }
        }
        $"ghcr.io/($repo_env)/agent-box"
    } else {
        $repository
    }

    let image = $"($repo):($tag)"

    let gh_user = if $username == null { $env.GITHUB_ACTOR? } else { $username }
    let gh_token = if $token == null { $env.GITHUB_TOKEN? } else { $token }

    if $gh_user != null and $gh_token != null {
        docker login ghcr.io -u $gh_user -p $gh_token
    }

    build-image
    docker tag agent-box:latest $image
    docker push $image
}

def main [] {
    build-image
}
