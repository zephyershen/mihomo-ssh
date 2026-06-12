# 2026-06-12 - TUN 模式 SSH 安全开关

- Date: 2026-06-12
- Type: update
- Project: /mnt/d/projects/mihomo

## Why
- 需要在软件里直接打开/关闭 Mihomo TUN 模式。
- 打开 TUN 会改服务器路由，配置不当可能影响当前 SSH 连接，所以必须带备份和自动保护。

## What Changed
- 新增 TUN 状态模型，健康检查会返回当前 `tun` 配置。
- 新增 Tauri 命令 `set_mihomo_tun_enabled`，用于打开/关闭 TUN。
- 打开/关闭 TUN 前自动创建备份，备份原因分别是 `enable_tun` 和 `disable_tun`。
- 启用 TUN 时自动写入：
  - `tun.enable=true`
  - `tun.stack=system`，如果原配置没有设置
  - `tun.auto-route=true`
  - `tun.auto-detect-interface=true`
  - `tun.dns-hijack=["any:53"]`，如果原配置没有设置
  - `tun.route-exclude-address`，合并原有排除地址、常见内网地址、当前 SSH 对端地址和当前接口网段
- 写入前先用 `/usr/local/bin/mihomo -t` 检查新配置。
- 写入后重启 `mihomo`，并用 35 秒远端看门狗保护：如果 SSH 确认标记没有写入，会自动恢复旧配置并重启。
- 更新订阅时保留现有 `tun` 段，避免刷新订阅后 TUN 被覆盖。
- 前端“配置”页新增“TUN 模式”面板，显示状态、关键参数、保护条目，并提供打开/关闭按钮。
- 概览页把容易误解的 `Power` 改成“mihomo 服务”，按钮改成“启动/停止/重启”。
- mock API 增加 TUN 开关状态，方便普通浏览器预览。

## Files Changed
- `src-tauri/src/mihomo.rs`
- `src-tauri/src/lib.rs`
- `src-tauri/src/models.rs`
- `src/App.tsx`
- `src/lib/api.ts`
- `src/styles.css`
- `src/types.ts`

## Behavior Impact
- 打开 TUN 后，远端服务器的部分流量会进入 Mihomo TUN 路由。
- SSH 对端地址、常见内网地址和接口网段会被排除，降低 SSH 被代理路由影响的风险。
- 如果写入后 SSH 会话中断，远端看门狗会尝试自动恢复旧配置。
- 订阅更新会保留现有 TUN 配置。

## Validation
- Passed: `npm run check`
- Passed: frontend production build
- Passed: frontend tests, 2 tests
- Passed: `cargo fmt --manifest-path src-tauri/Cargo.toml --check`
- Passed: `cargo check --manifest-path src-tauri/Cargo.toml --locked`
- Passed: Rust tests, 17 tests
- Passed: `git diff --check`
- Not run: live remote TUN enable/disable smoke test, to avoid changing a live server route during local validation.

## Risks
- 不同 Linux 发行版、内核、Mihomo 版本对 TUN/路由支持可能不同。
- 如果服务器没有 TUN 设备、缺少权限、或 systemd/mihomo 行为异常，打开 TUN 可能失败。
- SSH 保护会尽量排除当前连接和内网地址，但公网云厂商的特殊路由策略仍需要真实机器验证。

## Follow-up
- 可以增加一次“打开 TUN 后重新建立 SSH 测试连接”的二次验证。
