# 2026-06-11 - code quality review fixes

- Date: 2026-06-11
- Type: fix
- Project: /mnt/d/projects/mihomo

## Why
- 用户要求启动多个 agent review 项目代码逻辑，并修复代码质量不合格的问题。
- 前端存在跨服务器异步结果覆盖、旧节点状态残留、敏感信息直接展示等问题。
- 后端命令边界缺少订阅 URL 校验，SSH 进程输出较多时有管道阻塞风险。
- 工程检查缺少 Rust CI，Vite 配置未纳入类型检查，Vitest 版本有 npm audit 告警。

## What Changed
- 启动 3 个子 agent 分别审查前端逻辑、Rust/Tauri 后端逻辑、工程/依赖/CI。
- 切换服务器时清空健康状态、节点组、测速结果、远端代理配置和外网测试结果，避免旧服务器数据残留。
- 给前端异步请求补当前服务器/当前分组校验，旧请求返回后不会覆盖新服务器状态。
- `run` 增加递增 token，避免旧操作结束时提前清空新操作的 busy 状态。
- `markSubscriptionUsed` 只更新订阅列表，不再异步覆盖当前编辑草稿。
- 服务器列表手动刷新改走统一 `run` 错误提示；日志刷新失败时保留旧日志并显示提示。
- 扩展前端脱敏逻辑，覆盖代理 URL、订阅参数、Windows/Linux SSH key 路径，并在外网测试 title、远端代理环境变量、操作日志中使用脱敏展示。
- SSH 命令 stdout/stderr 改为后台线程读取，避免命令输出较多时管道填满导致子进程卡住。
- 后端 `install_or_repair_mihomo` 和 `update_subscription` 入口复用订阅 URL 校验，只允许 http/https 且拒绝空格/控制字符。
- 远端已有订阅文件在 curl 下载前也校验 scheme 和空白字符。
- mihomo 安装下载使用带超时的 HTTP client，并限制下载归档最大 80 MiB。
- SSH 隧道增加 `ExitOnForwardFailure=yes`，控制器 ready 检查要求 `/version` 返回成功且包含 version 字段。
- `TunnelRegistry` 增加 `Drop` 清理，应用退出时 kill/wait 剩余 SSH 隧道进程。
- 远端代理启停不再把 inspect 失败当默认配置，避免连接/权限错误时覆盖远端配置。
- 升级 `vitest`，补 `@types/node`，`npm audit` 归零。
- 构建脚本增加 `vite.config.ts` 的 no-emit 类型检查；忽略 TypeScript build info 缓存。
- CI 增加 Rust format/check/test job，并安装 Tauri Linux 依赖。
- 发布 workflow 和 README 的示例 tag 更新到当前版本 `v0.1.6`。
- 将直接依赖 `reqwest` 升到 0.13，避免项目直接依赖和 Tauri 依赖使用两套主版本。
- 将废弃的 `serde_yaml` 替换为维护中的 `yaml_serde`，订阅配置补丁逻辑保持不变。
- `npm run check` 现在同时运行前端和 Rust 检查，`check:frontend`/`check:rust` 可单独使用。
- 重 SSH/ssh-keygen/SCP/文件解压写入操作改到后台线程执行，避免阻塞异步运行时。
- SSH 密码引导不再把密码值放进子进程环境变量，改为 0600 临时文件，进程结束后随临时目录删除。
- mihomo Release 下载要求 GitHub API 返回 `sha256:` digest，并在写入前校验归档内容。

## Files Changed
- `.github/workflows/ci.yml`
- `.github/workflows/release-windows.yml`
- `.gitignore`
- `README.md`
- `package.json`
- `package-lock.json`
- `tsconfig.node.json`
- `src/App.tsx`
- `src/lib/redaction.ts`
- `src/lib/redaction.test.ts`
- `src-tauri/Cargo.toml`
- `src-tauri/Cargo.lock`
- `src-tauri/src/controller.rs`
- `src-tauri/src/lib.rs`
- `src-tauri/src/mihomo.rs`
- `src-tauri/src/redaction.rs`
- `src-tauri/src/remote_proxy.rs`
- `src-tauri/src/ssh.rs`
- `changelist/2026-06-11-code-quality-review-fixes.md`

## Behavior Impact
- 切换服务器后不会继续显示或操作上一个服务器的节点、健康状态、远端代理状态。
- 旧异步请求返回时会被丢弃，不会污染当前服务器界面。
- 页面和日志列表展示更少敏感信息。
- SSH 长输出命令更不容易卡住。
- 非 http/https 或带空白字符的订阅 URL 会在后端被拒绝。
- 远端代理启停遇到读取失败会报错，不会自动写入默认配置覆盖远端状态。
- mihomo 安装包下载内容必须通过 GitHub Release `sha256` 校验。
- CI 会覆盖 Rust 代码格式、编译检查和测试。

## Validation
- `npm run check`
- `npx tsc --noEmit -p tsconfig.node.json`
- `npm audit --audit-level=moderate`
- `cargo fmt --manifest-path src-tauri/Cargo.toml --check`
- `cargo metadata --manifest-path src-tauri/Cargo.toml --locked --no-deps`
- `cargo metadata --manifest-path src-tauri/Cargo.toml --format-version 1`
- `cargo check --manifest-path src-tauri/Cargo.toml --locked`
- `cargo test --manifest-path src-tauri/Cargo.toml --locked`

## Risks
- 未在真实远端服务器上验证 SSH 隧道、订阅更新和代理启停。
- 如果 GitHub Release API 没有返回资产 `sha256:` digest，安装/修复会拒绝继续。

## Follow-up
- 用真实远端服务器跑一遍安装/修复、订阅更新、远端代理启停和节点测速。
