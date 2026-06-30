# 2026-06-30 - bootstrap subscription fallback

- Date: 2026-06-30
- Type: update
- Project: /mnt/d/projects/mihomo

## Why
- 新服务器第一次用受限订阅时，远端可能直连不了订阅域名，导致保存了订阅链接但没有节点。
- 部分订阅需要 `geoip.metadb` 或 `GeoSite.dat`，远端下载这些地理库超时时，`mihomo -t` 会失败。

## What Changed
- 订阅更新支持两种下载方式：普通更新继续走远端 `127.0.0.1:7890`，引导更新可直连下载。
- 当订阅更新失败且本地保存了 `helium` 订阅时，自动先用 `helium` 直连引导远端配置。
- `helium` 引导成功后，通过控制 API 优先选择日本/新加坡命名的可用节点，并把 `GLOBAL` 指向引导组或节点。
- 引导节点可用后，自动重试原订阅更新，让受限订阅可以借助 `helium` 下载。
- 当 `mihomo -t` 因 `GeoIP/MMDB/GeoSite` 下载失败时，本机下载缺失地理库并上传到远端后自动重试。
- 应用版本升到 `0.1.10`，用于发布标签。

## Files Changed
- `.github/workflows/release-windows.yml`
- `README.md`
- `package.json`
- `package-lock.json`
- `src-tauri/Cargo.toml`
- `src-tauri/Cargo.lock`
- `src-tauri/tauri.conf.json`
- `src-tauri/src/lib.rs`
- `src-tauri/src/mihomo.rs`

## Behavior Impact
- 新服务器更新非 `helium` 订阅失败时，会自动尝试 `helium` 引导和节点选择，然后重试原订阅。
- 如果没有本地 `helium` 订阅，或当前更新的就是 `helium`，行为保持原样。
- 更新过程中可能会从 GitHub 下载 `geoip.metadb` 或 `GeoSite.dat` 并上传到 `/etc/mihomo`。
- 操作日志会记录初次失败、`helium` 引导、节点选择和最终重试结果。

## Validation
- `cargo test --manifest-path src-tauri/Cargo.toml --locked`
- `npm run check`

## Risks
- 引导节点选择依赖节点名称和控制 API 延迟测试；如果 `helium` 没有日本/新加坡或其它可测速节点，引导会失败并保留错误日志。
- 地理库下载依赖 GitHub release asset 可访问。

## Follow-up
- 可以后续在 UI 上明确显示“订阅链接已保存”和“订阅配置已生效”的区别。
