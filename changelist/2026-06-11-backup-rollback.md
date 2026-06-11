# 2026-06-11 - 备份与回滚

- Date: 2026-06-11
- Type: update
- Project: /mnt/d/projects/mihomo

## Why
- 远端配置、订阅地址、远端代理环境变量一旦改坏，之前只能手工 SSH 到服务器修。
- 需要在安装、更新订阅、改远端代理、回滚前自动留一份备份，出问题时可以从软件里恢复。

## What Changed
- 新增远端备份模块，把关键文件保存到 `/etc/mihomo/manager-backups/<时间>-<原因>/`。
- 新增本地 SQLite 备份索引表，记录备份原因、标签、远端目录、文件是否存在、大小和 sha256。
- 新增 Tauri 命令：列出备份、手动创建备份、回滚备份、删除备份。
- 在安装/修复、更新订阅、保存远端代理、切换远端代理、回滚前自动创建备份。
- 回滚配置前会先用 `mihomo -t` 检查备份配置，避免恢复一个明显坏的配置。
- 每台服务器最多保留 20 条备份记录，超过后自动清理旧备份；远端删除失败时保留记录并标记 `delete_failed`。
- 前端新增“备份”页，可以创建、刷新、回滚、删除备份。
- 本地 mock API 增加备份数据，方便无 Tauri 环境预览。

## Files Changed
- `src-tauri/src/backup.rs`
- `src-tauri/src/lib.rs`
- `src-tauri/src/models.rs`
- `src-tauri/src/storage.rs`
- `src/App.tsx`
- `src/lib/api.ts`
- `src/styles.css`
- `src/types.ts`

## Behavior Impact
- 关键远端写操作前会多执行一次 SSH 备份命令，因此这些操作会比以前稍慢。
- 如果备份创建失败，安装、订阅更新、远端代理修改会停止，避免在没有退路的情况下继续改远端文件。
- 回滚会覆盖远端当前关键配置，并按备份记录删除当时不存在的文件。

## Validation
- Passed: `npm run check`
- Passed: frontend production build
- Passed: frontend tests, 2 tests
- Passed: `cargo fmt --manifest-path src-tauri/Cargo.toml --check`
- Passed: `cargo check --manifest-path src-tauri/Cargo.toml --locked`
- Passed: Rust tests, 14 tests
- Passed: `git diff --check`
- Not run: real remote server backup/restore smoke test, to avoid changing a live server during local validation.

## Risks
- 真实服务器上的文件权限或 systemd 行为如果和当前脚本假设不同，回滚可能需要按服务器环境微调。
- 备份文件保存在远端服务器本机，如果服务器磁盘损坏，本机备份也会一起丢失。

## Follow-up
- 可以再加一个导出备份到本机的功能，避免只依赖远端本机备份。
