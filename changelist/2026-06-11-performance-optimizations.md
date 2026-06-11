# 2026-06-11 - performance optimizations

- Date: 2026-06-11
- Type: update
- Project: /mnt/d/projects/mihomo

## Why
- 用户希望把当前 mihomo 管理软件的性能尽量优化到更高水平。
- 节点测速原来按节点一个个等待，节点多时会明显拖慢。
- 安装 mihomo 时解压会把整个文件读进内存，文件越大内存占用越高。
- 日志、订阅和服务器列表在数据变多时会产生额外绘制和重复格式化成本。

## What Changed
- 将节点组测速改为最多 8 个节点并发测速，并复用同一个 `reqwest::Client` 读取控制器和测速接口。
- 保留节点原始顺序，避免并发测速改变前端显示顺序。
- 将代理组排序改为缓存排序 key，减少排序时重复生成小写字符串。
- 安装包 `.gz` 解压改成流式拷贝，避免一次性把解压结果放进内存。
- GitHub Release 查询和下载复用同一个 HTTP client。
- 给 SQLite 连接增加 `busy_timeout`，并给按服务器读取操作日志增加索引。
- 前端缓存当前节点列表和日志时间格式化结果，减少重复计算。
- 给服务器、订阅和操作日志列表行加 `content-visibility`，让 WebView 可跳过屏幕外行的绘制。
- 增加 `futures-util` 直接依赖，用于受限并发测速。
- 将远端 SSH/SCP/解压等重同步工作放到后台线程，减少界面和异步运行时被阻塞的机会。

## Files Changed
- `src-tauri/Cargo.toml`
- `src-tauri/Cargo.lock`
- `src-tauri/src/controller.rs`
- `src-tauri/src/mihomo.rs`
- `src-tauri/src/storage.rs`
- `src/App.tsx`
- `src/styles.css`

## Behavior Impact
- 节点组测速在节点多时应明显更快；最坏耗时大致从“节点数 × 单节点等待”变为“分批并发等待”。
- 节点测速不会无限并发；上限为 8，避免同时打爆本地控制器。
- 安装/修复 mihomo 时峰值内存更低。
- 日志查询在记录多时更稳定，遇到短时间数据库锁时更不容易立刻失败。
- 前端显示内容不变，只减少重复计算和屏幕外绘制。
- 远端命令执行期间，应用其他异步任务更不容易被同步 SSH/SCP 操作拖住。

## Validation
- `npm run check`
- `rustfmt --edition 2021 --check src-tauri/src/controller.rs src-tauri/src/mihomo.rs src-tauri/src/storage.rs`
- `cargo fmt --manifest-path src-tauri/Cargo.toml --check`
- `cargo metadata --manifest-path src-tauri/Cargo.toml --locked --no-deps`
- `cargo check --manifest-path src-tauri/Cargo.toml --locked`
- `cargo test --manifest-path src-tauri/Cargo.toml --locked`

## Risks
- 未做真实远端服务器测速对比。
- 并发测速上限固定为 8；如果远端控制器非常弱，可以再降到 4。

## Follow-up
- 用一个真实节点数较多的订阅，记录优化前后整组测速耗时。
