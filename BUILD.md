# Audire Build & Packaging Guide

This project can use **Bun** in the Tauri config. You do **not** need to use npm for the Tauri hooks. The `beforeDevCommand` and `beforeBuildCommand` values can call Bun directly.

## Recommended Tauri config values

Use these build commands in `src-tauri/tauri.conf.json`:

```json
"build": {
  "beforeDevCommand": "bun run dev",
  "beforeBuildCommand": "bun run build",
  "devUrl": "http://localhost:5173",
  "frontendDist": "../dist"
}
```

## Important packaging rule

Build each platform **on that platform**:

- build **Windows** installers on Windows
- build **Linux** packages on Linux
- build **macOS** bundles on macOS

`targets: "all"` means “all bundle formats supported on the current platform”, not one command that produces Windows, Linux, and macOS bundles from a single machine.

---

## Common build command

From the project root:

```bash
bun tauri build
```

or:

```bash
bun run tauri build
```

Tauri uses the frontend output from `beforeBuildCommand`, then bundles the desktop app.

Final artifacts are usually written under:

```text
src-tauri/target/release/bundle/
```

---

## Windows build guide

Run this on a **Windows** machine.

### Requirements

- Rust toolchain
- Visual Studio C++ build tools / MSVC
- Bun
- WebView2 runtime available for users, or let Tauri install/bootstrap it
- Strawberry Perl if you keep `bundled-sqlcipher-vendored-openssl`

### Build command

```bat
cd C:\Repos\audire
bun tauri build
```

### Expected outputs

Usually under one or both of these folders:

```text
src-tauri\target\release\bundle\msi\
src-tauri\target\release\bundle\nsis\
```

Typical files:

- `.msi`
- `.exe` installer

### Best format to share

- `.exe` installer for the easiest Windows install flow
- `.msi` if your users or IT teams prefer MSI deployment

### Notes

- Unsigned Windows installers may trigger SmartScreen warnings.
- For public distribution, code signing is strongly recommended.

---

## Linux build guide

Run this on a **Linux** machine.

### Requirements

- Rust toolchain
- Bun
- distro packaging tools as needed for the target format

### Build command

```bash
cd /path/to/audire
bun tauri build
```

### Expected outputs

Usually under:

```text
src-tauri/target/release/bundle/appimage/
src-tauri/target/release/bundle/deb/
src-tauri/target/release/bundle/rpm/
```

Typical files:

- `.AppImage`
- `.deb`
- `.rpm`

### Best format to share

- **AppImage** for the widest portability
- **.deb** for Ubuntu/Debian users
- **.rpm** for Fedora/RHEL-based users

### Notes

- AppImage is often the easiest single-file option for Linux users.
- If `.rpm` is not produced, the required packaging tools may be missing on the build machine.

---

## macOS build guide

Run this on a **macOS** machine.

### Requirements

- Rust toolchain
- Xcode command line tools
- Bun
- Apple Developer signing setup for broad public distribution

### Build command

```bash
cd /path/to/audire
bun tauri build
```

### Expected outputs

Usually under:

```text
src-tauri/target/release/bundle/app/
src-tauri/target/release/bundle/dmg/
```

Typical files:

- `.app`
- `.dmg`

### Best format to share

- `.dmg` for end users
- `.app` for direct internal testing

### Notes

- Unsigned macOS apps often trigger Gatekeeper warnings.
- For external distribution, signing and notarization are strongly recommended.

---

## Recommended distribution choices

If you want one sensible default per platform:

- **Windows:** NSIS `.exe` and MSI
- **Linux:** AppImage and `.deb`
- **macOS:** `.dmg`

---

## Useful commands

### Development

```bash
bun tauri dev
```

### Production build

```bash
bun tauri build
```

### Debug bundling

```bash
bun tauri build --debug
```

---

## Suggested next step

After your first successful Windows package build, check:

```text
src-tauri/target/release/bundle/
```

and share or use the installer from the relevant subfolder.
