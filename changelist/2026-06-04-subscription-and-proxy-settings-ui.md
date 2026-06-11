# 2026-06-04 - subscription and proxy settings UI

- Date: 2026-06-04
- Type: update
- Project: /mnt/d/projects/mihomo

## Why
- 订阅页有多个刷新入口，首次填写订阅时按钮文案也显得像必须手动刷新。
- 配置页同时展示关键信息、远端代理和 YAML 预览，远端代理管理不够集中。
- 配置页的“重启 Mihomo”与概览页的重启按钮控制同一个服务，容易误解。
- 现有界面偏蓝色控制台风格，用户希望更接近 shadcn 的简洁风格。
- 页面里的执行结果大文本块占空间，用户希望结果用软件弹窗提示，并在日志页集中保留记录。

## What Changed
- 移除订阅页顶部的“刷新远端保存”按钮。
- 将订阅编辑区主按钮改为“保存”，保留保存后自动更新远端订阅的行为。
- 将配置页改成单栏远端代理管理面板，移除 YAML Preview 和大块 Key Fields。
- 在远端代理面板中保留服务器、配置文件、检测到的环境变量、代理输入和启停操作。
- 移除配置页远端代理区域的“重启 Mihomo”按钮。
- 将全局样式调整为 shadcn 风格：中性色背景、细边框、轻阴影、黑色主按钮、低饱和 hover 和 focus 状态。
- 移除安装页、订阅页、配置页、日志页里的执行结果大文本块，命令结果改为底部弹窗提示。
- 日志页改为满宽操作记录列表，显示状态、操作、结果摘要和时间。
- 日志页改为读取全部操作记录，包含本地操作和远端服务器操作。
- 给节点组加载、测速、单节点测速、读取远端日志、外网测试补充后端操作日志摘要。

## Files Changed
- `src/App.tsx`
- `src/styles.css`
- `src-tauri/src/lib.rs`

## Behavior Impact
- 新订阅或编辑订阅时，用户只看到“保存”按钮；保存成功后仍会自动更新远端配置。
- 已保存订阅仍可通过每条订阅卡片右侧的刷新按钮更新远端。
- 配置页不再显示 YAML 预览，只聚焦远端服务器代理环境变量管理。
- `mihomo` 服务重启只保留在概览页，配置页只管理代理环境变量。
- 界面视觉更接近 shadcn 的中性组件风格，不改变已有数据和远端命令行为。
- 执行结果不再占页面空间；即时结果通过弹窗显示，详细排查看日志页记录。
- 日志页会显示每次操作的结果摘要，包含本地记录和远端服务器记录，便于 debug。

## Validation
- `npm run check`
- `cargo check` attempted, blocked because this WSL environment is missing `pkg-config` for Tauri GTK/GIO dependencies.
- `cargo fmt --check` attempted, but the repo has existing Rust formatting differences in unrelated files; left those files unchanged to avoid unrelated churn.

## Risks
- 未做真实远端 SSH 操作验证；本次验证了前端构建和现有单元测试。
- Rust 后端编译检查未完成，阻塞原因是本机缺少 `pkg-config`，不是代码检查报出的具体类型错误。

## Follow-up
- None.
