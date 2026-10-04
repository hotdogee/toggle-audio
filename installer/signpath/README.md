# SignPath artifact configuration

[`artifact-configuration.xml`](artifact-configuration.xml) tells [SignPath](https://signpath.io) what to sign in the artifact that the [release workflow](../../.github/workflows/release.yml) submits:

- The artifact is a ZIP (made by `actions/upload-artifact`) that holds `toggle-audio-<version>-x64.msi`, built from the **unsigned** executables.
- SignPath signs `toggle-audio.exe` and `toggle-audiow.exe` inside the MSI (folder `PFiles64/Toggle Audio`), repacks the MSI and then signs the MSI itself. This is deep signing of a composite file ([syntax](https://docs.signpath.io/artifact-configuration/syntax)).
- Signing fails unless each executable has ProductName `Toggle Audio`, CompanyName `Han Lin`, its own OriginalFilename and a ProductVersion equal to the `version` parameter, and the MSI has the Subject `Toggle Audio <version> installer` and the Author `Han Lin`. The SignPath Foundation [terms](https://signpath.org/terms.html) require these metadata restrictions.

The policy, the release procedure and the rest of the SignPath set-up are in [docs/signing.md](../../docs/signing.md).

## This file is a reviewed copy

SignPath does not read the configuration from the repository. The active copy is stored in the SignPath project. Keep the two identical:

1. Change `artifact-configuration.xml` in a pull request, like any other build script.
2. After the merge, paste the same XML into SignPath (next section).
3. Check it with a [dry run](../../docs/signing.md#dry-run) before the next release.

## Uploading it to SignPath

In the SignPath web app ([app.signpath.io](https://app.signpath.io)):

1. Open **Projects** and select the project (`toggle-audio`).
2. In the project's **Artifact configurations** section, add a new configuration, or open the existing one and edit it.
3. Paste the full content of `artifact-configuration.xml` and save.
4. Make it the project's **default** artifact configuration. The release workflow does not pass `artifact-configuration-slug`, so SignPath uses the default one.

Labels in the SignPath UI can change; the [projects documentation](https://docs.signpath.io/projects) and the [artifact configuration documentation](https://docs.signpath.io/artifact-configuration/) describe the current screens. SignPath can also generate a configuration from an uploaded sample file. Uploading the unsigned MSI from a dry run (the `signpath-unsigned-msi` workflow artifact) that way shows the paths that SignPath sees inside the MSI, which is a good cross-check of `PFiles64/Toggle Audio`.

## When to change it

| Change | Update |
| --- | --- |
| A file is added to or renamed in the MSI (`installer/toggle-audio.wxs`) | Add or rename the `<include>`; executables and DLLs must be listed to be signed. |
| The install folder or its parent directory changes in the WiX source | The `<directory path>`. Check it with `msiexec /a` ([docs/packaging.md](../../docs/packaging.md#inspecting-a-package-without-installing-it)). |
| The MSI file name changes in `release.yml` or `installer/build-msi.ps1` | `<msi-file path>`. |
| ProductName, CompanyName or the version scheme changes (`assets/app.rc`, `build.rs`, `Cargo.toml`) | The `product-name`, `company-name` or `product-version` restrictions. |
| The MSI Manufacturer or SummaryInformation Description changes | The `author` or `subject` restrictions. |

Element reference: [docs.signpath.io/artifact-configuration/reference](https://docs.signpath.io/artifact-configuration/reference). Examples: [docs.signpath.io/artifact-configuration/examples](https://docs.signpath.io/artifact-configuration/examples).
