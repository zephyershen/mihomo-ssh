# 2026-07-01 - node auto recovery and install flow

- Date: 2026-07-01
- Type: update
- Project: /mnt/d/projects/mihomo

## Why
- 订阅列表卡片的刷新按钮会切换当前编辑选中项，容易误操作。
- 安装/修复 mihomo 时顺手下载订阅，会让新服务器初始化流程和订阅选择混在一起。
- 节点页普通加载/测速会写很多操作日志，但真正需要记录的是当前节点失效和自动切换结果。

## What Changed
- 订阅卡片刷新只更新远端配置，不再改变当前选中的订阅卡片。
- 当前服务器还没有远端订阅时，在订阅页第一次选择订阅会自动更新远端订阅。
- 安装/修复入口完全忽略订阅 URL，只安装或修复 mihomo 和默认配置。
- 节点页增加当前节点监控状态，每 10 秒检查一次当前节点，连续失败满 30 秒后自动选择备用节点。
- 后端新增 `auto_recover_proxy_node` 命令，记录 `node_status` 和 `auto_select_proxy_node` 日志。
- 节点页普通打开通道、加载分组、测速、手动切换不再写操作日志。
- 版本升到 `0.1.11`，用于发布标签。

## Files Changed
- `.github/workflows/release-windows.yml`
- `README.md`
- `package.json`
- `package-lock.json`
- `src/App.tsx`
- `src/styles.css`
- `src/lib/api.ts`
- `src-tauri/Cargo.toml`
- `src-tauri/Cargo.lock`
- `src-tauri/src/lib.rs`
- `src-tauri/tauri.conf.json`

## Behavior Impact
- 点击非当前订阅卡片的刷新按钮，不会再切换表单里的订阅。
- 安装完成后需要到订阅页选择或刷新订阅，才会下载最新订阅配置。
- 节点页打开时会后台检查当前节点；当前节点连续失败约 30 秒后会尝试切到可用的国外节点。
- 操作日志中节点页只保留当前节点失效告警和自动选择结果。

## Validation
- `cargo test --manifest-path src-tauri/Cargo.toml --locked`
- `npm run check`

## Risks
- 自动切换依赖控制 API 的节点延迟测试；如果所有候选都不可用，会记录失败并保持当前节点。
- 节点页必须打开且有当前节点，自动监控才运行。

## Follow-up
- 可以后续把节点监控开关做成显式设置。
