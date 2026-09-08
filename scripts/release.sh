#!/usr/bin/env bash
#
# Publish a downloadable macOS build of the game as a GitHub release.
#
#   ./scripts/release.sh              # tag from the workspace version, e.g. v0.1.0
#   ./scripts/release.sh v0.2.0-test  # explicit tag
#   ./scripts/release.sh --draft      # publish privately first, to check the page
#   ./scripts/release.sh --dry-run    # build and package, publish nothing
#   ./scripts/release.sh --skip-gate  # skip tests + scenarios (don't)
#
# What ships is a .tar.gz, not a .zip, and that is deliberate: Archive Utility
# propagates the com.apple.quarantine xattr onto everything it unpacks from a
# downloaded zip, so an unsigned binary is refused before it ever runs. `tar`
# sets no xattrs on what it extracts, so the same build opens with one fewer
# argument with Gatekeeper. The README still explains the escape hatch, because
# a double-clicked .tar.gz goes through Archive Utility too.

set -euo pipefail

# Rust was installed with --no-modify-path, so a non-login shell has no cargo.
if ! command -v cargo >/dev/null 2>&1 && [ -f "$HOME/.cargo/env" ]; then
    . "$HOME/.cargo/env"
fi

cd "$(git rev-parse --show-toplevel)"

die() { printf 'release: %s\n' "$1" >&2; exit 1; }

tag=""
gate=1
draft=""
dry=0
for arg in "$@"; do
    case "$arg" in
        --skip-gate) gate=0 ;;
        --draft)     draft="--draft" ;;
        --dry-run)   dry=1 ;;
        -*)          die "unknown flag $arg" ;;
        *)           [ -z "$tag" ] || die "two tags given: $tag and $arg"; tag="$arg" ;;
    esac
done

# One definition of the version, and it is the manifest's.
version="$(cargo pkgid -p arpg | sed 's/.*[@#]//')"
[ -n "$version" ] || die "could not read the version out of cargo pkgid"
tag="${tag:-v$version}"

# uname, not a hardcoded triple: this builds for the host and says so on the tin,
# rather than claiming a universal binary it did not lipo together.
arch="$(uname -m)"
name="arpg-$tag-macos-$arch"

# --- Everything that can refuse, refuses before the build. ---------------------

head="$(git rev-parse HEAD)"

if [ "$dry" -eq 0 ]; then
    command -v gh >/dev/null 2>&1 || die "gh is not installed: brew install gh"
    gh auth status >/dev/null 2>&1 || die "gh is not logged in: gh auth login"
    git remote get-url origin >/dev/null 2>&1 || die "no 'origin' remote to publish to"

    if gh release view "$tag" >/dev/null 2>&1; then
        die "release $tag already exists — pass a new tag, or: gh release delete $tag --cleanup-tag"
    fi

    # gh tags the commit on the *remote*, so an unpushed HEAD produces a release
    # pointing at a commit nobody can fetch. Refuse rather than push: pushing is
    # the author's call, not this script's.
    if ! git branch -r --contains "$head" 2>/dev/null | grep -q .; then
        die "HEAD ($(git rev-parse --short HEAD)) is not on any remote branch — git push first"
    fi
fi

if ! git diff --quiet || ! git diff --cached --quiet; then
    printf 'release: warning — uncommitted changes are NOT in this build (it builds %s)\n' \
        "$(git rev-parse --short HEAD)" >&2
fi

# --- The gate. ----------------------------------------------------------------

if [ "$gate" -eq 1 ]; then
    printf '==> cargo test --workspace\n'
    cargo test --workspace --quiet
    printf '==> scenarios\n'
    cargo run --quiet --release -p scenario -- scenarios/
fi

# --- Build and stage. ---------------------------------------------------------

printf '==> cargo build --release\n'
cargo build --release -p arpg

staging="$(mktemp -d)"
trap 'rm -rf "$staging"' EXIT
payload="$staging/$name"
mkdir -p "$payload"

cp target/release/arpg "$payload/arpg"

# The app reads `scenes` relative to the working directory (crates/app/src/app.rs,
# scene_catalog), so the F2 picker is empty unless the game is launched from the
# extracted folder. play.command is what makes that true for a double-click.
cp -R scenes "$payload/scenes"

cat > "$payload/play.command" <<'LAUNCHER'
#!/bin/sh
# Double-click me. The cd is load-bearing: the scene picker (F2) looks for
# `scenes` in the working directory, which Finder otherwise sets to your home.
cd "$(dirname "$0")" || exit 1
exec ./arpg
LAUNCHER
chmod +x "$payload/play.command"

# Smaller download, and then re-signed: strip invalidates the ad-hoc signature
# the linker applies, and an arm64 binary with a broken signature is killed
# outright rather than merely warned about.
strip -x "$payload/arpg" 2>/dev/null || true
codesign --force --sign - "$payload/arpg" >/dev/null 2>&1 || true

cat > "$payload/README.txt" <<READER
arpg $tag — macOS ($arch)

Run it
    Double-click play.command.
    Or from a terminal, in this folder:  ./arpg

If macOS refuses to open it
    This build is unsigned and un-notarized, so Gatekeeper quarantines it.
    From a terminal, in this folder:

        xattr -dr com.apple.quarantine .

    then try again. (Unpacking with \`tar xzf\` instead of double-clicking the
    archive usually avoids this entirely.)

Controls
    WASD / arrows   move
    space           swing
    F1              attack tuning panel   (Left/Right adjust recovery, R resets)
    F2              scene picker          (Up/Down select, Enter starts fresh)
    [ / ]           halve / double the horde
    - / =           zoom
    V               toggle vsync
    P               screenshot (to \$TMPDIR)
    Esc             quit

Requires an Apple Silicon Mac with Metal. Built from $(git rev-parse --short HEAD).
READER

mkdir -p dist
archive="dist/$name.tar.gz"
tar -czf "$archive" -C "$staging" "$name"
printf '==> %s (%s)\n' "$archive" "$(du -h "$archive" | cut -f1)"

# --- Publish. -----------------------------------------------------------------

if [ "$dry" -eq 1 ]; then
    printf '==> dry run: built and packaged, published nothing\n'
    exit 0
fi

notes="$(cat <<NOTES
Download \`$name.tar.gz\`, unpack it, and double-click \`play.command\`.

Unsigned build — if macOS refuses to open it, run \`xattr -dr com.apple.quarantine .\`
in the unpacked folder. Apple Silicon / Metal only. See README.txt inside for controls.
NOTES
)"

gh release create "$tag" "$archive" \
    --title "arpg $tag" \
    --notes "$notes" \
    --target "$head" \
    $draft

gh release view "$tag" --json url --jq .url
