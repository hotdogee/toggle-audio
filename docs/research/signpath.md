# Code signing through SignPath Foundation: requirements, application and pipeline design

Researched on 2026-10-04 from primary sources: the SignPath Foundation site (signpath.org), the SignPath documentation (docs.signpath.io, which is where `about.signpath.io/documentation/` redirects), the `signpath/github-action-submit-signing-request` repository, and the release workflows of open-source projects that already sign through SignPath Foundation. Microsoft Learn is cited for SmartScreen behavior. Anything marked **(unconfirmed)** could not be verified from a primary source and should be checked during onboarding.

Earlier context: [packaging.md section 7](packaging.md#7-code-signing-and-smartscreen) chose SignPath Foundation as the realistic free route. This document turns that choice into a checklist, an application draft and a pipeline design.

---

## TL;DR

- **What you get:** an OV code signing certificate issued to **"SignPath Foundation"** (not to Han Lin), with its key held in SignPath's HSM, used through a free SignPath.io "Open Source Code Signing" subscription. Each project gets **its own** certificate with the same subject; the binaries of three projects checked on 2026-10-04 had three different thumbprints (see [section 7](#7-what-users-see-and-how-they-verify-a-signature)).
- **Eligibility:** the project meets every technical and policy condition, or can meet it with documentation changes only. The **real gap is reputation.** The repository was created today, has 0 stars and a handful of downloads. The terms say that "for executable programs that may be downloaded and executed based on our signature, we require a certain verifiable reputation", and the application form has a required **Reputation** field. Expect a deferral or a rejection if you apply right now.
- **Documentation work (no code):** add a "Code signing policy" section to the README and link it from every release page. The section needs the exact attribution sentence, the team roles (Han Lin in every role) and the privacy sentence. The wording is in [section 3](#3-required-wording-the-code-signing-policy).
- **Pipeline decision:** send **one signing request per release that contains only the MSI**, and have SignPath **deep-sign** it (both exes inside it first, then the MSI). Then extract the signed exes from the signed MSI with `msiexec /a` and build the portable zip from them. The result is one manual approval per release, three signatures, and exes that are byte-identical in the MSI, the zip and the attestation. The XML is in [section 5.3](#53-artifact-configuration-xml-chosen-design) and the workflow changes are in [section 5.4](#54-workflow-changes-releaseyml).
- **Every release-signing request needs a manual approval** in the SignPath web app ([terms](https://signpath.org/terms.html): "Every release needs manual approval for signing."). The action waits 600 s by default, so the design raises that to 3600 s.
- **SmartScreen:** signing does **not** remove the first-run warning immediately. The warning shows the verified publisher "SignPath Foundation" instead of "Unknown publisher", and reputation then builds up over releases signed with the same certificate. On Windows 11, Smart App Control blocks unsigned files outright, which is a strong reason to sign anyway.
- **Do not re-sign v0.1.0.** Its hashes are already published in `SHA256SUMS.txt`, the attestation and winget PR #446595. Signing starts with the next version.

---

## 1. What SignPath Foundation provides

From [signpath.org](https://signpath.org/) and the [terms](https://signpath.org/terms.html):

- A code signing certificate "issued to SignPath Foundation. This means that SignPath Foundation is the publisher of the OSS project."
- The private key is "securely generated and stored on our Hardware Security Module (HSM)", and signing runs through SignPath.io, which is free for OSS projects.
- What a signature promises: "For each release, SignPath.io verifies the origin of signed files. A signature confirms that the binary is a valid, automated build resulting from the source code at the noted source code repository." Note that "source code includes build scripts and CI configurations in the repository", so reviews must cover them.
- SignPath Foundation is operated by SignPath GmbH ([about](https://signpath.org/about)).
- The Code of Conduct is marked **"Draft"** on the terms page. Its last change in the [site repository](https://github.com/SignPath/fdn-website) was on 2025-02-14.

Observed on a released SignPath Foundation binary (`btm.exe` from bottom 0.14.9, checked locally with `signtool verify /pa /v` and `Get-AuthenticodeSignature`):

| Field | Value |
| --- | --- |
| Signer subject | `CN=SignPath Foundation, O=SignPath Foundation, L=Lewes, S=Delaware, C=US` |
| Issuer | `CN=GlobalSign GCC R45 CodeSigning CA 2020, O=GlobalSign nv-sa, C=BE` (root: GlobalSign Code Signing Root R45) |
| File digest | SHA-256 |
| Timestamp | RFC 3161, `DigiCert SHA256 RSA4096 Timestamp Responder 2025 1` |
| Certificate validity | 2026-01-28 to 2027-09-08. Timestamped signatures stay valid after the certificate expires. |

---

## 2. Eligibility checklist for Toggle Audio

Sources: [terms](https://signpath.org/terms.html) (conditions for free OSS subscriptions and for Foundation certificates) and the [application form](https://signpath.org/apply.html).

| Requirement (source wording, shortened) | Toggle Audio today | Status / action |
| --- | --- | --- |
| **No malware** / no potentially unwanted programs | Single-purpose audio switcher, no background process, no network | Met |
| **OSS license:** "OSI-approved Open Source license without commercial dual-licensing for all components" | MIT; the statically linked crates are MIT or offer MIT as an option (`THIRD-PARTY-NOTICES.txt`) | Met |
| **No proprietary code**, except System Libraries (GPL v3 section 1 definition) | The static MSVC C runtime counts as a compiler System Library | Met (low risk) |
| **Maintained:** "actively maintained" | First release today; CI, Dependabot, CHANGELOG | Met; keep commits and releases flowing |
| **Released:** "already released in the form that should be signed" | v0.1.0 MSI and zip on GitHub Releases | Met |
| **Documented:** functionality "described on its download page" | README plus release notes taken from CHANGELOG.md | Met |
| **Reputation:** "we require a certain verifiable reputation" for downloadable executables; required form field | Repo created 2026-10-04, 0 stars, about 6 downloads in total, winget PR #446595 still open | **Gap.** Apply once there is evidence: the winget PR merged, stars, download counts, posts or discussions that mention the tool. |
| **Own project / own binaries:** the signing team owns the repository and maintains all sources and build scripts | hotdogee owns the repo; the only binaries are the project's own | Met |
| **No hacking tools** | Not applicable | Met |
| **Privacy:** data collection must be described, shown during install and be optional | No data collection and no network access at all | Met; state it with the required sentence ([section 3](#3-required-wording-the-code-signing-policy)) |
| **Announce system changes** | The MSI adds an optional PATH entry (Custom Setup feature, documented); switching the default device is the requested function | Met |
| **Provide uninstallation** | MSI uninstall; the zip is removed by deleting the folder (README) | Met |
| **MFA:** "All team members must use multi-factor authentication for both SignPath and source code repository access" | GitHub 2FA could not be read through the API (the `gh` token lacks the scope). **(unconfirmed)** | Confirm 2FA at github.com/settings/security. Sign in to SignPath with a Google or Microsoft account that has 2-step verification ([users doc](https://docs.signpath.io/users)). |
| **Roles:** Authors/committers, reviewers, approvers | One maintainer (CODEOWNERS `* @hotdogee`) | Met: Han Lin holds all three roles (LocalSend publishes the same one-person setup; see [section 8](#8-reference-projects)) |
| **Code signing policy on the home page and download/release pages** | Not present | **Gap.** Add the README section and a link in every release ([section 3](#3-required-wording-the-code-signing-policy)) |
| **Metadata:** product name and product version attributes "set and enforced using file metadata restrictions" | Both exes: ProductName `Toggle Audio`, ProductVersion `0.1.0` (= Cargo/tag version), CompanyName `Han Lin`. MSI: Subject `Toggle Audio 0.1.0 installer`, Author `Han Lin` | Met; enforced in the artifact configuration ([section 5.3](#53-artifact-configuration-xml-chosen-design)) |
| **Verifiable build / manual approval** ("Don't fight the system") | Tag-triggered GitHub Actions on `windows-latest`, no build cache, `--locked` | Met; keep the release job free of caches and self-hosted runners |
| Upstream binaries | None (everything is statically linked from source) | Not applicable |

Optional hardening that SignPath can check through [pipeline policies](https://docs.signpath.io/pipeline-policies/) (not required by the terms):

- A branch ruleset on `main` that blocks force pushes and deletion.
- A tag ruleset on `v*` that restricts who can create tags.

These are cheap for a one-person project.

---

## 3. Required wording: the code signing policy

The terms ("Conditions for the website / repository") require:

1. "A code signing policy must be specified on the project's home page."
2. "Use the term 'Code signing policy' on your project's home page and download/release pages (section header or link to a dedicated page)."
3. The policy must include:
   - "Free code signing provided by SignPath.io, certificate by SignPath Foundation"
   - "Team roles and their members (see above, may include references to the project's permission groups)"
   - Privacy policy: "Link to your privacy policy or specify 'This program will not transfer any information to other networked systems unless specifically requested by the user or the person installing or operating it'."

The application form adds this about the **Download URL**: "This page must mention that the project uses the SignPath Foundation for code signing."

Where it goes for this project:

- **Home page** = the repository README (the form says the homepage "can be a dedicated website or the repository page"). Add a `## Code signing policy` section and a Contents entry.
- **Download/release page** = GitHub Releases. Every release body needs a line that links to that section. The release workflow can add it after the CHANGELOG extract.

Draft README section. The roles follow the terms' own terminology. The privacy sentence is quoted exactly, and the last sentence holds for Toggle Audio because it never touches the network:

```markdown
## Code signing policy

Free code signing provided by [SignPath.io](https://about.signpath.io/), certificate by [SignPath Foundation](https://signpath.org/).

Windows releases (the MSI and the executables in it and in the portable zip) are built by the [release workflow](.github/workflows/release.yml) on GitHub-hosted runners from a tagged commit of this repository, and every signing request is approved by hand.

Team roles:

- Committers and reviewers: [Han Lin (@hotdogee)](https://github.com/hotdogee)
- Approvers: [Han Lin (@hotdogee)](https://github.com/hotdogee)

Privacy policy: This program will not transfer any information to other networked systems unless specifically requested by the user or the person installing or operating it. (Toggle Audio makes no network connections at all; its only file is `%APPDATA%\toggle-audio\config.json`.)
```

Release-notes line (append it in the "Extract release notes" step so every release has it):

```markdown
Code signing policy: https://github.com/hotdogee/toggle-audio#code-signing-policy
```

Also, once the first signed release ships:

- Replace the "not code-signed yet" note in the README Install section.
- Update the SmartScreen row in Troubleshooting and the "Why the MSI is unsigned" part of `docs/packaging.md`.
- Update the out-of-scope line in `SECURITY.md`.

---

## 4. The application

### 4.1 Form fields and draft answers

The form on [signpath.org/apply](https://signpath.org/apply.html) is a HubSpot form. It replaced the earlier Excel-plus-email process in April 2026, per the [site history](https://github.com/SignPath/fdn-website/commits/main/docs/apply.md). The field list below was read from the form's public render definition (portal 145110231, form `bf62807d-bb72-4e45-9bde-1f3a53ba2472`, last updated 2026-09). Intro text: "Please provide basic information about the open source project you are submitting. This helps us verify eligibility for the SignPath Foundation program."

| Field | Req. | Help text (verbatim) | Draft answer |
| --- | --- | --- | --- |
| Project Name | yes | "A Google search for this name should clearly identify your project." | `Toggle Audio (toggle-audio)`. The plain name is generic, so include the slug. |
| Repository URL | yes | "Link to the project's main source code repository (GitHub or GitLab)." | `https://github.com/hotdogee/toggle-audio` |
| Homepage URL | yes | "The project's official homepage. This can be a dedicated website or the repository page." | `https://github.com/hotdogee/toggle-audio` |
| Download URL | no | "A page where users can download your software. This page must mention that the project uses the SignPath Foundation for code signing." | `https://github.com/hotdogee/toggle-audio/releases` (only after the release-notes line from section 3 exists) |
| Privacy Policy URL | no | "Link to your project's privacy policy (required if the software collects user data)." | Optional; `https://github.com/hotdogee/toggle-audio#code-signing-policy` (it contains the privacy statement) |
| Wikipedia URL (optional) | no | "If your project has an English Wikipedia article, please link it here." | blank |
| Tagline | yes | "A short one-sentence summary of your project. This may be displayed on the SignPath Foundation website." | `Flip Windows audio output between two devices in milliseconds: one tiny exe, perfect for a hotkey.` |
| Description | yes | "A short paragraph describing your project and its purpose. Avoid listing version-specific features or dependencies." | See 4.2 |
| Reputation | yes | "Provide links or information showing that your project is widely used or trusted. Examples include media coverage, blog posts, download statistics, GitHub insights, or community discussions." | See 4.2. **Fill in real numbers at application time; this is the weak point.** |
| Maintainer Type | no | "The type of organization or group that maintains the project." Options: Independent community project (no formal organization) / Non-profit foundation or research/educational institution / For-profit company or corporate-backed project / Individual maintainer(s) / Other (please specify) | `Individual maintainer(s)` |
| Build System | yes | "The automated build system used to build your project." Options: GitHub Actions / GitLab CI/CD | `GitHub Actions` |
| First Name | yes | "The name of the user account that will be created in SignPath." | `Han` |
| Last Name | yes | (same) | `Lin` |
| Email | yes | "The email address for the SignPath account and application notifications." | `hotdogee@gmail.com` |
| Company Name | no | "Your organization or employer, if applicable." | blank |
| Primary Discovery Channel | yes | "How did you first discover the SignPath Foundation?" Options: Organic search / AI / LLM tools / Developer platforms (e.g. GitHub) / Community platforms / Social media / Events / Referral / Direct contact / Other | Answer truthfully. Microsoft Learn's [code signing options](https://learn.microsoft.com/en-us/windows/apps/package-and-deploy/code-signing-options) page links SignPath Foundation. |
| Please specify the exact source (optional) | no | "For example: Google search, ChatGPT, GitHub repository, Reddit, conference name, blog article URL, etc." | optional |
| reCAPTCHA | yes | | |
| Consent checkbox | **yes** | "I have read and agree to the SignPath Foundation Code of Conduct and I understand that certificates are issued in SignPath Foundation's name and may be revoked if terms are violated." | tick |
| Marketing checkbox | no | "I agree to receive other communications from SignPath." | your choice |

The form has **no separate "build process" or "license" field**. SignPath checks those from the repository. Section 4.2 has a short build description to use if they ask by email.

### 4.2 Drafted text

**Description:**

> Toggle Audio is a small open-source Windows utility that switches the default playback device between two outputs the user picked (for example speakers and headphones) each time it runs, then exits. It is a native x64 executable written in Rust with no runtime, no background process and no network access, designed to be bound to a macro key in Logitech G HUB, Stream Deck, AutoHotkey or a Windows shortcut. A settings dialog picks the two devices; an MSI installer and a portable zip are provided.

**Reputation** (template; replace each bracket with real, checkable facts on the day you apply, and leave out anything that is not true):

> Released 2026-10-04 (v0.1.0) on GitHub Releases with SHA-256 sums and GitHub build-provenance attestations. Available through winget as `Hotdogee.ToggleAudio` ([winget-pkgs PR #446595], [merged on ...]). Download counts: [...] (GitHub Releases API). GitHub stars: [...]. Discussions or posts: [links]. The maintainer's GitHub account (hotdogee) has been active since 2012.

**Build process** (for follow-up questions):

> Releases are built only by `.github/workflows/release.yml` on GitHub-hosted `windows-latest` runners when a `vX.Y.Z` tag is pushed. The job checks that the tag matches Cargo.toml, runs `cargo test --locked` and `cargo build --release --locked` (static CRT, no build cache) and builds the MSI with WiX Toolset 7 (`installer/build-msi.ps1`). It then submits the unsigned MSI to SignPath through `signpath/github-action-submit-signing-request`, extracts the signed executables from the signed MSI for the portable zip, writes SHA256SUMS.txt and GitHub build-provenance attestations, and publishes the GitHub Release.

### 4.3 What happens after approval

Confirmed by the documentation:

- **Accounts.** You sign in at app.signpath.io with an interactive account (Google, Microsoft, or a username/password account hosted by SignPath on Okta) and accept the organization invitation within 14 days ([users](https://docs.signpath.io/users)).
- **Organization ID.** Shown by clicking the organization name at the upper right ([official demo README](https://github.com/SignPath/demo-github-actions)). Store it as the repository **variable** `SIGNPATH_ORGANIZATION_ID`. It is not secret.
- **Project.** It has a **slug** used by the API ("Valid characters: ASCII letters, digits, and the characters ., -, and _. Case-insensitive.") and a **Repository URL**, which origin verification checks ([projects](https://docs.signpath.io/projects)). This design assumes the slug `toggle-audio`.
- **Signing policies.** A project typically has `test-signing` and `release-signing`. A signing policy defines the certificate, the **Submitters** (the CI user), the **Approvers**, the **required approvals** count, trusted build system verification and origin verification. For Open Source Code Signing, trusted build system verification and origin verification are "required", and **malware scanning cannot be disabled** ("Not available for Open Source Code Signing") ([projects](https://docs.signpath.io/projects)).
- **Trusted build system.** The predefined **GitHub.com** trusted build system must be added to the organization and linked to the project ([GitHub doc](https://docs.signpath.io/trusted-build-systems/github)). Installing the SignPath GitHub App "is only required if source code and build policy verification is used" ([changelog, GitHub Connector 1.2.0](https://docs.signpath.io/changelog/)).
- **CI user and API token.** Create a CI user, make it a Submitter of the signing policy and generate its API token. "API tokens are only displayed when generated." Store it as the repository **secret** `SIGNPATH_API_TOKEN` ([users](https://docs.signpath.io/users)).
- **Approval of each release.** With "Use approval process", approvers "will receive e-mail notifications for each request". They approve or deny on the signing request page, and "a single deny will abort the request" ([projects](https://docs.signpath.io/projects)). The GitHub job waits meanwhile (`wait-for-completion`). Han Lin is the only approver, with 1 required approval. The CI user submits and Han approves; the docs say nothing against the same person being submitter and approver, because the submitter here is the CI user.
- **Artifact configuration.** Pasted into the project in the web UI as XML. You can also upload a sample artifact and let SignPath generate one, which is a good way to double-check the MSI-internal paths in section 5.3. Keep a reviewed copy in the repository (for example `.signpath/artifact-configuration.xml`, as zedis does); SignPath does not read it from there.
- **Resubmit.** A completed request can be resubmitted under another signing policy, for example test-signing first and release-signing later ([signing code](https://docs.signpath.io/signing-code)).

Not confirmed from primary sources **(unconfirmed)**:

- **Who creates what.** Whether SignPath Foundation pre-creates the project, policies and certificate, or the maintainer creates them and the Foundation then attaches the release certificate. Another project's onboarding notes ([Ferrite](https://github.com/OlaProeis/Ferrite/blob/master/docs/technical/platform/signpath-code-signing.md)) describe testing with a self-signed test certificate first and then asking for the production certificate. Plan for a `test-signing` round before the first real release.
- **Annual quotas** for Foundation projects (see 5.7).
- **How "Allowed branch names" matches tag builds.** See 5.5.

---

## 5. Pipeline design

### 5.1 Options

| | A. One request: zip with both exes and the MSI, MSI deep-signed | B. Two requests: sign exes, rebuild the MSI, sign the MSI | **C. One request: MSI only, deep-signed; extract the signed exes for the zip** |
| --- | --- | --- | --- |
| Manual approvals per release | 1 | **2**, each blocking the job | 1 |
| Individual signatures per release | 5 (2 exes in the zip + 2 in the MSI + MSI) | 3 | 3 |
| Exes in the zip vs. installed by the MSI | **Different bytes** (signed twice with different timestamps; confirmed on the latest starship release: zip `starship.exe` and the MSI's `starship.exe` hash differently) | Identical | Identical |
| MSI built from | unsigned exes, repacked by SignPath | signed exes, by WiX | unsigned exes, repacked by SignPath |
| MSI touched after WiX | yes (SignPath repacks it) | no (only signed) | yes (SignPath repacks it) |
| Workflow complexity | low | highest (two upload/sign rounds, two artifact configurations, MSI build after signing) | low, plus one `msiexec /a` step |
| Used by | starship (one artifact with exe + MSI), zedis, telepresence | Ditto (exe config, then installer config) | (no example found; same mechanism as A) |

**Is deep signing of an MSI supported? Yes.** The [syntax doc](https://docs.signpath.io/artifact-configuration/syntax) says: "For composite file formats like packages and installers, SignPath supports signing these files and their contents in a single step." Its example signs `myapp.exe` inside `<msi-file>` and then the MSI. The [reference](https://docs.signpath.io/artifact-configuration/reference) lists `<msi-file>` as a composite format (`.msi, .msm, .msp`) with `<authenticode-sign>`. [Setting up projects](https://docs.signpath.io/projects): "SignPath will extract the files and sign them from the inside out, then re-package everything and sign the containing file." The MSI must therefore be **built from the unsigned exes**, and SignPath signs them inside it. Starship's signed MSI (checked locally) has a validly signed `starship.exe` inside, with the same certificate as the MSI.

**Decision: C.**

- It has A's single approval and B's byte-identical exes.
- It uses the fewest signatures, which matters while the Foundation quota is unknown.
- It keeps one artifact configuration.
- The extra step is an administrative install (`msiexec /a`). It only extracts files, needs no installation, and `docs/packaging.md` already uses it to inspect packages.

Cost: the MSI that ships is SignPath's repack of the WiX output, not the WiX output itself. The pipeline therefore re-runs `wix msi validate` on the signed MSI, which the existing "Stage release assets" step already does. Check the first signed MSI with a real install, upgrade and uninstall (`docs/packaging.md`, "Testing a real install").

This replaces the order written in `docs/packaging.md` ("sign both exes, build the MSI, sign the MSI, then compute the hashes"). The new order is: build the exes, build the MSI, have SignPath deep-sign the MSI, extract the signed exes, build the zip, then compute the hashes and attestations.

### 5.2 MSI-internal paths

SignPath addresses files inside an MSI by the directory structure of the MSI's Directory table, as an administrative install lays it out. In SignPath's [official demo](https://github.com/SignPath/demo-github-actions/blob/main/.signpath/artifact-configurations/default.xml), `ProgramFilesFolder Name='application'` with `INSTALLDIR Name='SignPath Demo'` becomes `<directory path="application/SignPath Demo">`. zedis uses `PFiles/zedis/zedis.exe` for the same reason.

For Toggle Audio, the Directory table of `toggle-audio-0.1.0-x64.msi` has `ProgramFiles64Folder` with DefaultDir `PFiles64` and `INSTALLFOLDER` with `hvvho887|Toggle Audio`. `msiexec /a` (run locally) produced:

```text
PFiles64\Toggle Audio\LICENSE.txt
PFiles64\Toggle Audio\README.md
PFiles64\Toggle Audio\THIRD-PARTY-NOTICES.txt
PFiles64\Toggle Audio\toggle-audio.exe
PFiles64\Toggle Audio\toggle-audiow.exe
```

So the path is `PFiles64/Toggle Audio/<exe>`. Paths are case-insensitive, and `/` and `\` are both accepted ([syntax](https://docs.signpath.io/artifact-configuration/syntax)). If the MSI layout changes, update the configuration. Uploading a sample MSI in SignPath ("Update from an artifact sample") shows the paths that SignPath itself sees.

### 5.3 Artifact configuration XML (chosen design)

`actions/upload-artifact` zips by default, so the root must be `<zip-file>` ([GitHub doc](https://docs.signpath.io/trusted-build-systems/github): "By default, the upload-artifact action creates a ZIP archive, which requires the root element of your Artifact Configurations to be of type `<zip-file>`"). Element and attribute names below were checked against the [reference](https://docs.signpath.io/artifact-configuration/reference) and the [examples](https://docs.signpath.io/artifact-configuration/examples):

- `<parameters>`/`<parameter name required>`
- `<msi-file>` with the MSI restrictions `subject` and `author`
- `<directory>`
- `<pe-file-set>` with the PE restrictions `product-name`, `product-version`, `company-name` and `original-filename` (allowed on `<include>` too)
- `<authenticode-sign>` with `description` and `description-url`

```xml
<?xml version="1.0" encoding="utf-8"?>
<!--
  SignPath artifact configuration for Toggle Audio (reviewed copy; the active
  copy lives in app.signpath.io > project toggle-audio > Artifact configurations).

  Artifact: the GitHub Actions artifact uploaded by release.yml, i.e. a ZIP whose
  root holds toggle-audio-<version>-x64.msi. SignPath deep-signs the MSI: both
  executables inside it first, then the MSI itself. The release workflow then
  extracts the signed executables with msiexec /a for the portable zip.

  Paths inside the MSI follow its Directory table as an administrative install
  lays it out (installer/toggle-audio.wxs: ProgramFiles64Folder = PFiles64,
  INSTALLFOLDER = "Toggle Audio").

  Metadata restrictions (required by the SignPath Foundation terms): product name
  and product version of every signed PE file; Subject and Author of the MSI
  (WiX SummaryInformation Description and Package Manufacturer).
-->
<artifact-configuration xmlns="http://signpath.io/artifact-configuration/v1">
  <parameters>
    <parameter name="version" required="true" />
  </parameters>
  <zip-file>
    <msi-file path="toggle-audio-${version}-x64.msi"
              subject="Toggle Audio ${version} installer" author="Han Lin">
      <directory path="PFiles64/Toggle Audio">
        <pe-file-set product-name="Toggle Audio" product-version="${version}" company-name="Han Lin">
          <include path="toggle-audio.exe" original-filename="toggle-audio.exe" />
          <include path="toggle-audiow.exe" original-filename="toggle-audiow.exe" />
          <for-each>
            <authenticode-sign description="Toggle Audio" description-url="https://github.com/hotdogee/toggle-audio" />
          </for-each>
        </pe-file-set>
      </directory>
      <authenticode-sign description="Toggle Audio" description-url="https://github.com/hotdogee/toggle-audio" />
    </msi-file>
  </zip-file>
</artifact-configuration>
```

Notes:

- `version` comes from the action's `parameters` input (`version: "0.2.0"`). It must equal the exes' ProductVersion string, which `build.rs` takes from Cargo.toml (observed: `0.1.0` for v0.1.0), and the MSI Subject. Exactly how SignPath compares `product-version` (the string resource or the fixed binary version `0.1.0.0`) is **(unconfirmed)**. If the first test-signing request fails on this attribute, try `0.1.0.0`, or ask SignPath support.
- `file-version` is deliberately omitted for the same reason.
- No `hash-algorithm` attribute is needed: `<authenticode-sign>` defaults to `sha-256` ([reference](https://docs.signpath.io/artifact-configuration/reference)).
- Variant A, if ever preferred: add a sibling `<pe-file-set>` (same restrictions, `<include path="toggle-audio.exe"/>`, `<include path="toggle-audiow.exe"/>`) directly under `<zip-file>`, and upload the two exes together with the MSI.

### 5.4 Workflow changes (release.yml)

The [GitHub integration doc](https://docs.signpath.io/trusted-build-systems/github) and the action's [`action.yml` at v3](https://github.com/SignPath/github-action-submit-signing-request/blob/v3/action.yml) define these inputs:

| Input | Default | Use here |
| --- | --- | --- |
| `connector-url` | v3: `https://pipelineconnector.connectors.signpath.io/GitHubActions/GitHubCom` (the docs table still shows `.../GitHub/GitHubCom`) | omit |
| `api-token` | mandatory | `${{ secrets.SIGNPATH_API_TOKEN }}` |
| `organization-id` | mandatory | `${{ vars.SIGNPATH_ORGANIZATION_ID }}` |
| `project-slug` | mandatory | `toggle-audio` |
| `signing-policy-slug` | mandatory | `release-signing` (`test-signing` while onboarding) |
| `artifact-configuration-slug` | project default | omit (one configuration) |
| `github-artifact-id` | mandatory: "Must be uploaded using the actions/upload-artifact v4+ action before it can be signed. Use `${{ steps.<step-id>.outputs.artifact-id }}`" | from the upload step |
| `github-token` | `${{ github.token }}`; "Requires the action:read and content:read permissions" | default |
| `wait-for-completion` | `true` | `true` |
| `wait-for-completion-timeout-in-seconds` | `600` | `3600` (manual approval) |
| `service-unavailable-timeout-in-seconds` | `600` | default |
| `download-signed-artifact-timeout-in-seconds` | `300` | default |
| `output-artifact-directory` | none, in which case nothing is downloaded | `signed` |
| `parameters` | none; "one line per parameter with the format `<name>: "<value>"` where `<value>` needs to be a valid JSON string" | `version: "<x.y.z>"` |
| `skip-decompress` | `false` | `false` (artifact is zipped) |

Outputs: `signing-request-id`, `signing-request-web-url`, `signed-artifact-download-url`, and `signpath-api-url` in `action.yml` (the changelog for 2.0.0 says that one was removed).

**Current major version: v3.** It was tagged on 2026-09-10 (`v3.0` = commit `f6d04783`) and runs on `node24`. The only change from v2.3 is the new default connector URL, the "Pipeline Connector" that supports the new pipeline policies (changelog, Pipeline Connector 0.8.0). The docs and the official demo use `@v3`, and starship pins `f6d04783… # v3.0`. bottom, nextest and Ditto still use v1/v2.

`actions/upload-artifact@v7` (already used in this repository) has the `artifact-id` output and the `archive` input. Use a unique artifact `name` (`unsigned-msi`), because the job already uploads `release-assets`, and the SignPath doc warns about an upload-artifact bug with duplicate names (actions/upload-artifact issues #769 and #785).

> **Superseded draft.** The two YAML fragments below are the first draft from this research, kept to show the reasoning. The implemented workflow differs, and [`.github/workflows/release.yml`](../../.github/workflows/release.yml) and [docs/signing.md](../signing.md) are authoritative. What changed and why:
>
> - **On/off switch.** The draft signs whenever `vars.SIGNPATH_ORGANIZATION_ID` exists. The workflow uses `vars.SIGNPATH_SIGNING_POLICY_SLUG` as the switch for tag releases, so the SignPath connection can be set up and tested with a `test-signing` dry run (`workflow_dispatch`) while tag releases stay unsigned. A dry run is limited to `test-signing`, so trusted signed files only come from tags.
> - **No hard-coded slugs.** The project slug comes from `vars.SIGNPATH_PROJECT_SLUG` and the policy from the switch variable; a half-finished configuration fails before the build.
> - **Artifact name** `signpath-unsigned-msi` instead of `unsigned-msi`, with `overwrite: true` so a re-run of the job can upload it again.
> - **No `GITHUB_ENV`.** The exe directory is a step output (`bin-dir`), and signature checks are a separate step that fails on a tag push and only warns in a dry run; it also requires one signer certificate for all three files.
> - **Two jobs instead of one.** Signing runs in a `build` job with only `contents: read` and `actions: read`, so neither SignPath nor its action receives a write-scoped token. A separate `publish` job (tag pushes only) holds `contents`, `attestations` and `id-token: write`, downloads the verified files and attests and publishes them.
> - Third-party actions are pinned to commit SHAs.

The draft steps go between "Build MSI" and "Stage release assets". The job's permission block gains `actions: read`: the workflow sets explicit permissions, so every scope not listed is `none`. The doc makes the permission mandatory only for private repositories without the GitHub App, but it costs nothing to set it here. Each fragment parses with PyYAML on its own; they are fragments, not a complete workflow.

Draft permissions (top level of the workflow):

```yaml
permissions:
  contents: write       # create the release and upload assets
  id-token: write       # build provenance (Sigstore OIDC)
  attestations: write   # build provenance
  actions: read         # SignPath reads the workflow run and downloads the unsigned artifact
```

Draft steps (inside `jobs.release.steps`, after "Build MSI"):

```yaml
      # ---- Code signing (SignPath Foundation), see docs/research/signpath.md ----
      # Skipped until the SIGNPATH_ORGANIZATION_ID repository variable exists, so
      # the pipeline keeps producing unsigned releases before onboarding.
      - name: Upload unsigned MSI for signing
        id: upload-unsigned
        if: vars.SIGNPATH_ORGANIZATION_ID != ''
        uses: actions/upload-artifact@v7
        with:
          name: unsigned-msi
          path: installer/out/toggle-audio-${{ steps.version.outputs.version }}-x64.msi
          if-no-files-found: error
          retention-days: 1

      # Release-signing needs a manual approval in SignPath for every request; the
      # job waits up to an hour for it.
      - name: Submit signing request to SignPath
        id: sign
        if: vars.SIGNPATH_ORGANIZATION_ID != ''
        uses: signpath/github-action-submit-signing-request@v3
        with:
          api-token: ${{ secrets.SIGNPATH_API_TOKEN }}
          organization-id: ${{ vars.SIGNPATH_ORGANIZATION_ID }}
          project-slug: toggle-audio
          signing-policy-slug: release-signing
          github-artifact-id: ${{ steps.upload-unsigned.outputs.artifact-id }}
          wait-for-completion: true
          wait-for-completion-timeout-in-seconds: 3600
          output-artifact-directory: signed
          parameters: |
            version: "${{ steps.version.outputs.version }}"

      - name: Take the signed MSI and extract its signed executables
        if: vars.SIGNPATH_ORGANIZATION_ID != ''
        env:
          VERSION: ${{ steps.version.outputs.version }}
        run: |
          $ErrorActionPreference = 'Stop'
          $msiName = "toggle-audio-$env:VERSION-x64.msi"
          if (-not (Test-Path "signed/$msiName")) { throw "SignPath returned no signed/$msiName" }
          Copy-Item "signed/$msiName" "installer/out/$msiName" -Force

          # Administrative install = plain extraction of the MSI payload; nothing is installed.
          $admin = Join-Path $env:RUNNER_TEMP 'msi-admin'
          $msiPath = (Resolve-Path "installer/out/$msiName").Path
          $p = Start-Process msiexec.exe -Wait -PassThru -ArgumentList "/a `"$msiPath`" /qn TARGETDIR=`"$admin`""
          if ($p.ExitCode -ne 0) { throw "msiexec /a failed with exit code $($p.ExitCode)" }
          $bin = New-Item -ItemType Directory -Force signed-bin
          Copy-Item "$admin/PFiles64/Toggle Audio/toggle-audio.exe", "$admin/PFiles64/Toggle Audio/toggle-audiow.exe" $bin

          foreach ($f in @($msiPath) + (Get-ChildItem $bin -Filter *.exe).FullName) {
            $s = Get-AuthenticodeSignature $f
            if ($s.Status -ne 'Valid') { throw "$f signature status: $($s.Status) $($s.StatusMessage)" }
            if ($s.SignerCertificate.Subject -notmatch 'O=SignPath Foundation') { throw "$f unexpected signer: $($s.SignerCertificate.Subject)" }
            if (-not $s.TimeStamperCertificate) { throw "$f signature is not timestamped" }
            Write-Host "$([IO.Path]::GetFileName($f)): $($s.Status), $($s.SignerCertificate.Subject), timestamped by $($s.TimeStamperCertificate.Subject)"
          }
          "BIN_DIR=signed-bin" >> $env:GITHUB_ENV
```

Draft changes to the existing steps:

- **Stage release assets:**
  - Copy the exes from `$bin = if ($env:BIN_DIR) { $env:BIN_DIR } else { 'target/release' }` instead of the fixed `target/release`.
  - Keep the existing `wix msi validate` of the (now signed) MSI.
  - Keep `SHA256SUMS.txt` last.
  - Optionally re-check the UpgradeCode of the signed MSI, as `build-msi.ps1` does.
- **Extract release notes:** append the "Code signing policy" line from section 3.
- **attest-build-provenance:** use `${{ env.BIN_DIR || 'target/release' }}/toggle-audio.exe` and `.../toggle-audiow.exe` as subjects, so the attested exes are the shipped (signed) ones.
- **winget.yml:** no change. `winget-releaser` hashes the published, signed MSI.

Gating on a repository variable keeps `main` releasable before onboarding finishes. After onboarding, a signing failure fails the job, so no unsigned release can slip out while the variable is set. During onboarding, use a throwaway tag on a test branch with `signing-policy-slug: test-signing`, or run the same steps in a `workflow_dispatch` copy. Either way, check the configuration before the first real tag. Do not publish a GitHub Release from the test run.

### 5.5 What origin verification constrains

From [origin verification](https://docs.signpath.io/origin-verification/), [trusted build systems](https://docs.signpath.io/trusted-build-systems/) and the [GitHub page](https://docs.signpath.io/trusted-build-systems/github):

- **Checks SignPath makes:**
  - "A build was actually performed by a GitHub workflow, not by some other entity in possession of the API token"
  - "Origin metadata is provided by GitHub, not the build script, and can therefore not be forged"
  - "The artifact is stored as a GitHub workflow artifact before it is submitted for signing"
  - "**For OSS projects:** All jobs of the GitHub workflow leading up to the signing request were executed on GitHub-hosted agents"

  So: no self-hosted runners in `release.yml`, and the artifact must come from `actions/upload-artifact` in the same run. A file uploaded by hand or built on a laptop cannot be release-signed.
- **What is verified:** the repository URL (must equal the project's Repository URL), the branch (against the policy's **Allowed branch names**), the commit, the build job URL, and "reproducibility". The last one means build settings come from files under source control, there are no manual overrides, and caching from earlier unverified builds is prevented. Keep the release job cache-free, as it is now.
- **Tags.** `release.yml` runs on `push: tags: ['v*']`, so the run's ref is `refs/tags/vX.Y.Z`, not a branch. nextest release-signs from tag-triggered runs, so tag builds are accepted in practice. How the "Allowed branch names" field is matched against a tag ref is **(unconfirmed)**. Ask during onboarding that `release-signing` allows the release tags (for example `refs/tags/v*`), or whatever form SignPath expects.
- **Re-runs.** "SignPath currently allows policy evaluation for up to 3 re-runs of a build." A `disallow_reruns: true` pipeline policy would forbid re-running a release job, for example after the approval timed out. Do not enable that policy, or push a new tag instead.
- **Reviews cover build scripts:** "Make sure that your source code review policy includes CI configuration, build scripts, and makefiles. External content should not be accepted in reviews." This applies to `release.yml`, `build.rs`, `installer/*.ps1` and `toggle-audio.wxs`.

### 5.6 Timestamps and digests

- The digest is SHA-256 by default (`hash-algorithm` default `sha-256`; `sha1`, `sha384` and `sha512` are also accepted) ([reference](https://docs.signpath.io/artifact-configuration/reference)).
- SignPath timestamps every signature. Its TSA is not configurable on SaaS, and since 2023 "Timestamping now falls back to alternative timestamping servers when primary server is unavailable" ([changelog](https://docs.signpath.io/changelog/), Application 1.151.1). The TSA URL is not documented. Observed: DigiCert (`DigiCert SHA256 RSA4096 Timestamp Responder 2025 1`) on a 2026-08-27 signature.
- Certificate chains are embedded ("Certificate chains are now always embedded in Authenticode ... signatures", changelog).

### 5.7 Limits

- **Quotas:** SignPath tracks two quotas per organization, both counted per year since November 2025 (changelog, Application 1.200.2): the total artifact size (`yearlyMaxSizeOfArtifactsInBytes`) and the number of individual signatures (`yearlyMaxNumberOfIndividualSignatures`). The organization details page shows the limits. The **values for Foundation subscriptions are not published (unconfirmed)**. Read them after onboarding. This design uses 3 signatures and well under 1 MB per release.
- **Size:** "Limited the maximum file size for artifact retrieval to 4GB in SaaS" (changelog). OPC files are limited to 40 MB and XML files to 2 MB. Neither format is used here.
- **Action timeouts:** see the table in 5.4. The job's 6-hour limit on GitHub-hosted runners caps any approval wait.
- **Malware scanning** always runs for Open Source Code Signing and cannot be turned off ([projects](https://docs.signpath.io/projects)). A false positive blocks signing until it is resolved.
- **SBOM:** the terms reserve the right to require SBOMs later.

---

## 6. Optional extras SignPath offers

- **[Pipeline policies](https://docs.signpath.io/pipeline-policies/)** ("Available for Pipeline Integrity, Open Source Code Signing"). Example: `github-build-policies: runners: require_github_hosted: true`, plus `github-scm-policies` ruleset constraints such as `non_fast_forward`. Rules like `pull_request` with `required_approving_review_count` do not fit a one-person project.
- **[SLSA attestations](https://docs.signpath.io/slsa-attestations/)** signed by SignPath. Whether the Open Source subscription includes them is **(unconfirmed)**. The GitHub build-provenance attestations already in `release.yml` cover provenance in the meantime.

---

## 7. What users see and how they verify a signature

**Verify** (PowerShell, no extra tools):

```powershell
Get-AuthenticodeSignature .\toggle-audio-0.2.0-x64.msi |
  Format-List Status, StatusMessage, SignerCertificate, TimeStamperCertificate
```

Expect `Status : Valid`, a signer of `CN=SignPath Foundation, O=SignPath Foundation, L=Lewes, S=Delaware, C=US`, and a timestamping certificate. The same check works on `toggle-audio.exe` and `toggle-audiow.exe`.

**Verify** with `signtool` (Windows SDK):

```powershell
signtool verify /pa /v .\toggle-audio-0.2.0-x64.msi
```

`/pa` selects the Default Authenticode Verification Policy ([SignTool](https://learn.microsoft.com/en-us/windows/win32/seccrypto/signtool)). The output lists the chain (GlobalSign Code Signing Root R45 > GlobalSign GCC R45 CodeSigning CA 2020 > SignPath Foundation), "The signature is timestamped: ..." and "Successfully verified". Explorer shows the same under Properties > Digital Signatures.

Signature, SHA-256 sums and attestation are complementary checks:

- **The signature** shows that SignPath Foundation signed a build of this repository.
- **`SHA256SUMS.txt`** and `gh attestation verify <file> --repo hotdogee/toggle-audio` still pin the exact files.

**SmartScreen and UAC.** Microsoft's [SmartScreen reputation](https://learn.microsoft.com/en-us/windows/apps/package-and-deploy/smartscreen-reputation) and [code signing options](https://learn.microsoft.com/en-us/windows/apps/package-and-deploy/code-signing-options) pages (updated in 2026):

- A validly signed file still gets a warning on first download ("flagged as unrecognized until reputation accumulates"), but the "verified publisher name is displayed". For Toggle Audio, the SmartScreen dialog and the MSI's UAC prompt show **SignPath Foundation** instead of "Unknown publisher". They do not show Han Lin or Toggle Audio.
- Reputation builds from file hash and publisher certificate. "It can take several weeks and hundreds of clean installs." Signing every release with the same certificate lets "certificate reputation ... build, potentially avoiding warnings on new files signed by the same trusted certificate." EV certificates no longer bypass this.
- Each Foundation project has its own certificate. bottom, nextest and starship binaries all carry the same subject, but each has a different thumbprint and serial number. This project therefore builds its own reputation rather than inheriting another project's. That conclusion is inferred from the distinct certificates; how SmartScreen weighs a shared subject name is not documented.
- The certificate is renewed periodically. The one observed is valid until 2027-09-08, and renewal is a certificate change that the docs say "affects the publisher trust signal". Timestamped signatures stay valid after expiry.
- **Smart App Control** (Windows 11) "will block execution of unsigned files unless the file has a positive reputation". Signing is the only way Toggle Audio runs on machines where it is enabled.

---

## 8. Reference projects

These projects were verified on 2026-10-04 by reading their workflows on GitHub:

| Project | What it shows | Workflow / policy |
| --- | --- | --- |
| [starship/starship](https://github.com/starship/starship) (Rust, WiX MSI) | One request with `starship.exe` and the MSI built from the unsigned exe; action pinned to v3.0; org id in a repository variable; README policy with roles and the privacy sentence | [release.yml](https://github.com/starship/starship/blob/main/.github/workflows/release.yml), README "Code Signing Policy" |
| [nextest-rs/nextest](https://github.com/nextest-rs/nextest) (Rust) | Tag-triggered release; `release-signing` for release tags, `test-signing` for others; sets ProductName/ProductVersion with verpatch before signing | [release.yml](https://github.com/nextest-rs/nextest/blob/main/.github/workflows/release.yml), [policy](https://nexte.st/docs/installation/pre-built-binaries/#code-signing-policy) |
| [ClementTsang/bottom](https://github.com/ClementTsang/bottom) (Rust) | Signs `btm.exe` only; nightly builds use `test-signing`; its WiX MSI is **not** signed | [build_releases.yml](https://github.com/ClementTsang/bottom/blob/main/.github/workflows/build_releases.yml) |
| [sabrogden/Ditto](https://github.com/sabrogden/Ditto) | Option B: sign exes with configuration `exe`, build the installer, sign it with configuration `Installer` (two requests, two approvals) | [build.yml](https://github.com/sabrogden/Ditto/blob/master/.github/workflows/build.yml) |
| [vicanso/zedis](https://github.com/vicanso/zedis) | Reviewed copy of the artifact configuration in the repo; MSI-internal path `PFiles/zedis/zedis.exe` | [.signpath/artifact-configuration.xml](https://github.com/vicanso/zedis/blob/main/.signpath/artifact-configuration.xml) |
| [telepresenceio/telepresence](https://github.com/telepresenceio/telepresence) | `product-name` / `product-version="${version}"` restrictions inside `<msi-file>` | [build-aux/signpath/core.xml](https://github.com/telepresenceio/telepresence/blob/release/v2/build-aux/signpath/core.xml) |
| [localsend/localsend](https://github.com/localsend/localsend) | Code signing policy with a **single person in every role** | [CODE_SIGNING.md](https://github.com/localsend/localsend/blob/main/CODE_SIGNING.md) |
| [SignPath/demo-github-actions](https://github.com/SignPath/demo-github-actions) | Official demo: deep-signed MSI in a zip artifact, policy chosen by branch, `vars.SIGNPATH_ORGANIZATION_ID`, `@v3` | [build-and-sign.yml](https://github.com/SignPath/demo-github-actions/blob/main/.github/workflows/build-and-sign.yml), [default.xml](https://github.com/SignPath/demo-github-actions/blob/main/.signpath/artifact-configurations/default.xml) |

Policy wording used by these projects:

- starship: "Free code signing provided by SignPath.io, certificate by SignPath Foundation." / "Reviewers: Astronauts" / "Approvers and Authors: Mission Control" / the privacy sentence.
- nextest: "Committers and reviewers: Members team" / "Approvers: Owners" / the privacy sentence, plus a note on its self-update feature.
- LocalSend: "Committers and reviewers: @Tienisto" / "Approvers: @Tienisto".

The SignPath Foundation [project list](https://signpath.org/projects) includes small single-purpose tools (for example catlock and catime), not only large projects.

---

## 9. Open items

1. **Reputation (blocking).** Decide when to apply, and gather real evidence first (section 4.2).
2. **MFA.** Confirm GitHub 2FA, and use an MFA-protected Google or Microsoft account for SignPath.
3. **Code signing policy (docs).** Add the README section and the release-notes line *before* applying, because the form's Download URL must already mention SignPath Foundation.
4. **Ask during onboarding:**
   - how tag refs are matched by "Allowed branch names";
   - the Foundation's yearly quotas;
   - whether you or the Foundation creates the project and policies;
   - how `product-version` is compared.
5. **Check the first signed release with a test-signing run:**
   - the MSI-internal paths;
   - `wix msi validate` on the repacked MSI;
   - a real install, upgrade from 0.1.0 and uninstall;
   - the signatures on the extracted exes.
6. **After the first signed release:** update the README Install note, Troubleshooting, `docs/packaging.md` ("Why the MSI is unsigned" and the signing order) and `SECURITY.md`.

## Sources

- SignPath Foundation: [home](https://signpath.org/), [terms / Code of Conduct](https://signpath.org/terms.html), [apply](https://signpath.org/apply.html), [about](https://signpath.org/about), [projects](https://signpath.org/projects), [site repository](https://github.com/SignPath/fdn-website)
- SignPath product page: [Open Source](https://about.signpath.io/product/open-source)
- SignPath documentation: [index](https://docs.signpath.io/), [setting up projects (signing policies, approval, origin verification restriction, deep signing)](https://docs.signpath.io/projects), [signing code](https://docs.signpath.io/signing-code), [artifact configuration](https://docs.signpath.io/artifact-configuration/), [syntax](https://docs.signpath.io/artifact-configuration/syntax), [reference](https://docs.signpath.io/artifact-configuration/reference), [examples](https://docs.signpath.io/artifact-configuration/examples), [trusted build systems](https://docs.signpath.io/trusted-build-systems/), [GitHub](https://docs.signpath.io/trusted-build-systems/github), [origin verification](https://docs.signpath.io/origin-verification/), [pipeline policies](https://docs.signpath.io/pipeline-policies/), [SLSA attestations](https://docs.signpath.io/slsa-attestations/), [users](https://docs.signpath.io/users), [changelog](https://docs.signpath.io/changelog/)
- GitHub Action: [SignPath/github-action-submit-signing-request](https://github.com/SignPath/github-action-submit-signing-request) ([action.yml at v3](https://github.com/SignPath/github-action-submit-signing-request/blob/v3/action.yml)); [actions/upload-artifact action.yml at v7](https://github.com/actions/upload-artifact/blob/v7/action.yml)
- Microsoft Learn: [Code signing options](https://learn.microsoft.com/en-us/windows/apps/package-and-deploy/code-signing-options), [SmartScreen reputation](https://learn.microsoft.com/en-us/windows/apps/package-and-deploy/smartscreen-reputation), [SignTool](https://learn.microsoft.com/en-us/windows/win32/seccrypto/signtool)
