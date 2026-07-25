#!/bin/sh
# Make the pre-push lint gates mean what they claim.
#
# The pre-push hooks below this one (`cargo clippy`, the macOS cross-check)
# build the *working tree*, not the commits being pushed. With uncommitted
# changes, or when pushing a ref that isn't HEAD, they report a result about
# code that isn't in the push -- so a commit that fails `-D warnings` sails
# through on the strength of fixes that only exist in your editor.
#
# Rather than teach every gate to check out the pushed commit (a cold rebuild
# per push), refuse to vouch at all unless the tree already matches what's
# going out. Then "working tree" and "the push" are the same thing and the
# gates are honest.
#
# Bypass with `git push --no-verify`.
set -eu

head=$(git rev-parse HEAD)
zero=0000000000000000000000000000000000000000

# git feeds pre-push "<local ref> <local sha> <remote ref> <remote sha>" on
# stdin. If the runner doesn't forward it we just get nothing and fall
# through to the dirty-tree check, which is still worth having.
while read -r _local_ref local_sha _remote_ref _remote_sha; do
    [ "$local_sha" = "$zero" ] && continue # branch deletion, nothing to lint
    if [ "$local_sha" != "$head" ]; then
        echo "push gate: pushing $(git rev-parse --short "$local_sha") but HEAD is $(git rev-parse --short "$head")."
        echo "The lint gates build the working tree, so they would describe HEAD"
        echo "rather than the commit you are pushing."
        echo "Check that commit out (or push from a worktree on it) and retry."
        exit 1
    fi
done

# Anything that changes what cargo compiles must already be committed.
if ! git diff --quiet HEAD -- '*.rs' '*Cargo.toml' '*Cargo.lock' 'rust-toolchain.toml'; then
    echo "push gate: build inputs differ from HEAD:"
    git diff --name-only HEAD -- '*.rs' '*Cargo.toml' '*Cargo.lock' 'rust-toolchain.toml' |
        sed 's/^/  /'
    echo
    echo "Linting now would describe your working tree, not the commit being"
    echo "pushed. Commit or stash these, then push."
    exit 1
fi
