# Rylus Shipping TODOs

Shippability roadmap for the three target distribution channels. Current version: **v0.17.0**.

Legend:
- `[ ]` engineering work in this repo
- `[P]` external prerequisite (account registration, cert purchase, identity setup)
- `[$]` ongoing recurring cost

Architectural note: Rylus is a desktop **server** + browser **PWA** client. The tablet never runs native Rylus code — it loads the PWA from the server over mDNS+TLS. App-store strategy reflects this.

---

## 1. AUR (Arch User Repository)

**Status today**: CI produces `packages/rylus-linux.zip` and `packages/rylus*.deb` via `cargo-deb` and publishes them to GitHub Releases on tag push (`.github/workflows/build.yml:44-84`). Alpine musl tarball also published. No Arch-native package exists.

### Prerequisites
- [P] Register an AUR account at `aur.archlinux.org`.
- [P] Reserve the `rylus` package name (and optionally `rylus-bin`) by pushing the first PKGBUILD.
- [P] Generate an SSH keypair; add public key to AUR profile; add private key as `AUR_SSH_KEY` secret in GitHub repo settings.

### Engineering tasks
- [ ] Create `packaging/aur/PKGBUILD` (source-based) — builds from GitHub release tarball, runs `cargo build --release --locked`, installs `rylus` to `/usr/bin/`.
- [ ] Create `packaging/aur/PKGBUILD-bin` variant — downloads the prebuilt Linux binary from GitHub Releases for zero-compile install.
- [ ] Create a real icon asset: `packaging/icons/rylus.svg` and generated `rylus-256.png`. Update `packaging/rylus.desktop` to reference `rylus` (not the fallback `input-tablet`).
- [ ] Create `packaging/systemd/rylus.service` (user unit) — `[Service] ExecStart=/usr/bin/rylus`, `[Install] WantedBy=default.target`.
- [ ] Auto-generate man page via `clap_mangen` in `crates/rylus-server/build.rs`; install `packaging/rylus.1` to `/usr/share/man/man1/`.
- [ ] PKGBUILD install steps: `rylus` → `/usr/bin/`, `.desktop` → `/usr/share/applications/`, icon → `/usr/share/icons/hicolor/scalable/apps/`, LICENSE → `/usr/share/licenses/rylus/LICENSE`, systemd unit → `/usr/lib/systemd/user/`, man page → `/usr/share/man/man1/`.
- [ ] Generate `.SRCINFO` via `makepkg --printsrcinfo > .SRCINFO` and commit alongside each PKGBUILD.
- [ ] Add `publish-aur` CI job to `.github/workflows/build.yml` that, on tag push, bumps `pkgver`, regenerates `.SRCINFO`, and pushes to `ssh://aur@aur.archlinux.org/rylus.git` using the `AUR_SSH_KEY` secret.
- [ ] Document AUR install instructions in `Readme.md` (`yay -S rylus` / `yay -S rylus-bin`).

---

## 2. Apple — Notarized macOS DMG (outside the App Store)

**Status today**: CI runs `cargo bundle --release --package rylus-server` on a self-hosted macOS runner and publishes `macos-intel.zip` (unsigned, Intel-only) on tag push (`.github/workflows/build.yml:116-145`). Bundle identifier `net.freedesk.rylus` is set (`crates/rylus-server/Cargo.toml:50-52`). **No code signing, no notarization, no arm64, no DMG.**

Decision: skip Mac App Store (sandbox blocks screen capture + synthetic input injection) and skip iPad native App Store app (PWA remains the tablet client).

### Prerequisites
- [P][$] Enroll in Apple Developer Program — $99/yr.
- [P] Create a **Developer ID Application** certificate in Apple Developer portal; export as `.p12` with password.
- [P] Generate an app-specific password for the Apple ID used for notarization.
- [P] Add secrets to GitHub repo: `APPLE_DEVELOPER_ID_CERT_P12` (base64 of .p12), `APPLE_DEVELOPER_ID_CERT_PASSWORD`, `APPLE_ID`, `APPLE_TEAM_ID`, `APPLE_NOTARY_PASSWORD`.

### Engineering tasks
- [ ] Create `packaging/macos/icon.icns` — full icon set 16→1024 px.
- [ ] Enrich `[package.metadata.bundle]` in `crates/rylus-server/Cargo.toml`: add `category`, `copyright`, `short_description`, `long_description`, `version`, `icon` pointing to the `.icns`, and `osx_info_plist_exts` / `osx_minimum_system_version`.
- [ ] Add usage-description strings to `Info.plist` via cargo-bundle metadata: `NSScreenCaptureUsageDescription`, `NSAppleEventsUsageDescription` (for any AppleScript/Accessibility hooks). Screen Recording + Accessibility are TCC-prompted at runtime; these strings are mandatory for the prompts to render.
- [ ] Create `packaging/macos/entitlements.plist` with hardened-runtime entries required for signing: `com.apple.security.cs.allow-jit` (if needed), `com.apple.security.cs.disable-library-validation` (only if loading unsigned dylibs — avoid if possible).
- [ ] Switch CI macOS job to build **universal**: `cargo build --release --target x86_64-apple-darwin` + `cargo build --release --target aarch64-apple-darwin`, then `lipo -create … -output Rylus.app/Contents/MacOS/rylus`.
- [ ] Add signing step to CI macOS job: import `.p12` into a temp keychain, then `codesign --deep --force --options=runtime --timestamp --entitlements packaging/macos/entitlements.plist --sign "Developer ID Application: …" Rylus.app`.
- [ ] Add notarization step: `xcrun notarytool submit Rylus.dmg --apple-id … --team-id … --password … --wait` followed by `xcrun stapler staple Rylus.app` and `xcrun stapler staple Rylus.dmg`.
- [ ] Replace the `zip` packaging step with `create-dmg` producing `Rylus-<ver>-universal.dmg` with a background image + drag-to-Applications UX.
- [ ] Update the release upload to publish `Rylus-<ver>-universal.dmg` (drop the old `macos-intel.zip`).
- [ ] Update `Readme.md` macOS install section: download DMG, drag to Applications, grant Screen Recording + Accessibility on first run.

---

## 3. Windows — Signed MSI Installer

**Status today**: CI cross-builds `rylus.exe` for `x86_64-pc-windows-msvc` on a self-hosted Windows runner and publishes a bare `rylus-windows.zip` on tag push (`.github/workflows/build.yml:147-182`). `FFMPEG_DIR=C:\ffmpeg-win64` is set at build time but **no DLLs are shipped** alongside the binary — installs on clean machines likely fail to run. No icon, no version resource, no installer, no code signing.

Decision: ship a signed `.msi` for direct download. Skip Microsoft Store / MSIX for now.

### Prerequisites
- [P][$] Purchase an Authenticode code-signing certificate (OV ~$200–400/yr, EV ~$300–600/yr from DigiCert/Sectigo/SSL.com). EV avoids SmartScreen reputation warm-up.
- [P] Export cert as `.pfx` with password; add as GitHub secrets: `WINDOWS_CERT_PFX` (base64), `WINDOWS_CERT_PASSWORD`.
- [P] Install WiX Toolset v4 on the `win11-ci` self-hosted runner (`dotnet tool install --global wix`).

### Engineering tasks
- [ ] Create `packaging/windows/rylus.ico` (multi-resolution: 16/32/48/64/128/256 px).
- [ ] Add `crates/rylus-server/build.rs` using the `winresource` crate to embed the icon + a version resource block (FileVersion, ProductVersion, CompanyName, FileDescription, ProductName, LegalCopyright) — only compiles on `target_os = "windows"`.
- [ ] Add `winresource` as a `[target.'cfg(windows)'.build-dependencies]` entry in `crates/rylus-server/Cargo.toml`.
- [ ] Create `packaging/windows/rylus.wxs` (WiX v4) defining:
  - ProductRef / package identity with UpgradeCode GUID
  - Install dir `%ProgramFiles%\Rylus\`
  - Files: `rylus.exe` + bundled FFmpeg DLLs (`avcodec-*.dll`, `avformat-*.dll`, `avutil-*.dll`, `swscale-*.dll`, `swresample-*.dll`)
  - Start Menu shortcut with icon
  - Uninstaller registration
  - Firewall rule for the Rylus server TCP port (WiX `FirewallException` extension)
- [ ] Update the Windows CI job:
  - Copy FFmpeg DLLs from `C:\ffmpeg-win64\bin` into a staging dir alongside `rylus.exe`
  - Run `wix build -arch x64 packaging/windows/rylus.wxs -o rylus-<ver>-x64.msi`
  - Decode `WINDOWS_CERT_PFX` secret to disk; run `signtool sign /fd SHA256 /tr http://timestamp.digicert.com /td SHA256 /f cert.pfx /p $env:WINDOWS_CERT_PASSWORD rylus.exe rylus-<ver>-x64.msi`
  - Publish the signed `.msi` (drop or keep the bare `.zip` as a portable fallback)
- [ ] Add a CI smoke test after build: install the `.msi` silently (`msiexec /i … /qn`), run `rylus --version`, uninstall. Catches missing DLLs and bad manifest before shipping.
- [ ] Update `Readme.md` Windows install section: download `.msi`, run installer, Start Menu shortcut.

---

## Cross-cutting follow-ups (all targets)

- [ ] Wire `clap_mangen` man-page generation once so AUR (Linux) and the Windows/macOS builds can share the text.
- [ ] Define a single source of truth for icon assets (`packaging/icons/rylus.svg`) and generate `.icns`, `.ico`, and `.png` sizes via a helper script so they stay in sync.
- [ ] Update `CHANGELOG.md` shipping notes as each channel goes live.
