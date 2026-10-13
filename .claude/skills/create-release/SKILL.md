---
name: create-release
description: Create a rust-microsip version tag and GitHub Release. Use when the user asks to release, tag, publish or cut a version. Releases are allowed only when CI on main is green for the tagged commit, and a final version also needs every release gate checked and the owner's decision.
---

# Releasing rust-microsip

Binaries are built only by `.github/workflows/release.yml` on `v*.*.*` tags. Checks (fmt, clippy,
tests, rustdoc, notices, AppImage, publication scan, cargo-deny) run only in `ci.yml` on `main`.
See `docs/release.md`.

## Release rules

- **Only tag a commit whose `ci` run on `main` is green.** The workflow refuses otherwise.
- **A final version needs the owner's explicit decision** and `docs/release/vX.Y.Z.md` with every
  gate checked (the workflow refuses any `- [ ]`). Never tick a gate without a receipt bound to a
  commit; human, VM and device gates are done only by people.
- `-rc.N` tags publish pre-releases (no gate check) whose notes say they are not a release.
- Run `make scan` before pushing; never publish `base/`, secrets or personal paths.

## Before you start

```bash
gh auth status
git status                                  # must be clean
git fetch origin --tags && git checkout main && git pull --ff-only
LAST=$(git describe --tags --abbrev=0 2>/dev/null || echo none); echo "Last tag: $LAST"
```

## Versioning

Semantic versions with a `v` prefix: `v0.1.0`, `v0.1.1` (fixes), `v0.2.0` (features),
`v0.1.0-rc.1` (pre-release). The tag must equal `[workspace.package] version` in `Cargo.toml`.

## Process

1. Review changes: `git log $LAST..HEAD --oneline`.
2. Bump the version if needed, refresh the lock in Docker, commit and push:

   ```bash
   scripts/cargo-docker.sh cargo update -w
   git commit -am "chore: release X.Y.Z" && git push origin main
   ```

3. Final versions: write `docs/release/vX.Y.Z.md` (sections below, limitations, rollback, the gate
   checklist with receipt links), confirm with the owner, commit and push.
4. Require green CI on that commit:

   ```bash
   gh run list --workflow ci.yml --commit "$(git rev-parse HEAD)" --json status,conclusion -q '.[0]'
   ```

5. Tag and push (triggers `release.yml`):

   ```bash
   git tag -a $NEXT -m "Release $NEXT" && git push origin $NEXT
   ```

6. Monitor and verify: `gh run list --workflow release.yml -L 3`, `gh release view $NEXT` (five
   assets), download and `sha256sum --check SHA256SUMS`.

## Release notes

Categorize commits in this order; omit empty sections: **✨ New Features**, **🔧 Improvements**,
**🔒 Security**, **🧹 Cleanup**, **📚 Documentation**, then **📦 Downloads** (ELF, AppImage, SBOM,
licences, `SHA256SUMS`) and **Full Changelog**: `https://github.com/jniltinho/rust-microsip/compare/vPREV...vNEXT`.
State known limitations honestly (no claim of full MicroSIP parity).
