# Release process

For maintainers: how a version of the launcher is made, signed and shipped. A release is a
`vX.Y.Z` tag on `main`; `.github/workflows/release.yml` does the rest. Version numbers follow the
[versioning policy](reference/versioning.md).

## Before the tag

1. **Gate.** On a machine with Docker and a hypervisor: `pnpm release-check`
   ([testing/release-check.md](testing/release-check.md)), and the manual checks it lists for what
   the release changes. CI must be green on `main`.
2. **Version.** Set the same `X.Y.Z` in `package.json`, `src-tauri/tauri.conf.json` and
   `src-tauri/Cargo.toml` (then `cargo check --manifest-path src-tauri/Cargo.toml` updates
   `Cargo.lock`). The MSI needs MAJOR and MINOR ≤ 255 and PATCH ≤ 65535.
3. **Changelog.** Rename `## [Unreleased]` in `CHANGELOG.md` to `## [X.Y.Z] — YYYY-MM-DD` and
   start a new empty `## [Unreleased]` above it.
4. **Store listing.** Update `releaseNotes` in `packaging/store/listings/<lang>.json`
   ([packaging/store/README.md](../packaging/store/README.md)).
5. Commit as `Cyber CTF X.Y.Z`, with a short summary of the release in the body, on `main`.

## Tag

```bash
git tag -s vX.Y.Z -m "Cyber CTF X.Y.Z"   # a signed tag, when you have a signing key set up in Git
git push origin vX.Y.Z
```

## What the workflow does

| Job           | What                                                                                                                                                                              |
| ------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `preflight`   | the Tauri JS/Rust versions agree; on a tag, says which signing secrets are missing (a warning: what they sign ships unsigned, the release goes on)                                |
| `e2e`         | the web end-to-end suite (`e2e.yml`); red, the release stops here                                                                                                                 |
| `prepare`     | creates the draft GitHub release (through the `cyber-ctf-creator` GitHub App), once `preflight` and `e2e` are green                                                               |
| `third-party` | `pnpm notices` and `pnpm sbom` ([scripts/release/third-party.ts](../scripts/release/third-party.ts)): THIRD_PARTY_NOTICES and the CycloneDX SBOMs, attached to the release        |
| `release`     | builds on five runners (macOS universal, Linux x64 and arm64 on Ubuntu 22.04, Windows x64 and Arm64), signs, **verifies the signatures**, uploads to the draft; notarizes the DMG |
| `attest`      | `SHA256SUMS` of every asset, and a build provenance attestation of each                                                                                                           |
| `publish`     | takes the draft live (tags only), its notes naming any platform whose build failed; one platform's failure never holds the others back                                            |
| `distribute`  | renders the Homebrew cask from `packaging/homebrew/cyberctf.rb` into `Cyber-Courses/homebrew-tap`, and starts `Cyber-Courses/apt`'s publish workflow                              |
| `store`       | builds the MSIX bundle and submits it to the Microsoft Store (`msix.yml`); `STORE_AUTO_SUBMIT=false` leaves a draft in Partner Center                                             |

What each build signs:

| Platform | Signature                                                                                                                | Verified in the build                                              |
| -------- | ------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------ |
| Windows  | Authenticode through Azure Artifact Signing (`trusted-signing-cli`, pinned): the app, the NSIS installer, the MSI        | `Get-AuthenticodeSignature` is `Valid` on each                     |
| macOS    | Developer ID Application certificate; the app notarized and stapled by tauri-action, the DMG notarized and stapled after | `codesign --verify --deep --strict`, `spctl -a` (app and DMG)      |
| Updates  | minisign with the updater key (`TAURI_SIGNING_PRIVATE_KEY`): the `.sig` beside each update package, and `latest.json`    | by the installed apps, against the public key in `tauri.conf.json` |
| Store    | Microsoft signs the MSIX                                                                                                 |                                                                    |

A manual run (`workflow_dispatch`) builds a draft for the version in `tauri.conf.json`, signs what
it has secrets for, and publishes nothing.

## Secrets and settings it needs

| Secret                                                                      | For                                                                                                       |
| --------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------- |
| `AZURE_CLIENT_ID`, `AZURE_CLIENT_SECRET`, `AZURE_TENANT_ID`                 | the Entra ID app allowed to sign with the Artifact Signing account                                        |
| `AZURE_SIGNING_ENDPOINT`, `AZURE_SIGNING_ACCOUNT`, `AZURE_SIGNING_PROFILE`  | the Artifact Signing endpoint (e.g. `https://weu.codesigning.azure.net`), account and certificate profile |
| `APPLE_CERTIFICATE`, `APPLE_CERTIFICATE_PASSWORD`, `APPLE_SIGNING_IDENTITY` | the Developer ID Application certificate (base64 `.p12`) and its name                                     |
| `APPLE_API_KEY_P8`, `APPLE_API_KEY_ID`, `APPLE_API_ISSUER`                  | the App Store Connect API key (base64 `.p8`) used to notarize                                             |
| `TAURI_SIGNING_PRIVATE_KEY`, `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`           | the updater's minisign key                                                                                |
| `APP_ID_GENERATOR`, `APP_PRIVATE_KEY_GENERATOR`                             | the `cyber-ctf-creator` GitHub App (the draft release, the tap, apt)                                      |
| `STORE_TENANT_ID`, `STORE_CLIENT_ID`, `STORE_CLIENT_SECRET`                 | the Partner Center API (Store submission)                                                                 |

The repository variable `STORE_AUTO_SUBMIT=false` keeps Store submissions as drafts.

## After the release

- Check the release page: every installer, `latest.json`, `SHA256SUMS`, `THIRD_PARTY_NOTICES` and
  the two `.cdx.json` SBOMs; `gh attestation verify <file> --repo Cyber-Courses/Cyber-CTF-Launcher`
  on one of them.
- The Homebrew tap and the apt repository have the new version (`brew upgrade --cask cyberctf`,
  `apt-get update && apt-cache policy cyber-ctf`).
- The Store submission went into certification (Partner Center). `msix.yml`'s `commit_pending` and
  `discard_pending` inputs handle a submission left open.

## When something fails

- **A missing signing secret**: `preflight` names it. Add it, delete the tag (local and remote),
  and push it again.
- **One platform's build failed**: re-run the failed job. The draft keeps what the others
  uploaded; `attest` and `publish` wait for every platform.
- **Distribution only**: re-run `distribute.yml` by hand with the tag (`workflow_dispatch`). It
  renders the cask from the tag's own `packaging/homebrew/cyberctf.rb`, so a tag from before that
  file existed can't use it.
