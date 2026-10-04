# Code signing

Toggle Audio's Windows releases are to be signed with Authenticode through [SignPath Foundation](https://signpath.org), which gives open-source projects a free code signing certificate used through [SignPath.io](https://signpath.io). This page holds the full code signing policy, how the release workflow signs, the maintainer's release procedure, the SignPath set-up it expects, and the application to SignPath Foundation. The research behind it is in [research/signpath.md](research/signpath.md).

## Contents

- [Status](#status)
- [Code signing policy](#code-signing-policy)
- [Verifying a signature](#verifying-a-signature)
- [How the release workflow signs](#how-the-release-workflow-signs)
- [Release procedure (maintainer)](#release-procedure-maintainer)
- [Dry run](#dry-run)
- [SignPath configuration](#signpath-configuration)
- [Secrets and variables](#secrets-and-variables)
- [Artifact configuration](#artifact-configuration)
- [Rotating the API token](#rotating-the-api-token)
- [When signing is off or fails](#when-signing-is-off-or-fails)
- [Applying to SignPath Foundation](#applying-to-signpath-foundation)
- [After the first signed release](#after-the-first-signed-release)
- [Placeholders on this page](#placeholders-on-this-page)
- [Sources](#sources)

## Status

| Item | State |
| --- | --- |
| Signing in the release workflow | Implemented. It is off until the SignPath secret and variables are set ([Secrets and variables](#secrets-and-variables)). |
| Application to SignPath Foundation | Not submitted yet. The project still lacks the "verifiable reputation" the terms ask for ([Applying](#applying-to-signpath-foundation)). |
| Signed releases | None so far. 0.1.0 is unsigned and will not be re-signed: its hashes are already published in `SHA256SUMS.txt`, in its build provenance attestation and in the winget manifest. Signing starts with the first release after onboarding. |

## Code signing policy

The README carries the short form of this policy, because SignPath Foundation requires it on the project's home page ([terms](https://signpath.org/terms.html), "Conditions for the website / repository"). Keep the two in sync.

Free code signing provided by [SignPath.io](https://about.signpath.io), certificate by [SignPath Foundation](https://signpath.org).

### Team roles

| Role | Members | Responsibility |
| --- | --- | --- |
| Committers and reviewers | [Han Lin (@hotdogee)](https://github.com/hotdogee) | Write and review every change, including CI configuration and build scripts. Contributions from others are merged only after review. |
| Approvers | [Han Lin (@hotdogee)](https://github.com/hotdogee) | Approve each release-signing request in SignPath after checking that it comes from the expected tag and workflow run. |

Every team member must use multi-factor authentication for GitHub and for SignPath.

### What is signed

- The MSI, `toggle-audio-<version>-x64.msi`.
- Both executables, `toggle-audio.exe` and `toggle-audiow.exe`. The same signed files are installed by the MSI and shipped in the portable zip.

Nothing else is signed: not CI builds of pull requests, not local builds, not the benchmark programs under `bench/`. Third-party binaries are never signed; the executables contain only this project's code and statically linked open-source crates built from source.

### How releases are built and signed

1. A release starts when the maintainer pushes a `vX.Y.Z` tag on a reviewed commit of [hotdogee/toggle-audio](https://github.com/hotdogee/toggle-audio).
2. The [release workflow](../.github/workflows/release.yml) builds the executables and the MSI in GitHub Actions, on a GitHub-hosted Windows runner, from a clean checkout with no build cache.
3. The workflow uploads the unsigned MSI as an artifact of the same run and submits it to SignPath. SignPath checks where it came from (the repository, the commit and the workflow run, as reported by GitHub) and checks the file metadata ([origin verification](https://docs.signpath.io/origin-verification/)).
4. An approver approves the request by hand in SignPath. Every release-signing request needs this approval.
5. SignPath signs the executables inside the MSI and then the MSI, with a timestamp.
6. The workflow checks every signature and publishes the signed files on [GitHub Releases](https://github.com/hotdogee/toggle-audio/releases).

Files are signed with the SignPath Foundation certificate only by tag-triggered runs of this workflow. Manual test runs of the workflow (the [dry run](#dry-run)) can only use SignPath's untrusted test certificate. The signing key never leaves SignPath's hardware security module, and nobody signs on a personal computer.

### Where signed files are published

Only on the [GitHub Releases](https://github.com/hotdogee/toggle-audio/releases) page of this repository. The winget package `Hotdogee.ToggleAudio` downloads the MSI from there. Every release page says whether that release is signed and links to this policy.

### Privacy policy

This program will not transfer any information to other networked systems unless specifically requested by the user or the person installing or operating it.

In fact Toggle Audio makes no network connections at all, not even on request: it has no network code, no telemetry and no update check. It reads and writes only its own settings file, `%APPDATA%\toggle-audio\config.json`, and writes it only when you save in the settings dialog. The MSI changes only what [docs/packaging.md](packaging.md#what-the-msi-installs) lists.

### Misuse and incidents

Report a signed file that misbehaves, that is not listed in a release's `SHA256SUMS.txt`, or that was not published on this repository's Releases page through the private process in [SECURITY.md](../SECURITY.md). If a release was signed in error or a build was compromised, the maintainer removes the release, publishes an advisory and informs SignPath Foundation.

## Verifying a signature

PowerShell, no extra tools (replace the file name with the one you downloaded):

```powershell
Get-AuthenticodeSignature .\toggle-audio-0.2.0-x64.msi |
  Format-List Status, StatusMessage, SignerCertificate, TimeStamperCertificate
```

With the Windows SDK:

```powershell
signtool verify /pa /v .\toggle-audio-0.2.0-x64.msi
```

Explorer shows the same under **Properties > Digital Signatures**. The same checks work for `toggle-audio.exe` and `toggle-audiow.exe`.

| Field | Expected for a signed release |
| --- | --- |
| Status | `Valid` (`signtool`: "Successfully verified") |
| Signer | `CN=SignPath Foundation, O=SignPath Foundation, L=Lewes, S=Delaware, C=US` |
| Issuer | A GlobalSign code signing CA (observed on other SignPath Foundation projects: `GlobalSign GCC R45 CodeSigning CA 2020`) |
| Digest | SHA-256 |
| Timestamp | Present (RFC 3161). Timestamped signatures stay valid after the certificate expires. |

A release whose notes say "Unsigned release" shows `Status : NotSigned`; check those files against `SHA256SUMS.txt` instead. The three checks complement each other:

- **The signature** shows that SignPath Foundation signed a build of this repository that the maintainer approved.
- **`SHA256SUMS.txt`** pins the exact files of a release.
- **The build provenance attestation** (`gh attestation verify <file> --repo hotdogee/toggle-audio`) ties each file to the workflow run that built it. For signed releases it covers the signed files.

**What users see.** Windows names the publisher **SignPath Foundation**, not Han Lin, in the UAC prompt and in SmartScreen. A signature does not remove the SmartScreen warning at once: a new file is "flagged as unrecognized until reputation accumulates", and reputation builds over releases signed with the same certificate ([SmartScreen reputation](https://learn.microsoft.com/en-us/windows/apps/package-and-deploy/smartscreen-reputation)). On Windows 11 with Smart App Control on, unsigned files are blocked outright unless they already have a good reputation, so signing is what lets Toggle Audio run there ([code signing options](https://learn.microsoft.com/en-us/windows/apps/package-and-deploy/code-signing-options)).

## How the release workflow signs

All signing steps live in [`.github/workflows/release.yml`](../.github/workflows/release.yml). The workflow has two jobs: **build** (Windows; builds, signs, verifies and stages) and **publish** (Ubuntu, tag pushes only; attests and publishes what build uploaded).

| Step | What it does |
| --- | --- |
| Check code signing configuration | Decides whether this run signs (table below) and exports `enabled`, `policy` and `bin-dir` for the later steps. It runs before the build, so a half-finished configuration fails within seconds. Secrets cannot be tested in an `if:` expression, which is why this is a step. |
| cargo test, cargo build, Build MSI | Unchanged. The MSI is built from the **unsigned** executables. |
| Upload unsigned MSI for signing | `actions/upload-artifact@v7` stores `installer/out/toggle-audio-<version>-x64.msi` as the artifact `signpath-unsigned-msi` (kept 1 day, `overwrite: true` so a re-run of the job can upload it again) and returns its `artifact-id`. SignPath signs only artifacts of the same run. |
| Submit signing request to SignPath | `signpath/github-action-submit-signing-request` v3.0, pinned to its commit SHA, with `wait-for-completion: true` and a 3600 s timeout (the default is 600 s, too short for a manual approval). The signed artifact is unpacked into `signed/`. |
| Use the signed MSI and extract its signed executables | Replaces the MSI in `installer/out/` with the signed one (and deletes the `.sha256` file that `build-msi.ps1` wrote for the unsigned MSI) and extracts both signed executables with `msiexec /a` (an administrative install only unpacks files) into `signed-bin/`. |
| Verify Authenticode signatures | Checks the MSI and both executables with `Get-AuthenticodeSignature`: status `Valid`, a signer subject that starts exactly with `CN=SignPath Foundation, O=SignPath Foundation, `, a timestamp, and the same signer certificate (thumbprint) on all three files. It also checks that the repacked MSI kept its ProductVersion and UpgradeCode. Any failure stops the release before hashes are written. |
| Stage release assets | Unchanged, except that it takes the executables from `bin-dir` (`target/release` or `signed-bin`). It runs `wix msi validate` on the (signed) MSI, builds the portable zip and writes `SHA256SUMS.txt` last. |
| Extract release notes | Adds "**Signed with SignPath Foundation certificate.**" or "**Unsigned release.**" and a link to the code signing policy to every release page. |
| Upload release assets, Upload inputs of the publish job | Store `dist/` as `release-assets` (also the result of a dry run) and, on tag pushes, the shipped executables and the release notes as `release-publish` (kept 1 day). Both use `overwrite: true`. |
| publish: attest-build-provenance | Runs in the publish job on the downloaded files. The executable subjects are the shipped ones, so the attestation covers the signed files. |
| publish: action-gh-release | Creates the GitHub Release with the MSI, the zip and `SHA256SUMS.txt`. |

**One request, deep signing.** SignPath signs the executables inside the MSI and then the MSI in one request ([syntax](https://docs.signpath.io/artifact-configuration/syntax): "SignPath supports signing these files and their contents in a single step"). The workflow then takes the signed executables out of the signed MSI for the zip. So each release needs one approval and three signatures, and the executables in the MSI, the zip and the attestation are byte-identical. Signing the loose executables in the same request would sign them twice, with different bytes in the zip and in the MSI; signing them first and building the MSI afterwards would need two requests and two approvals.

**Permissions.** The workflow grants nothing by default (`permissions: {}`). The build job gets only `contents: read` and `actions: read`, the two permissions SignPath's GitHub integration asks for ([GitHub integration](https://docs.signpath.io/trusted-build-systems/github)). The SignPath action passes the job token (its `github-token` input defaults to it) to SignPath, so SignPath only ever receives this read-only token. The publish job, which runs only on tag pushes and runs no build tools and no SignPath code, holds `contents: write`, `id-token: write` and `attestations: write` to attest and publish. Third-party actions are pinned to commit SHAs, because they run next to the SignPath API token or the write token.

**When it signs.** The repository variable `SIGNPATH_SIGNING_POLICY_SLUG` is the on/off switch for tag releases:

| Configuration | Tag push (`vX.Y.Z`) | Manual run (dry run) |
| --- | --- | --- |
| Nothing set (today) | Unsigned release, as before. The release notes say "Unsigned release". | Unsigned build. Nothing is published. |
| Token, organization ID and project slug set; `SIGNPATH_SIGNING_POLICY_SLUG` empty | Unsigned release. | Signed with `test-signing`. Nothing is published. |
| All four set | Signed with `SIGNPATH_SIGNING_POLICY_SLUG` (`release-signing`). Published only if every signature is valid. | Signed with `test-signing`. Nothing is published. |
| `SIGNPATH_SIGNING_POLICY_SLUG` set, something else missing | Fails at "Check code signing configuration". Nothing is built or published. | Same. |

While signing is on, a release can never go out unsigned by accident: a denied, failed or timed-out request fails the job. A manual run can never sign with the release policy: the `signing-policy-slug` input offers only `test-signing`, and "Check code signing configuration" fails a manual run whose policy is `release-signing` or equals `SIGNPATH_SIGNING_POLICY_SLUG`. So files signed with the SignPath Foundation certificate come only from tag pushes.

## Release procedure (maintainer)

With signing on, a release goes like this:

1. **Prepare** as in [CONTRIBUTING.md](../CONTRIBUTING.md#releases-maintainer-notes): move the `[Unreleased]` entries in `CHANGELOG.md` into the new version, bump `Cargo.toml` and `assets/app.manifest`, commit, push `main` and wait for CI.
2. **Tag:**

   ```powershell
   git tag -a v0.2.0 -m "Toggle Audio 0.2.0"
   git push origin v0.2.0
   ```

3. **The workflow builds.** It runs the tests, builds both executables and the MSI, uploads the MSI and submits the signing request. The log of "Submit signing request to SignPath" links the request.
4. **The signing request appears in SignPath** ([app.signpath.io](https://app.signpath.io)), and every approver gets an email.
5. **Check, then approve.** On the request page check:
   - project `toggle-audio` and signing policy `release-signing`;
   - the origin: repository `https://github.com/hotdogee/toggle-audio`, the tag and commit you just pushed, and a build URL that points to this workflow run;
   - the artifact: `toggle-audio-0.2.0-x64.msi`, built from the version you are releasing.

   Then choose **Approve**. If anything is unexpected, choose **Deny**: the job fails and nothing is published. Approve within an hour, or the job times out.
6. **The workflow continues.** It downloads the signed MSI, extracts the signed executables, verifies all three signatures and the MSI identity, builds the zip, writes `SHA256SUMS.txt`, attests and publishes the GitHub Release with "Signed with SignPath Foundation certificate" in the notes. The `winget` workflow then submits the release to winget-pkgs.
7. **Check the release.** Download the MSI and [verify the signature](#verifying-a-signature). After the first signed release, also run a real install, an upgrade from the previous version and an uninstall ([Testing a real install](packaging.md#testing-a-real-install)), because SignPath repacks the MSI.

If the approval timed out or SignPath was unavailable, re-run the failed **Build and sign** job in GitHub Actions; it builds again and creates a new signing request (both artifact uploads use `overwrite: true`, so the re-run can replace them). This works as long as no pipeline policy forbids signing re-runs, and only a few times: "SignPath currently allows policy evaluation for up to 3 re-runs of a build" ([GitHub integration](https://docs.signpath.io/trusted-build-systems/github)). After that, or if only the **Attest and publish** job failed and re-running it does not help, release a new patch version.

## Dry run

A manual run of the Release workflow builds and signs but never attests or publishes. Use it during onboarding, after changing `release.yml`, the WiX source or the artifact configuration, and before the first signed release.

1. On GitHub, open **Actions > Release > Run workflow** and choose the branch (`main`). The signing policy input offers only `test-signing`. With the GitHub CLI: `gh workflow run release.yml --ref main -f signing-policy-slug=test-signing`.
2. Approve the request in SignPath if the policy requires an approval.
3. Read the log of "Verify Authenticode signatures". In a dry run, a signature that is not trusted (the test certificate is self-signed) or has an unexpected signer is only a warning; a missing signature still fails.
4. Download the `release-assets` artifact of the run. It holds the MSI, the zip and `SHA256SUMS.txt` exactly as a release would. Install, upgrade and uninstall that MSI on a test machine.

The dry run signs as soon as the token, organization ID and project slug exist, even while `SIGNPATH_SIGNING_POLICY_SLUG` is still empty, so tag releases stay unsigned until the dry run has passed. A dry run always uses `test-signing`. The workflow refuses the release policy in a dry run, because the `release-assets` artifact of a run can be downloaded by anyone signed in to GitHub for as long as it is kept, and trusted signed files must only appear on the Releases page. To test `release-signing` itself, release a new patch version.

The run's ref is a branch, not a tag. SignPath checks it against the signing policy's allowed branch names ([origin verification](https://docs.signpath.io/origin-verification/)), so `test-signing` must allow `main`.

## SignPath configuration

What the workflow expects in SignPath. Whether SignPath Foundation or the maintainer creates each item is not documented; ask during onboarding.

| Item | Expected value | Notes |
| --- | --- | --- |
| Organization | Created by SignPath for the Foundation subscription. ID: `<fill after approval>` | Shown when you click the organization name at the upper right of the web app. Stored as `SIGNPATH_ORGANIZATION_ID`. |
| Interactive user | Han Lin, `hotdogee@gmail.com` | Sign in with a Google or Microsoft account that has 2-step verification ([users](https://docs.signpath.io/users)). |
| Trusted build system | The predefined **GitHub.com** trusted build system, added to the organization and linked to the project | [GitHub integration](https://docs.signpath.io/trusted-build-systems/github). |
| Project | Slug `toggle-audio`; repository URL `https://github.com/hotdogee/toggle-audio` | Origin verification compares the repository URL. Slug stored as `SIGNPATH_PROJECT_SLUG` ([projects](https://docs.signpath.io/projects)). |
| Artifact configuration | The project's **default** configuration, with the content of [`installer/signpath/artifact-configuration.xml`](../installer/signpath/artifact-configuration.xml). Suggested name: `msi-deep-sign` | The workflow does not pass `artifact-configuration-slug`, so the default is used. See [installer/signpath/README.md](../installer/signpath/README.md). |
| Signing policy `test-signing` | Test certificate. Submitter: the CI user. Allowed branches must include `main` for dry runs. | For dry runs and onboarding. |
| Signing policy `release-signing` | The SignPath Foundation certificate. Submitter: the CI user. Approver: Han Lin, 1 required approval. Trusted build system verification and origin verification on (required for open source). Allowed branch names must match the release tags. | Stored as `SIGNPATH_SIGNING_POLICY_SLUG`. How tag refs (`refs/tags/v*`) are matched against "allowed branch names" is not documented; ask during onboarding. |
| CI user | For example `toggle-audio-github-actions`; Submitter on both signing policies | Its API token is stored as `SIGNPATH_API_TOKEN`. "API tokens are only displayed when generated" ([users](https://docs.signpath.io/users)). |

Malware scanning always runs for open-source projects and cannot be turned off ([projects](https://docs.signpath.io/projects)). Do not enable the pipeline policy that prevents signing builds from re-runs, or a timed-out approval can only be fixed with a new release ([pipeline policies](https://docs.signpath.io/pipeline-policies/), [GitHub integration](https://docs.signpath.io/trusted-build-systems/github)).

## Secrets and variables

Create these in the repository (**Settings > Secrets and variables > Actions**) after SignPath approval:

| Name | Kind | Value | Used by |
| --- | --- | --- | --- |
| `SIGNPATH_API_TOKEN` | Secret | API token of the SignPath CI user | `api-token` of the SignPath action |
| `SIGNPATH_ORGANIZATION_ID` | Variable | SignPath organization ID, `<fill after approval>` (not secret) | `organization-id` |
| `SIGNPATH_PROJECT_SLUG` | Variable | `toggle-audio` | `project-slug` |
| `SIGNPATH_SIGNING_POLICY_SLUG` | Variable | `release-signing`. **The on/off switch** for signing tag releases. | `signing-policy-slug` for tag pushes |

Recommended order, with the GitHub CLI (each `gh secret set` prompts for the value, so the token never lands in the shell history):

```powershell
gh secret set SIGNPATH_API_TOKEN --repo hotdogee/toggle-audio
gh variable set SIGNPATH_ORGANIZATION_ID --repo hotdogee/toggle-audio --body "<fill after approval>"
gh variable set SIGNPATH_PROJECT_SLUG --repo hotdogee/toggle-audio --body "toggle-audio"

# Dry run with test-signing and check the result (see "Dry run"), then switch tag releases on:
gh variable set SIGNPATH_SIGNING_POLICY_SLUG --repo hotdogee/toggle-audio --body "release-signing"
```

To switch signing off again, delete `SIGNPATH_SIGNING_POLICY_SLUG` (`gh variable delete SIGNPATH_SIGNING_POLICY_SLUG --repo hotdogee/toggle-audio`). The next tag is then released unsigned.

Only `release.yml` reads these values. Pull request workflows from forks never receive repository secrets.

## Artifact configuration

[`installer/signpath/artifact-configuration.xml`](../installer/signpath/artifact-configuration.xml) is the reviewed copy of the configuration stored in SignPath. In short:

- root `<zip-file>`, because `actions/upload-artifact` zips the artifact ([GitHub integration](https://docs.signpath.io/trusted-build-systems/github));
- `<msi-file path="toggle-audio-${version}-x64.msi">`, the file name `release.yml` uploads, with the `subject` and `author` restrictions;
- inside it, `<directory path="PFiles64/Toggle Audio">` (the administrative-install layout of `ProgramFiles64Folder\Toggle Audio`) with a `<pe-file-set>` for both executables, restricted by product name, product version, company name and original file name, each signed with `<authenticode-sign>`;
- then `<authenticode-sign>` for the MSI itself.

The `version` parameter comes from the workflow (`parameters: version: "<x.y.z>"`). SHA-256 is the default digest ([reference](https://docs.signpath.io/artifact-configuration/reference)) and SignPath always adds a timestamp. Whether SignPath compares `product-version` with the version string `0.2.0` or the binary version `0.2.0.0` is not documented; the first dry run shows it. If it fails on that restriction, ask SignPath support.

How to upload it and when to change it: [installer/signpath/README.md](../installer/signpath/README.md).

## Rotating the API token

Rotate the token if it may have leaked, when someone loses access, and otherwise about once a year.

1. In SignPath, open **Users**, select the CI user and generate a new API token. Copy it at once; SignPath shows it only once ([users](https://docs.signpath.io/users)).
2. Replace the secret: `gh secret set SIGNPATH_API_TOKEN --repo hotdogee/toggle-audio`.
3. Revoke the old token in SignPath if generating a new one did not already invalidate it.
4. Start a [dry run](#dry-run) to confirm the new token works.

A leaked token lets someone submit signing requests, but origin verification ties release-signing requests to workflow runs of this repository and each one still needs your approval. Rotate anyway, and deny any request you did not expect.

## When signing is off or fails

| Situation | Result |
| --- | --- |
| No SignPath secret or variables (today) | Unsigned release, exactly as before; the log has a notice, and the release notes say "Unsigned release". |
| `SIGNPATH_SIGNING_POLICY_SLUG` set, but the token, organization ID or project slug missing | The job fails at "Check code signing configuration" before building. Nothing is published. |
| Token wrong, expired or revoked | "Submit signing request to SignPath" fails. Fix the secret and re-run the job. |
| Request denied, or not approved within an hour | The job fails and nothing is published. Re-run the job to get a new request (SignPath evaluates at most 3 re-runs of a build). |
| SignPath rejects the request (origin verification, a metadata restriction, the malware scan) | The job fails; the reason is on the request page in SignPath. |
| A signature is not valid, not from SignPath Foundation, has no timestamp or comes from a different certificate than the other two files, or the repacked MSI changed its ProductVersion or UpgradeCode | "Verify Authenticode signatures" fails before any hash is written. Nothing is published. |
| Attestation fails | Non-blocking, as before. |

To publish one release unsigned while signing is set up, delete `SIGNPATH_SIGNING_POLICY_SLUG` before pushing the tag. Do not do this to work around a rejected request without finding out why it was rejected.

## Applying to SignPath Foundation

The form is at [signpath.org/apply](https://signpath.org/apply.html). The terms are at [signpath.org/terms](https://signpath.org/terms.html).

### Eligibility checklist

| Requirement | Toggle Audio |
| --- | --- |
| No malware, no potentially unwanted programs | Met: single-purpose audio switcher, no background process, no network access. |
| OSI-approved license for all components, no commercial dual licensing | Met: MIT; the statically linked crates are MIT or offer MIT ([THIRD-PARTY-NOTICES.txt](../THIRD-PARTY-NOTICES.txt)). |
| No proprietary code except system libraries | Met: only the statically linked MSVC C runtime, a compiler system library. |
| Actively maintained | Met: CI, Dependabot, changelog. Keep commits and releases coming. |
| Already released in the form to be signed | Met: MSI and zip of 0.1.0 on GitHub Releases. |
| Functionality described on the download page | Met: README and release notes. |
| **Verifiable reputation** for downloadable executables (required form field) | **Gap.** Apply once there is checkable evidence: the winget package merged, download counts, stars, posts or discussions that mention the tool. |
| The team owns the repository and maintains all sources and build scripts | Met: hotdogee owns the repository. |
| No hacking tools | Met. |
| Privacy: data collection described and optional | Met: no data collection; the policy states it. |
| System changes announced; uninstall provided | Met: MSI with an optional PATH entry in Custom Setup and a clean uninstall; the zip is removed by deleting its folder. |
| Multi-factor authentication for SignPath and GitHub | **Check yourself**: GitHub 2FA at [github.com/settings/security](https://github.com/settings/security); a Google or Microsoft account with 2-step verification for SignPath. |
| Team roles defined | Met: Han Lin in every role ([policy](#team-roles)). |
| Code signing policy on the home page and on the download/release pages | Met by this change: README section "Code signing policy" and a link in every release's notes. The current release page (0.1.0) predates it; the next release has it, or add the line to the 0.1.0 notes by hand before applying. |
| Product name and version set and enforced by metadata restrictions | Met: ProductName `Toggle Audio` and ProductVersion in both executables, Subject and Author in the MSI, enforced in the artifact configuration. |
| Verifiable CI build, manual approval per release | Met: tag-triggered GitHub Actions on GitHub-hosted runners, no cache, `--locked`; approval step in the release procedure. |

### Form answers

Fields as they appeared on the form in October 2026; check them again when applying.

| Field | Answer |
| --- | --- |
| Project Name | `Toggle Audio (toggle-audio)` |
| Repository URL | `https://github.com/hotdogee/toggle-audio` |
| Homepage URL | `https://github.com/hotdogee/toggle-audio` |
| Download URL | `https://github.com/hotdogee/toggle-audio/releases` (the page must mention SignPath Foundation; the release-notes line does) |
| Privacy Policy URL | `https://github.com/hotdogee/toggle-audio#code-signing-policy` |
| Wikipedia URL | (blank) |
| Tagline | `Flip Windows audio output between two devices in milliseconds: one tiny exe, perfect for a hotkey.` |
| Description | See below. |
| Reputation | See below. Fill in real, checkable facts on the day you apply. |
| Maintainer Type | `Individual maintainer(s)` |
| Build System | `GitHub Actions` |
| First Name / Last Name | `Han` / `Lin` |
| Email | `hotdogee@gmail.com` |
| Company Name | (blank) |
| Primary Discovery Channel | Answer truthfully (for example, Microsoft Learn's [code signing options](https://learn.microsoft.com/en-us/windows/apps/package-and-deploy/code-signing-options) page links SignPath Foundation). |
| Consent checkbox | Tick after reading the Code of Conduct in the [terms](https://signpath.org/terms.html). |

**Description:**

> Toggle Audio is a small open-source Windows utility that switches the default playback device between two outputs the user picked (for example speakers and headphones) each time it runs, then exits. It is a native x64 executable written in Rust with no runtime, no background process and no network access, designed to be bound to a macro key in Logitech G HUB, Stream Deck, AutoHotkey or a Windows shortcut. A settings dialog picks the two devices; an MSI installer and a portable zip are provided.

**Reputation** (keep only what is true on the day you apply):

> Released 2026-10-04 (v0.1.0) on GitHub Releases with SHA-256 sums and GitHub build provenance attestations. Available through winget as `Hotdogee.ToggleAudio` (microsoft/winget-pkgs pull request #446595, merged on `<fill when applying>`). Downloads: `<fill when applying>` (GitHub Releases API). GitHub stars: `<fill when applying>`. Mentions and discussions: `<fill when applying>`.

**Build process** (if SignPath asks):

> Releases are built only by `.github/workflows/release.yml` on GitHub-hosted `windows-latest` runners when a `vX.Y.Z` tag is pushed. The job checks that the tag matches Cargo.toml, runs `cargo test --locked` and `cargo build --release --locked` (static C runtime, no build cache) and builds the MSI with WiX Toolset 7 (`installer/build-msi.ps1`). It submits the unsigned MSI to SignPath through `signpath/github-action-submit-signing-request` (pinned by commit SHA, in a job with a read-only GitHub token), extracts the signed executables from the signed MSI for the portable zip, verifies all signatures and writes SHA256SUMS.txt. A separate job then writes GitHub build provenance attestations and publishes the GitHub Release. Manual runs of the workflow can only use the test-signing policy and never publish.

### After approval

1. Accept the SignPath organization invitation (it expires after 14 days) and sign in with an MFA-protected account.
2. Check or create the items in [SignPath configuration](#signpath-configuration), and paste the artifact configuration.
3. Ask SignPath what is still open: how tag refs match "allowed branch names", the yearly quotas of a Foundation subscription (signatures and artifact size; each release uses 3 signatures and less than 1 MB), and who creates the project and policies.
4. Add `SIGNPATH_API_TOKEN`, `SIGNPATH_ORGANIZATION_ID` and `SIGNPATH_PROJECT_SLUG` ([Secrets and variables](#secrets-and-variables)).
5. Start a [dry run](#dry-run) with `test-signing`, fix what it reports, and test the MSI from it.
6. Set `SIGNPATH_SIGNING_POLICY_SLUG` to `release-signing`. The next tag is signed.

## After the first signed release

Update the pages that still say releases are unsigned:

- README: the note in Install, the "Not in effect yet" note under Code signing policy and the SmartScreen row in Troubleshooting.
- [packaging.md](packaging.md): "Installing" and "Code signing".
- [SECURITY.md](../SECURITY.md): "Verifying releases" and the out-of-scope line.
- This page: [Status](#status).

## Placeholders on this page

Two kinds of markers are left on purpose:

| Marker | Where | Replace with |
| --- | --- | --- |
| `<fill after approval>` | [SignPath configuration](#signpath-configuration) and [Secrets and variables](#secrets-and-variables) | The SignPath organization ID. Put it in the `SIGNPATH_ORGANIZATION_ID` variable; it can also be written here, since it is not secret. |
| `<fill when applying>` | The Reputation answer in [Form answers](#form-answers) | Real numbers and links on the day you apply. |

## Sources

- SignPath Foundation: [home](https://signpath.org), [terms and Code of Conduct](https://signpath.org/terms.html), [application form](https://signpath.org/apply.html)
- SignPath documentation: [setting up projects](https://docs.signpath.io/projects), [users and API tokens](https://docs.signpath.io/users), [artifact configuration](https://docs.signpath.io/artifact-configuration/), [syntax](https://docs.signpath.io/artifact-configuration/syntax), [reference](https://docs.signpath.io/artifact-configuration/reference), [examples](https://docs.signpath.io/artifact-configuration/examples), [GitHub integration](https://docs.signpath.io/trusted-build-systems/github), [origin verification](https://docs.signpath.io/origin-verification/), [pipeline policies](https://docs.signpath.io/pipeline-policies/)
- GitHub Action: [SignPath/github-action-submit-signing-request](https://github.com/SignPath/github-action-submit-signing-request)
- Microsoft Learn: [code signing options](https://learn.microsoft.com/en-us/windows/apps/package-and-deploy/code-signing-options), [SmartScreen reputation](https://learn.microsoft.com/en-us/windows/apps/package-and-deploy/smartscreen-reputation), [SignTool](https://learn.microsoft.com/en-us/windows/win32/seccrypto/signtool)
- Research and reasoning: [research/signpath.md](research/signpath.md)
