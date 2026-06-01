# Mihomo Server Manager

Local Tauri desktop app for managing `mihomo` on headless Linux servers over SSH.

## What is included

- React + TypeScript desktop UI with server list, health, install, subscription, node switching, config preview, and logs.
- Tauri/Rust backend commands for Windows `ssh.exe`/`scp.exe`, SQLite persistence, systemd service control, mihomo installation, subscription update, and controller API access through an SSH tunnel.
- Local redaction for subscription URLs, proxy node credentials, and SSH identity paths.

## Development

```bash
npm install
npm run build
npm test
```

To run the desktop app you also need the Rust toolchain and Tauri system prerequisites:

```bash
npm run desktop:dev
```

## Packaging

After installing Rust and Visual Studio Build Tools with the MSVC + Windows SDK components:

```bash
npm run package
```

The Windows installer will be written under:

```text
src-tauri/target/release/bundle/nsis/
```

## First-version assumptions

- Windows native desktop runtime.
- Servers are imported from the Windows SSH config.
- Remote servers are Ubuntu/Debian with systemd and root SSH access.
- Subscription URLs are stored only on the remote server at `/etc/mihomo/subscription.url`.
- Remote TUN mode is intentionally out of scope for v1.
