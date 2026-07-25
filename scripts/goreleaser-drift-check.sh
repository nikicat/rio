#!/bin/sh
# Catch upstream release-flag changes that the merge can't deliver.
#
# The fork replaced upstream's `.goreleaser.yaml` with per-arch OSS configs,
# because upstream's needs a GoReleaser Pro licence plus Apple/winget
# credentials the fork doesn't have (see 6671181ea0). Since that file no
# longer exists here, git reports no conflict when upstream edits it — the
# changes simply never arrive, silently. That is how `--features=wgpu` sat
# missing from fork release builds for six weeks after upstream added it.
#
# Compare the cargo feature set of the Linux build entries on both sides and
# fail when they drift. Only the feature sets are compared: the rest of the
# two files is legitimately different (the fork drops macOS, Windows, winget,
# brew, notarization and cargo-publish) and always will be.
#
# Bypass with `git push --no-verify`.
set -eu

UPSTREAM_REF=${RIO_UPSTREAM_REF:-origin/main}
FORK_CONFIGS='.goreleaser.amd64.yaml .goreleaser.arm64.yaml'

if ! git rev-parse --verify -q "$UPSTREAM_REF" >/dev/null 2>&1; then
    echo "goreleaser drift: $UPSTREAM_REF not fetched, skipping"
    exit 0
fi

# Cargo features named on build entries that target Linux (wayland or x11).
# `tr` splits both `--features=a,b` and separate `--features=` flags.
features_of() {
    grep -E '^[[:space:]]*flags:.*--features=(wayland|x11)' |
        tr ',' '\n' |
        sed -n 's/.*--features=\([a-zA-Z0-9_-]*\).*/\1/p' |
        sort -u
}

upstream=$(git show "$UPSTREAM_REF:.goreleaser.yaml" 2>/dev/null | features_of || true)
fork=$(cat $FORK_CONFIGS | features_of)

if [ -z "$upstream" ]; then
    echo "goreleaser drift: no Linux build entries found in $UPSTREAM_REF:.goreleaser.yaml"
    echo "(upstream may have restructured it — check by hand)"
    exit 1
fi

if [ "$upstream" != "$fork" ]; then
    echo "goreleaser drift: Linux build features differ from $UPSTREAM_REF."
    echo
    echo "  upstream (.goreleaser.yaml):     $(echo "$upstream" | tr '\n' ' ')"
    echo "  fork (.goreleaser.*.yaml):       $(echo "$fork" | tr '\n' ' ')"
    echo
    echo "Upstream edits that file; the fork replaced it, so a merge cannot"
    echo "deliver them. Port the difference into $FORK_CONFIGS by hand,"
    echo "or update this check if the divergence is deliberate."
    exit 1
fi
