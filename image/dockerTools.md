 Docker Tools in Nixpkgs - Complete Reference

  Main Location: pkgs/build-support/docker/default.nix:1-1444

  ---
  Core Image Building Functions

  1. dockerTools.buildImage (default.nix:611)

  Creates a Docker-compatible repository tarball with a single layer.

  Key Features:
  - Single layer for all files/dependencies
  - Supports privileged operations via runAsRoot (uses VM with KVM)
  - Reproducible builds with static creation dates
  - Good for simple images or when you need root scripts

  Parameters:
  - name (String) - Image name
  - tag (String | Null) - Image tag (uses nix hash if null)
  - fromImage (Path | Null) - Base image to extend
  - fromImageName/fromImageTag - Select specific image from multi-image tarball
  - copyToRoot (Path | List | Null) - Files to add to image root
  - keepContentsDirlinks (Boolean) - Preserve directory symlinks
  - config (AttrSet | Null) - Docker container config (Cmd, Env, ExposedPorts, etc.)
  - architecture (String) - Target architecture (default: host arch)
  - extraCommands (String) - Bash script run before finalizing
  - runAsRoot (String | Null) - Privileged bash script (runs in VM)
  - diskSize (Number) - VM disk size in MiB (default: 1024)
  - buildVMMemorySize (Number) - VM memory (default: 512)
  - created (String) - ISO-8601 timestamp or "now" (default: "1970-01-01T00:00:01Z")
  - uid/gid (Number) - File ownership (default: 0)
  - compressor (String) - "none", "gz", "zstd" (default: "gz")
  - includeNixDB (Boolean) - Add nix database (default: false)
  - meta (AttrSet) - Derivation metadata

  Outputs:
  - Main: compressed tarball ready for docker load
  - buildArgs - Arguments passed to buildImage
  - layer - The layer derivation
  - imageTag - Generated tag

  ---
  2. dockerTools.buildLayeredImage (default.nix:578)

  Creates multi-layer Docker image with automatic layer optimization.

  Key Features:
  - Multiple layers - one per store object for better sharing
  - No KVM required - uses fakeroot instead of VM
  - Popular objects get dedicated layers
  - Wrapper around streamLayeredImage that stores result

  Parameters:
  - name (String) - Image name
  - tag (String | Null) - Image tag
  - contents (Path | List) - Directories for final layer
  - config (AttrSet | Null) - Docker configuration
  - fromImage (Path | Null) - Base image
  - maxLayers (Number) - Max layers (default: 100, Docker max: 125)
  - extraCommands (String) - Bash customization script
  - fakeRootCommands (String) - Script in fakeroot environment
  - enableFakechroot (Boolean) - Use proot for chroot (default: false)
  - includeStorePaths (Boolean) - Include store files (default: true)
  - includeNixDB (Boolean) - Add nix database (default: false)
  - created (String) - Creation timestamp
  - mtime (String) - File modification time
  - uid/gid/uname/gname - File ownership
  - compressor (String) - Compression (default: "gz")
  - meta/passthru - Derivation attributes

  ---
  3. dockerTools.streamLayeredImage (default.nix:1000)

  Builds a script that streams a Docker image (doesn't store in Nix store).

  Key Features:
  - Most efficient - saves IO, disk, and cache space
  - Streams directly to stdout when executed
  - Perfect for CI/CD pipelines: $(nix-build) | docker load
  - Same layering algorithm as buildLayeredImage

  Parameters: Same as buildLayeredImage

  Outputs:
  - Main: executable script
  - imageTag - Image tag
  - isExe - True
  - conf - Config JSON
  - streamScript - Python streaming script

  Usage Example:
  $(nix-build -A myImage) | docker load

  ---
  4. dockerTools.pullImage (default.nix:141)

  Pull Docker images from registries (like docker pull).

  Key Features:
  - Uses Docker Registry HTTP API V2
  - Returns uncompressed tarball
  - Requires two hashes: imageDigest (for registry) + sha256 (for Nix)

  Parameters:
  - imageName (String) - Image name (prepend registry if not docker.io)
  - imageDigest (String) - Digest from registry
  - outputHash (String) - Nix hash of result (use nix-prefetch-docker)
  - outputHashAlgo (String) - Hash algorithm
  - os (String) - OS (default: "linux")
  - arch (String) - Architecture (default: host arch)
  - finalImageName/finalImageTag - Rename after download
  - tlsVerify (Boolean) - TLS verification (default: true)
  - name (String | Null) - Store path name

  Helper: Use nix-prefetch-docker script to get both hashes

  ---
  5. dockerTools.exportImage (default.nix:376)

  Export Docker image filesystem as tarball (like docker export).

  Key Features:
  - Extracts filesystem only (no Docker metadata)
  - Can re-import with docker import
  - Requires KVM device

  Parameters:
  - fromImage (Path | AttrSet) - Source image
  - fromImageName/fromImageTag - Select from multi-image tarball
  - diskSize (Number) - VM disk in MiB (default: 1024)
  - name (String | Null) - Output name

  ---
  Advanced Functions

  6. dockerTools.mergeImages (default.nix:902)

  Merge multiple Docker images into single tarball.

  Use Case: Load multiple images with one docker load

  Parameters:
  - images (List) - List of image tarballs

  ---
  7. dockerTools.streamNixShellImage (default.nix:1260)

  Stream Docker image with nix-shell-like environment.

  Key Features:
  - Build derivations inside container
  - Includes Nix, all build inputs, shellHook
  - Non-reproducible - matches current system environment

  Parameters:
  - drv (Derivation) - Derivation to create environment for
  - name/tag - Image identification
  - uid/gid - User/group ID (default: 1000)
  - homeDirectory (String) - Home dir (default: "/build")
  - shell (String) - Shell binary (default: bashInteractive)
  - command (String | Null) - Command to run
  - run (String | Null) - Non-interactive command

  ---
  8. dockerTools.buildNixShellImage (default.nix:1429)

  Same as streamNixShellImage but stores result in Nix store.

  Additional Parameter:
  - compressor (String) - Compression (default: "gz")

  ---
  Convenience Wrappers

  9. dockerTools.buildImageWithNixDb (default.nix:995)

  Calls buildImage with includeNixDB = true

  10. dockerTools.buildLayeredImageWithNixDb (default.nix:997)

  Calls buildLayeredImage with includeNixDB = true

  ---
  Helper Utilities

  11. dockerTools.shadowSetup (default.nix:232)

  Bash script string that sets up shadow-utils files.

  Creates:
  - /etc/passwd, /etc/shadow, /etc/group, /etc/gshadow
  - PAM configuration
  - /etc/login.defs
  - Adds shadow utilities to PATH

  Usage: Include in runAsRoot or fakeRootCommands

  ---
  12. dockerTools.usrBinEnv (default.nix:967)

  Provides /usr/bin/env from coreutils (package derivation)

  13. dockerTools.binSh (default.nix:974)

  Provides /bin/sh linked to bashInteractive

  Use Case: Enables interactive containers (docker run -it)

  14. dockerTools.caCertificates (default.nix:980)

  Adds trusted TLS/SSL root certificates

  Locations:
  - /etc/ssl/certs/ca-bundle.crt
  - /etc/ssl/certs/ca-certificates.crt
  - /etc/pki/tls/certs/ca-bundle.crt

  15. dockerTools.fakeNss (default.nix:963)

  Re-export of fakeNss package (minimal /etc/passwd and /etc/group)

  ---
  Internal Functions (Advanced Use)

  16. mkPureLayer (default.nix:419)

  Create layer without privileged operations

  17. mkRootLayer (default.nix:482)

  Create layer requiring root via VM

  18. mkDbExtraCommand (default.nix:58)

  Generate nix database in image

  19. shellScript (default.nix:409)

  Create shell script with coreutils

  20. mergeDrvs (default.nix:200)

  Merge multiple derivations

  ---
  Compression Options
  ┌────────────┬───────────┬──────┬─────────┐
  │ Compressor │ Extension │ Tool │ Default │
  ├────────────┼───────────┼──────┼─────────┤
  │ "none"     │ ""        │ -    │ No      │
  ├────────────┼───────────┼──────┼─────────┤
  │ "gz"       │ ".gz"     │ pigz │ Yes     │
  ├────────────┼───────────┼──────┼─────────┤
  │ "zstd"     │ ".zst"    │ zstd │ No      │
  └────────────┴───────────┴──────┴─────────┘
  ---
  Decision Guide
  ┌──────────────────────────────────┬───────────────────────────┐
  │               Need               │            Use            │
  ├──────────────────────────────────┼───────────────────────────┤
  │ Simple image, one layer          │ buildImage                │
  ├──────────────────────────────────┼───────────────────────────┤
  │ Need root scripts (apt-get, etc) │ buildImage with runAsRoot │
  ├──────────────────────────────────┼───────────────────────────┤
  │ Better layer sharing             │ buildLayeredImage         │
  ├──────────────────────────────────┼───────────────────────────┤
  │ No KVM available                 │ buildLayeredImage         │
  ├──────────────────────────────────┼───────────────────────────┤
  │ Save disk space                  │ streamLayeredImage        │
  ├──────────────────────────────────┼───────────────────────────┤
  │ CI/CD pipeline                   │ streamLayeredImage        │
  ├──────────────────────────────────┼───────────────────────────┤
  │ Pull from registry               │ pullImage                 │
  ├──────────────────────────────────┼───────────────────────────┤
  │ Filesystem only export           │ exportImage               │
  ├──────────────────────────────────┼───────────────────────────┤
  │ Development environment          │ streamNixShellImage       │
  ├──────────────────────────────────┼───────────────────────────┤
  │ Use Nix in container             │ *WithNixDb variants       │
  ├──────────────────────────────────┼───────────────────────────┤
  │ Load multiple images             │ mergeImages               │
  └──────────────────────────────────┴───────────────────────────┘
  ---
  Documentation & Examples

  - Main docs: doc/build-helpers/images/dockertools.section.md
  - Examples: pkgs/build-support/docker/examples.nix (25 examples)
  - Tests: nixos/tests/docker-tools*.nix (4 test files)

  ---
  Architecture Detection

  Uses go.GOARCH for default architecture:
  - Common: 386, amd64, arm, arm64
  - Override with architecture parameter

  All functions and their complete parameter sets are now documented!
