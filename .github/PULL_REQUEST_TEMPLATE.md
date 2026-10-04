<!--
PR title: use a Conventional Commit, e.g. "feat(gui): remember window position" or "fix(audio): ...".
PRs are squash-merged, so the title becomes the commit message on main.
-->

## Summary

<!-- What does this change and why? -->

Closes #

## Type of change

- [ ] feat: new feature
- [ ] fix: bug fix
- [ ] perf: performance
- [ ] docs: documentation only
- [ ] build / ci: build system, installer, workflows
- [ ] refactor / test / chore

## Checklist

- [ ] PR title follows [Conventional Commits](https://www.conventionalcommits.org/)
- [ ] `cargo fmt --all -- --check` passes
- [ ] `cargo clippy --all-targets --locked -- -D warnings` passes
- [ ] `cargo test --locked` passes
- [ ] If audio code changed (`src/audio.rs`, `src/toggle.rs`, roles or device selection): tested on real devices with `cargo test -- --ignored` and/or `scripts/e2e.ps1` (devices used: ...)
- [ ] If the settings dialog changed: screenshot attached
- [ ] If the installer changed: MSI built with `installer/build-msi.ps1` and `wix msi validate` passes
- [ ] `CHANGELOG.md` `[Unreleased]` section updated (not needed for docs/ci/chore-only changes)
- [ ] New `unsafe` blocks have a `// SAFETY:` comment
