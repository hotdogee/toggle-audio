# Security Policy

## Supported versions

Toggle Audio is maintained by one person, so only the latest release receives security fixes. Please upgrade before reporting.

| Version | Supported |
| --- | --- |
| 0.1.x (latest) | Yes |
| Earlier builds and the PowerShell proof of concept | No |

Once 1.0.0 ships, the latest minor release of the current major version will be supported.

## Reporting a vulnerability

Please do **not** open a public issue, discussion or pull request for a security problem.

Report it privately through GitHub's private vulnerability reporting:

1. Go to <https://github.com/hotdogee/toggle-audio/security/advisories/new>, or open the repository's **Security** tab and choose **Report a vulnerability**.
2. Describe the issue, the affected version (`toggle-audio --version`), your Windows version, and the steps to reproduce it. A proof of concept helps but is not required.

If you cannot use GitHub, email <hotdogee@gmail.com> with "toggle-audio security" in the subject.

What to expect:

- An acknowledgment within 7 days.
- An assessment and, if the report is accepted, a planned fix and release timeline within 30 days.
- Credit in the release notes and the GitHub Security Advisory, unless you prefer to stay anonymous.

## Verifying releases

Every release publishes `SHA256SUMS.txt` and, when the repository allows it, a GitHub build provenance attestation (`gh attestation verify <file> --repo hotdogee/toggle-audio`). Releases are being moved to Authenticode signing through [SignPath Foundation](https://signpath.org): a signed release carries a valid, timestamped signature from `CN=SignPath Foundation` on the MSI and on both executables, and its release notes say "Signed with SignPath Foundation certificate". Releases whose notes say "Unsigned release" (0.1.0 included) have no signature. Signing happens only in the release workflow, after a manual approval. The policy and how to check a signature are in [docs/signing.md](docs/signing.md). A signature from anyone else, a signed file whose hash is not in `SHA256SUMS.txt`, or a signed file that does not come from this repository's Releases page is worth reporting.

## Scope and security model

Useful context when deciding whether something is a vulnerability:

- **Runs as the invoking user.** Both executables use an `asInvoker` manifest and never request elevation. They can do nothing the user who launched them cannot already do. Administrator rights are needed only once, by the MSI, to install into `C:\Program Files\Toggle Audio\`.
- **Undocumented Windows interface.** Changing the default playback device uses the undocumented `IPolicyConfig` COM interface (CLSID `{870af99c-171d-4f9e-af0d-e63df40c2bc9}`), the same interface used by the Windows Sound settings and by tools such as SoundSwitch and AudioDeviceCmdlets. Microsoft does not document or guarantee it, so a Windows update could change its behavior. A crash or misbehavior caused by such a change is a bug, not a vulnerability, unless it is exploitable.
- **No network access, no background process.** The tool makes no network connections, installs no service, scheduled task or tray process, and exits after each run.
- **Files touched.** It reads and writes only its own configuration file, `%APPDATA%\toggle-audio\config.json`, which is owned by the user. Contents of that file are treated as untrusted input; a crafted config that causes memory corruption or code execution would be in scope.
- **Out of scope:** SmartScreen warnings on unsigned releases or on newly published signed ones (see the README for SHA256 verification), denial of service by a user against their own audio settings, and issues in Windows itself or in third-party launchers such as Logitech G HUB.
