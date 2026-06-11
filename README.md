# Mihomo Server Manager

Local Tauri desktop app for managing `mihomo` on headless Linux servers over SSH.

## What is included

- React + TypeScript desktop UI with server list, health, install, subscription, node switching, remote proxy environment, config preview, and logs.
- Tauri/Rust backend commands for Windows `ssh.exe`/`scp.exe`, SQLite persistence, systemd service control, mihomo installation, subscription update, and controller API access through an SSH tunnel.
- Local redaction for subscription URLs, proxy node credentials, and SSH identity paths.

## Development

```bash
npm install
npm run check
```

If you switch between Windows `cmd` and WSL, reinstall dependencies in the
environment you are using. Native packages under `node_modules` are OS-specific,
so a `node_modules` directory created by Windows can miss Linux packages, and the
reverse can happen too.

To run the desktop app you also need the Rust toolchain and Tauri system prerequisites:

```bash
npm run desktop:dev
```

## Packaging

Local packaging requires Rust and Visual Studio Build Tools with the MSVC + Windows SDK components:

```bash
npm run package
```

The Windows installer will be written under:

```text
src-tauri/target/release/bundle/nsis/
```

The normal Windows installer is built on GitHub Actions when `main` is pushed:

```bash
git push origin main
```

For a signed release with updater metadata, push a version tag after configuring the updater secrets:

```bash
git tag v0.1.6
git push origin v0.1.6
```

Required GitHub secrets for release updates:

- `TAURI_UPDATER_PUBLIC_KEY`
- `TAURI_SIGNING_PRIVATE_KEY`
- `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` if the private key is password protected

Generate the updater key pair locally:

```bash
npm run tauri -- signer generate -w ~/.tauri/mihomo-server-manager.key
```

Use the printed public key as `TAURI_UPDATER_PUBLIC_KEY`. Use the private key file contents as `TAURI_SIGNING_PRIVATE_KEY`. Keep the private key backed up and never commit it.

The first updater-enabled release still has to be installed manually. Later release builds can be installed from inside the app through the Update tab.

## First-version assumptions

- Windows native desktop runtime.
- Servers are imported from the Windows SSH config.
- Remote servers are Ubuntu/Debian with systemd and root SSH access.
- Subscription profiles are stored locally; the selected URL is written to the remote server at `/etc/mihomo/subscription.url`.
- Remote TUN mode is intentionally out of scope for v1.
