# Releasing ctx

Three distribution channels, three different mechanisms. Only the first is automated.

## 1. GitHub release (automated by tag)

```
# on main, with CI green
scripts/coherence/coherence.sh            # 9/9
bash scripts/coherence/fitcheck-local.sh  # must reach Ship
git tag v0.7.3 && git push origin v0.7.3
```

Pushing a `v*.*.*` tag runs `.github/workflows/release.yml`: a preflight that checks the tag matches
`version` in Cargo.toml and that `CHANGELOG.md` has a `## [<version>]` heading, then a build matrix,
the coherence suite, and a GitHub Release carrying `ctx-<target>.tar.gz` plus `checksums.txt`.

Bump `version` in Cargo.toml and add the changelog heading **before** tagging. The preflight fails
the release otherwise, and it fails after the tag exists, so you end up deleting and re-pushing it.

## 2. Homebrew tap

The tap is a separate repository, `saurabh0392/homebrew-ctx`, and nothing updates it automatically.
Run this **after** the GitHub release has published its assets:

```
scripts/release-brew.sh 0.7.3 --push
```

It downloads both macOS tarballs, computes their checksums, rewrites `Formula/ctx.rb`, commits, and
pushes. Without `--push` it stops after the commit so you can read the diff. It refuses to write a
formula pointing at a release that does not exist, because a formula with a valid-looking checksum
and a 404 url fails on the user's machine rather than here.

Users then get it with `brew update && brew upgrade ctx`.

## 3. crates.io

Published as **`ctx-agent`** (the `ctx` name was taken); the binary is still `ctx`.

```
cargo publish --dry-run     # confirm the package contents and size
cargo publish
```

This needs a crates.io API token that is not stored in the repo:
`cargo login` once, with a token from https://crates.io/settings/tokens.

Packaging uses an `include` allowlist in Cargo.toml, not `exclude`. This matters: `exclude` only
removes what it names, so any untracked file sitting in the working tree gets published. That once
produced a 156 MiB package against a 10 MiB limit, because a sibling service's `node_modules` and a
demo video happened to be on disk. The allowlist keeps the crate at roughly 2.3 MiB and identical
from any checkout. If you add a file the binary reads at build or run time, add it to `include` or
it will be missing for anyone installing from crates.io.

Publishing is permanent. A version can be yanked but never replaced, so check `cargo publish
--dry-run` output before running the real thing.
