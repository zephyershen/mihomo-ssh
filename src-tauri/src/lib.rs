mod backup;
mod controller;
mod mihomo;
mod models;
mod redaction;
mod remote_proxy;
mod ssh;
mod storage;

use std::{path::PathBuf, time::Duration};

use models::{
    BackupSnapshot, CommandResult, EgressTestResult, InstallOptions, ManagedSshKeyInfo,
    ManualServerInput, OperationLog, ProxyGroup, ProxyNode, RemoteProxyConfig, RemoteProxyInput,
    Server, ServerBootstrapInput, ServerHealth, ServiceCommandResult, SubscriptionInput,
    SubscriptionProfile, SubscriptionUpdateOptions, TunnelInfo,
};
use reqwest::Url;
use storage::Storage;
use tauri::{Manager, State};

const BOOTSTRAP_SUBSCRIPTION_NAME: &str = "helium";

pub struct AppState {
    storage: Storage,
    tunnels: controller::TunnelRegistry,
    app_dir: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct BootstrapNodeCandidate {
    group: String,
    node: String,
    preferred_region: bool,
}

#[tauri::command]
fn list_servers(state: State<'_, AppState>) -> Result<Vec<Server>, String> {
    state.storage.list_servers()
}

#[tauri::command]
fn import_ssh_hosts(state: State<'_, AppState>) -> Result<Vec<Server>, String> {
    let hosts = ssh::importable_hosts_from_default_config()?;
    let servers = state.storage.upsert_imported_hosts(&hosts)?;
    state.storage.add_log(
        None,
        "import_ssh_hosts",
        "ok",
        &format!("imported {} SSH hosts", hosts.len()),
    )?;
    Ok(servers)
}

#[tauri::command]
async fn get_managed_ssh_key(state: State<'_, AppState>) -> Result<ManagedSshKeyInfo, String> {
    let app_dir = state.app_dir.clone();
    run_blocking(move || ssh::ensure_managed_key(&app_dir)).await
}

#[tauri::command]
async fn add_manual_server(
    state: State<'_, AppState>,
    input: ManualServerInput,
) -> Result<Vec<Server>, String> {
    let storage = state.storage.clone();
    let app_dir = state.app_dir.clone();
    run_blocking(move || {
        let input = normalize_manual_server_input(input)?;
        let key = ssh::ensure_managed_key(&app_dir)?;
        let private_key = ssh::managed_private_key_path(&app_dir);
        let servers = storage.upsert_manual_server(&input, &private_key, &key.private_key_hint)?;
        storage.add_log(
            None,
            "add_manual_server",
            "ok",
            &format!(
                "added {}@{}:{}",
                input.user,
                input.host_name,
                input.port.unwrap_or(22)
            ),
        )?;
        Ok(servers)
    })
    .await
}

#[tauri::command]
async fn bootstrap_server_with_password(
    state: State<'_, AppState>,
    input: ServerBootstrapInput,
) -> Result<Vec<Server>, String> {
    let storage = state.storage.clone();
    let app_dir = state.app_dir.clone();
    run_blocking(move || {
        let input = normalize_bootstrap_input(input)?;
        let key = ssh::ensure_managed_key(&app_dir)?;
        let result =
            ssh::bootstrap_authorized_key(&input, &key.public_key, Duration::from_secs(25))?;
        if !result.ok {
            storage.add_log(
                None,
                "bootstrap_server_with_password",
                "error",
                command_summary(&result).as_str(),
            )?;
            return Err(command_summary(&result));
        }

        let manual_input = ManualServerInput {
            display_name: input.display_name,
            host_name: input.host_name,
            user: input.user,
            port: input.port,
        };
        let private_key = ssh::managed_private_key_path(&app_dir);
        let servers =
            storage.upsert_manual_server(&manual_input, &private_key, &key.private_key_hint)?;
        storage.add_log(
            None,
            "bootstrap_server_with_password",
            "ok",
            &format!(
                "installed managed key on {}@{}:{}",
                manual_input.user,
                manual_input.host_name,
                manual_input.port.unwrap_or(22)
            ),
        )?;
        Ok(servers)
    })
    .await
}

#[tauri::command]
fn delete_server(state: State<'_, AppState>, server_id: i64) -> Result<Vec<Server>, String> {
    let server = state.storage.get_server(server_id)?;
    let _ = state.tunnels.close(server_id);
    let servers = state.storage.delete_server(server_id)?;
    state.storage.add_log(
        None,
        "delete_server",
        "ok",
        &format!("removed local entry {}", server.alias),
    )?;
    Ok(servers)
}

#[tauri::command]
fn list_operation_logs(
    state: State<'_, AppState>,
    server_id: Option<i64>,
    limit: Option<u32>,
) -> Result<Vec<OperationLog>, String> {
    state.storage.list_logs(server_id, limit.unwrap_or(120))
}

#[tauri::command]
fn list_backups(state: State<'_, AppState>, server_id: i64) -> Result<Vec<BackupSnapshot>, String> {
    state.storage.list_backup_snapshots(server_id)
}

#[tauri::command]
async fn create_backup(
    state: State<'_, AppState>,
    server_id: i64,
    label: Option<String>,
) -> Result<BackupSnapshot, String> {
    let storage = state.storage.clone();
    let server = storage.get_server(server_id)?;
    create_indexed_backup(storage, server, "manual".to_string(), label).await
}

#[tauri::command]
async fn restore_backup(
    state: State<'_, AppState>,
    server_id: i64,
    backup_id: i64,
) -> Result<CommandResult, String> {
    let storage = state.storage.clone();
    let server = storage.get_server(server_id)?;
    let snapshot = storage.get_backup_snapshot(server_id, backup_id)?;
    create_indexed_backup(
        storage.clone(),
        server.clone(),
        "pre_restore".to_string(),
        Some(format!("回滚备份 {backup_id} 前")),
    )
    .await?;
    let result = run_blocking({
        let server = server.clone();
        let snapshot = snapshot.clone();
        move || backup::restore(&server, &snapshot)
    })
    .await?;
    log_command(&storage, server_id, "restore_backup", &result)?;
    Ok(result)
}

#[tauri::command]
async fn delete_backup(
    state: State<'_, AppState>,
    server_id: i64,
    backup_id: i64,
) -> Result<CommandResult, String> {
    let storage = state.storage.clone();
    let server = storage.get_server(server_id)?;
    let snapshot = storage.get_backup_snapshot(server_id, backup_id)?;
    let result = match run_blocking({
        let server = server.clone();
        let remote_dir = snapshot.remote_dir.clone();
        move || backup::delete_remote(&server, &remote_dir)
    })
    .await
    {
        Ok(result) => result,
        Err(err) => CommandResult {
            ok: false,
            code: None,
            stdout: String::new(),
            stderr: err,
        },
    };
    if result.ok {
        storage.delete_backup_record(backup_id)?;
    } else {
        storage.update_backup_status(backup_id, "delete_failed")?;
    }
    log_command(&storage, server_id, "delete_backup", &result)?;
    Ok(result)
}

#[tauri::command]
fn list_subscriptions(state: State<'_, AppState>) -> Result<Vec<SubscriptionProfile>, String> {
    state.storage.list_subscriptions()
}

#[tauri::command]
fn save_subscription(
    state: State<'_, AppState>,
    input: SubscriptionInput,
) -> Result<SubscriptionProfile, String> {
    let input = normalize_subscription_input(input)?;
    let profile = state.storage.save_subscription(&input)?;
    state.storage.add_log(
        None,
        "save_subscription",
        "ok",
        &format!("saved {}", profile.name),
    )?;
    Ok(profile)
}

#[tauri::command]
fn delete_subscription(
    state: State<'_, AppState>,
    subscription_id: i64,
) -> Result<Vec<SubscriptionProfile>, String> {
    let profile = state.storage.get_subscription(subscription_id)?;
    let next = state.storage.delete_subscription(subscription_id)?;
    state.storage.add_log(
        None,
        "delete_subscription",
        "ok",
        &format!("removed {}", profile.name),
    )?;
    Ok(next)
}

#[tauri::command]
fn mark_subscription_used(
    state: State<'_, AppState>,
    subscription_id: i64,
) -> Result<SubscriptionProfile, String> {
    state.storage.mark_subscription_used(subscription_id)
}

#[tauri::command]
async fn test_connection(
    state: State<'_, AppState>,
    server_id: i64,
) -> Result<CommandResult, String> {
    let storage = state.storage.clone();
    run_blocking(move || {
        let server = storage.get_server(server_id)?;
        let result = ssh::run_ssh_script(&server, "true", Duration::from_secs(12))?;
        let status = if result.ok { "online" } else { "offline" };
        storage.update_status(server_id, status)?;
        storage.add_log(
            Some(server_id),
            "test_connection",
            status,
            command_summary(&result).as_str(),
        )?;
        Ok(result)
    })
    .await
}

#[tauri::command]
async fn test_server_egress(
    state: State<'_, AppState>,
    server_id: i64,
    url: String,
) -> Result<EgressTestResult, String> {
    let storage = state.storage.clone();
    run_blocking(move || {
        let server = storage.get_server(server_id)?;
        let url = normalize_test_url(&url)?;
        let script = format!(
            r#"
set -e
url={url}
if command -v curl >/dev/null 2>&1; then
  curl -L --max-time 12 -o /dev/null -sS -w 'status=%{{http_code}}
elapsed=%{{time_total}}
remote_ip=%{{remote_ip}}
' "$url"
elif command -v wget >/dev/null 2>&1; then
  start=$(date +%s 2>/dev/null || echo 0)
  wget -T 12 -q --spider "$url"
  end=$(date +%s 2>/dev/null || echo "$start")
  echo "status=reachable"
  echo "elapsed=$((end - start))"
else
  echo "curl or wget is required on the remote server" >&2
  exit 127
fi
"#,
            url = shell_quote(&url)
        );
        let result = ssh::run_ssh_script(&server, &script, Duration::from_secs(18))?;
        let parsed = parse_egress_output(&url, result);
        let status = if parsed.ok { "ok" } else { "error" };
        storage.add_log(
            Some(server_id),
            "test_server_egress",
            status,
            &format!(
                "status={} elapsed={} url={}",
                parsed.status.as_deref().unwrap_or("-"),
                parsed
                    .elapsed_ms
                    .map(|value| format!("{value}ms"))
                    .unwrap_or_else(|| "-".to_string()),
                redaction::redact(&parsed.url)
            ),
        )?;
        Ok(parsed)
    })
    .await
}

#[tauri::command]
async fn inspect_server(
    state: State<'_, AppState>,
    server_id: i64,
) -> Result<ServerHealth, String> {
    let server = state.storage.get_server(server_id)?;
    match mihomo::inspect_server(&server).await {
        Ok(mut health) => {
            health.config_preview = mihomo::read_remote_config(&server).await.ok();
            health.tun = mihomo::inspect_tun_config(&server).await.ok().flatten();
            state.storage.update_status(server_id, "online")?;
            state
                .storage
                .add_log(Some(server_id), "inspect_server", "ok", "health refreshed")?;
            Ok(health)
        }
        Err(err) => {
            state.storage.update_status(server_id, "error")?;
            state
                .storage
                .add_log(Some(server_id), "inspect_server", "error", &err)?;
            Err(err)
        }
    }
}

#[tauri::command]
async fn install_or_repair_mihomo(
    state: State<'_, AppState>,
    server_id: i64,
    options: Option<InstallOptions>,
) -> Result<CommandResult, String> {
    let server = state.storage.get_server(server_id)?;
    let _ = options;
    create_indexed_backup(
        state.storage.clone(),
        server.clone(),
        "install".to_string(),
        Some("安装或修复前".to_string()),
    )
    .await?;
    let result = mihomo::install_or_repair(
        &server,
        InstallOptions {
            subscription_url: None,
        },
    )
    .await?;
    log_command(
        &state.storage,
        server_id,
        "install_or_repair_mihomo",
        &result,
    )?;
    Ok(result)
}

#[tauri::command]
async fn update_subscription(
    state: State<'_, AppState>,
    server_id: i64,
    options: Option<SubscriptionUpdateOptions>,
) -> Result<CommandResult, String> {
    let server = state.storage.get_server(server_id)?;
    let options =
        normalize_subscription_update_options(options.unwrap_or(SubscriptionUpdateOptions {
            subscription_url: None,
        }))?;
    create_indexed_backup(
        state.storage.clone(),
        server.clone(),
        "update_subscription".to_string(),
        Some("更新订阅前".to_string()),
    )
    .await?;
    let mut result = mihomo::update_subscription(&server, options.clone()).await?;
    if !result.ok {
        if let Some(bootstrap) =
            find_bootstrap_subscription(&state.storage, options.subscription_url.as_deref())?
        {
            log_command(
                &state.storage,
                server_id,
                "update_subscription_initial",
                &result,
            )?;
            result = retry_subscription_with_bootstrap(
                &state, &server, server_id, &options, bootstrap, result,
            )
            .await?;
        }
    }
    log_command(&state.storage, server_id, "update_subscription", &result)?;
    Ok(result)
}

#[tauri::command]
async fn set_mihomo_service(
    state: State<'_, AppState>,
    server_id: i64,
    service_state: String,
) -> Result<ServiceCommandResult, String> {
    let storage = state.storage.clone();
    run_blocking(move || {
        let server = storage.get_server(server_id)?;
        let result = mihomo::set_service(&server, &service_state)?;
        log_command(
            &storage,
            server_id,
            &format!("service:{service_state}"),
            &result.output,
        )?;
        Ok(result)
    })
    .await
}

#[tauri::command]
async fn set_mihomo_tun_enabled(
    state: State<'_, AppState>,
    server_id: i64,
    enabled: bool,
) -> Result<CommandResult, String> {
    let storage = state.storage.clone();
    let server = storage.get_server(server_id)?;
    create_indexed_backup(
        storage.clone(),
        server.clone(),
        if enabled {
            "enable_tun".to_string()
        } else {
            "disable_tun".to_string()
        },
        Some(if enabled {
            "打开 TUN 前".to_string()
        } else {
            "关闭 TUN 前".to_string()
        }),
    )
    .await?;
    let result = mihomo::set_tun_enabled(&server, enabled).await?;
    log_command(
        &storage,
        server_id,
        if enabled {
            "enable_mihomo_tun"
        } else {
            "disable_mihomo_tun"
        },
        &result,
    )?;
    Ok(result)
}

#[tauri::command]
async fn inspect_remote_proxy(
    state: State<'_, AppState>,
    server_id: i64,
) -> Result<RemoteProxyConfig, String> {
    let storage = state.storage.clone();
    run_blocking(move || {
        let server = storage.get_server(server_id)?;
        let result = remote_proxy::inspect(&server)?;
        storage.add_log(Some(server_id), "inspect_remote_proxy", "ok", "loaded")?;
        Ok(result)
    })
    .await
}

#[tauri::command]
async fn save_remote_proxy(
    state: State<'_, AppState>,
    server_id: i64,
    input: RemoteProxyInput,
) -> Result<CommandResult, String> {
    let storage = state.storage.clone();
    run_blocking(move || {
        let server = storage.get_server(server_id)?;
        create_indexed_backup_sync(
            &storage,
            &server,
            "save_remote_proxy",
            Some("保存远端代理前"),
        )?;
        let result = remote_proxy::save(&server, input)?;
        log_command(&storage, server_id, "save_remote_proxy", &result)?;
        Ok(result)
    })
    .await
}

#[tauri::command]
async fn set_remote_proxy_enabled(
    state: State<'_, AppState>,
    server_id: i64,
    enabled: bool,
) -> Result<CommandResult, String> {
    let storage = state.storage.clone();
    run_blocking(move || {
        let server = storage.get_server(server_id)?;
        create_indexed_backup_sync(
            &storage,
            &server,
            "toggle_remote_proxy",
            Some("切换远端代理前"),
        )?;
        let result = remote_proxy::set_enabled(&server, enabled)?;
        log_command(
            &storage,
            server_id,
            if enabled {
                "enable_remote_proxy"
            } else {
                "disable_remote_proxy"
            },
            &result,
        )?;
        Ok(result)
    })
    .await
}

#[tauri::command]
fn open_controller_tunnel(
    state: State<'_, AppState>,
    server_id: i64,
) -> Result<TunnelInfo, String> {
    let server = state.storage.get_server(server_id)?;
    state.tunnels.open(&server)
}

#[tauri::command]
fn close_controller_tunnel(
    state: State<'_, AppState>,
    server_id: i64,
) -> Result<TunnelInfo, String> {
    state.tunnels.close(server_id)
}

#[tauri::command]
async fn list_proxy_groups(
    state: State<'_, AppState>,
    server_id: i64,
) -> Result<Vec<ProxyGroup>, String> {
    let port = ensure_tunnel(&state, server_id)?;
    controller::list_proxy_groups(port).await
}

#[tauri::command]
async fn select_proxy_node(
    state: State<'_, AppState>,
    server_id: i64,
    group: String,
    node: String,
) -> Result<Vec<ProxyGroup>, String> {
    let port = ensure_tunnel(&state, server_id)?;
    controller::select_proxy_node(port, &group, &node).await?;
    controller::list_proxy_groups(port).await
}

#[tauri::command]
async fn measure_proxy_delay(
    state: State<'_, AppState>,
    server_id: i64,
    group: String,
) -> Result<Vec<ProxyNode>, String> {
    let port = ensure_tunnel(&state, server_id)?;
    controller::measure_proxy_delay(port, &group).await
}

#[tauri::command]
async fn measure_proxy_node_delay(
    state: State<'_, AppState>,
    server_id: i64,
    node: String,
) -> Result<ProxyNode, String> {
    let port = ensure_tunnel(&state, server_id)?;
    controller::measure_proxy_node_delay(port, &node).await
}

#[tauri::command]
async fn auto_recover_proxy_node(
    state: State<'_, AppState>,
    server_id: i64,
    group: String,
    current_node: String,
    failed_for_seconds: u32,
) -> Result<Vec<ProxyGroup>, String> {
    let port = ensure_tunnel(&state, server_id)?;
    let failed_for_seconds = failed_for_seconds.max(30);
    state.storage.add_log(
        Some(server_id),
        "node_status",
        "warning",
        &format!("group={group} node={current_node} failed_for={failed_for_seconds}s"),
    )?;

    let groups = controller::list_proxy_groups(port).await?;
    let candidates = recovery_node_candidates(&groups, &group, &current_node);
    for candidate in candidates.iter().take(24) {
        let measured = controller::measure_proxy_node_delay(port, &candidate.node).await?;
        if measured.alive == Some(false) || measured.delay_ms.is_none() {
            continue;
        }

        controller::select_proxy_node(port, &candidate.group, &candidate.node).await?;
        let delay = measured
            .delay_ms
            .map(|value| format!("{value}ms"))
            .unwrap_or_else(|| "-".to_string());
        state.storage.add_log(
            Some(server_id),
            "auto_select_proxy_node",
            "warning",
            &format!(
                "group={} from={} to={} delay={delay}",
                candidate.group, current_node, candidate.node
            ),
        )?;
        return controller::list_proxy_groups(port).await;
    }

    state.storage.add_log(
        Some(server_id),
        "auto_select_proxy_node",
        "error",
        &format!("group={group} from={current_node} no available foreign node"),
    )?;
    Err("当前节点连续失效，但没有找到可用的国外节点".to_string())
}

#[tauri::command]
async fn read_mihomo_logs(
    state: State<'_, AppState>,
    server_id: i64,
    lines: Option<u32>,
) -> Result<String, String> {
    let storage = state.storage.clone();
    run_blocking(move || {
        let server = storage.get_server(server_id)?;
        let lines = lines.unwrap_or(200);
        let logs = mihomo::read_logs(&server, lines)?;
        storage.add_log(
            Some(server_id),
            "read_mihomo_logs",
            "ok",
            &format!("read {} lines", logs.lines().count().min(lines as usize)),
        )?;
        Ok(logs)
    })
    .await
}

#[tauri::command]
async fn read_mihomo_config(state: State<'_, AppState>, server_id: i64) -> Result<String, String> {
    let server = state.storage.get_server(server_id)?;
    mihomo::read_remote_config(&server).await
}

fn ensure_tunnel(state: &State<'_, AppState>, server_id: i64) -> Result<u16, String> {
    if let Some(port) = state.tunnels.port(server_id)? {
        return Ok(port);
    }
    let server = state.storage.get_server(server_id)?;
    let info = state.tunnels.open(&server)?;
    Ok(info.local_port)
}

fn find_bootstrap_subscription(
    storage: &Storage,
    primary_url: Option<&str>,
) -> Result<Option<SubscriptionProfile>, String> {
    let primary_url = primary_url.map(str::trim).filter(|value| !value.is_empty());
    Ok(storage.list_subscriptions()?.into_iter().find(|profile| {
        profile
            .name
            .eq_ignore_ascii_case(BOOTSTRAP_SUBSCRIPTION_NAME)
            && primary_url
                .map(|url| profile.url.trim() != url)
                .unwrap_or(true)
    }))
}

async fn retry_subscription_with_bootstrap(
    state: &State<'_, AppState>,
    server: &Server,
    server_id: i64,
    options: &SubscriptionUpdateOptions,
    bootstrap: SubscriptionProfile,
    initial: CommandResult,
) -> Result<CommandResult, String> {
    let bootstrap_result = mihomo::update_subscription_direct(
        server,
        SubscriptionUpdateOptions {
            subscription_url: Some(bootstrap.url),
        },
    )
    .await?;
    log_command(
        &state.storage,
        server_id,
        "bootstrap_subscription",
        &bootstrap_result,
    )?;
    if !bootstrap_result.ok {
        return Ok(merge_bootstrap_failure(initial, bootstrap_result));
    }

    let selected = match select_bootstrap_node(state, server_id).await {
        Ok(message) => {
            state
                .storage
                .add_log(Some(server_id), "bootstrap_select_node", "ok", &message)?;
            message
        }
        Err(err) => {
            state
                .storage
                .add_log(Some(server_id), "bootstrap_select_node", "error", &err)?;
            return Ok(merge_bootstrap_selection_failure(
                initial,
                bootstrap_result,
                err,
            ));
        }
    };

    let retry = mihomo::update_subscription(server, options.clone()).await?;
    Ok(merge_bootstrap_retry_result(
        initial,
        bootstrap_result,
        &selected,
        retry,
    ))
}

async fn select_bootstrap_node(
    state: &State<'_, AppState>,
    server_id: i64,
) -> Result<String, String> {
    let port = ensure_tunnel(state, server_id)?;
    let groups = controller::list_proxy_groups(port).await?;
    let candidates = bootstrap_node_candidates(&groups);
    if candidates.is_empty() {
        return Err("helium 配置里没有可选的代理节点".to_string());
    }

    for candidate in candidates.iter().take(24) {
        let measured = controller::measure_proxy_node_delay(port, &candidate.node).await?;
        if measured.alive == Some(false) || measured.delay_ms.is_none() {
            continue;
        }

        controller::select_proxy_node(port, &candidate.group, &candidate.node).await?;
        let global = select_global_bootstrap_target(port, &groups, candidate).await?;
        let delay = measured
            .delay_ms
            .map(|value| format!("{value}ms"))
            .unwrap_or_else(|| "-".to_string());
        return Ok(match global {
            Some(global_target) => format!(
                "selected {} in {}; GLOBAL -> {}; delay={delay}",
                candidate.node, candidate.group, global_target
            ),
            None => format!(
                "selected {} in {}; delay={delay}",
                candidate.node, candidate.group
            ),
        });
    }

    Err("没有日本/新加坡或其它候选节点通过延迟测试".to_string())
}

async fn select_global_bootstrap_target(
    port: u16,
    groups: &[ProxyGroup],
    candidate: &BootstrapNodeCandidate,
) -> Result<Option<String>, String> {
    if candidate.group.eq_ignore_ascii_case("GLOBAL") {
        return Ok(Some(candidate.node.clone()));
    }

    let Some(global) = groups
        .iter()
        .find(|group| group.name.eq_ignore_ascii_case("GLOBAL"))
    else {
        return Ok(None);
    };

    if global.nodes.iter().any(|node| node.name == candidate.group) {
        controller::select_proxy_node(port, &global.name, &candidate.group).await?;
        return Ok(Some(candidate.group.clone()));
    }
    if global.nodes.iter().any(|node| node.name == candidate.node) {
        controller::select_proxy_node(port, &global.name, &candidate.node).await?;
        return Ok(Some(candidate.node.clone()));
    }
    Ok(None)
}

fn bootstrap_node_candidates(groups: &[ProxyGroup]) -> Vec<BootstrapNodeCandidate> {
    let mut candidates = Vec::new();
    for group in groups {
        for node in &group.nodes {
            if !is_leaf_proxy_node(node) {
                continue;
            }
            candidates.push(BootstrapNodeCandidate {
                group: group.name.clone(),
                node: node.name.clone(),
                preferred_region: is_preferred_bootstrap_region(&node.name),
            });
        }
    }
    candidates.sort_by_key(|candidate| {
        (
            !candidate.preferred_region,
            candidate.group.eq_ignore_ascii_case("GLOBAL"),
            candidate.group.to_ascii_lowercase(),
            candidate.node.to_ascii_lowercase(),
        )
    });
    candidates
}

fn recovery_node_candidates(
    groups: &[ProxyGroup],
    group_name: &str,
    current_node: &str,
) -> Vec<BootstrapNodeCandidate> {
    let mut candidates =
        recovery_node_candidates_with_mode(groups, group_name, current_node, false);
    if candidates.is_empty() {
        candidates = recovery_node_candidates_with_mode(groups, group_name, current_node, true);
    }
    candidates.sort_by_key(|candidate| {
        (
            !candidate.preferred_region,
            candidate.group.to_ascii_lowercase(),
            candidate.node.to_ascii_lowercase(),
        )
    });
    candidates
}

fn recovery_node_candidates_with_mode(
    groups: &[ProxyGroup],
    group_name: &str,
    current_node: &str,
    allow_strategy_nodes: bool,
) -> Vec<BootstrapNodeCandidate> {
    groups
        .iter()
        .filter(|group| group.name == group_name)
        .flat_map(|group| {
            group.nodes.iter().filter_map(move |node| {
                if node.name == current_node
                    || !is_recovery_candidate_node(node, allow_strategy_nodes)
                {
                    return None;
                }
                Some(BootstrapNodeCandidate {
                    group: group.name.clone(),
                    node: node.name.clone(),
                    preferred_region: is_preferred_bootstrap_region(&node.name),
                })
            })
        })
        .collect()
}

fn is_recovery_candidate_node(node: &ProxyNode, allow_strategy_nodes: bool) -> bool {
    if is_management_node_name(&node.name) {
        return false;
    }

    let node_type = node
        .node_type
        .as_deref()
        .unwrap_or_default()
        .to_ascii_lowercase();
    if matches!(node_type.as_str(), "direct" | "reject") {
        return false;
    }
    is_leaf_proxy_node(node)
        || (allow_strategy_nodes
            && matches!(
                node_type.as_str(),
                "selector" | "urltest" | "fallback" | "loadbalance"
            ))
}

fn is_leaf_proxy_node(node: &ProxyNode) -> bool {
    let node_type = node
        .node_type
        .as_deref()
        .unwrap_or_default()
        .to_ascii_lowercase();
    !matches!(
        node_type.as_str(),
        "direct" | "reject" | "selector" | "urltest" | "fallback" | "loadbalance" | "relay"
    )
}

fn is_management_node_name(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    lower.contains("剩余流量")
        || lower.contains("流量")
        || lower.contains("套餐")
        || lower.contains("到期")
        || lower.contains("官网")
        || lower.contains("traffic")
        || lower.contains("expire")
        || lower.contains("subscription")
}

fn is_preferred_bootstrap_region(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    lower.contains("japan")
        || lower.contains("tokyo")
        || lower.contains("osaka")
        || lower.contains("singapore")
        || has_ascii_token(&lower, "jp")
        || has_ascii_token(&lower, "sg")
        || lower.contains("日本")
        || lower.contains("东京")
        || lower.contains("大阪")
        || lower.contains("新加坡")
        || lower.contains("狮城")
}

fn has_ascii_token(value: &str, token: &str) -> bool {
    value
        .split(|ch: char| !ch.is_ascii_alphanumeric())
        .any(|part| part == token)
}

fn merge_bootstrap_failure(initial: CommandResult, bootstrap: CommandResult) -> CommandResult {
    let initial_text = command_text(&initial);
    let bootstrap_text = command_text(&bootstrap);
    CommandResult {
        ok: false,
        code: bootstrap.code.or(initial.code),
        stdout: initial.stdout,
        stderr: format!(
            "initial update failed:\n{}\nhelium bootstrap failed:\n{}",
            initial_text.trim(),
            bootstrap_text.trim()
        ),
    }
}

fn merge_bootstrap_selection_failure(
    initial: CommandResult,
    bootstrap: CommandResult,
    selection_error: String,
) -> CommandResult {
    let initial_text = command_text(&initial);
    CommandResult {
        ok: false,
        code: initial.code,
        stdout: bootstrap.stdout,
        stderr: format!(
            "initial update failed:\n{}\nhelium bootstrap loaded, but node selection failed:\n{}",
            initial_text.trim(),
            selection_error
        ),
    }
}

fn merge_bootstrap_retry_result(
    initial: CommandResult,
    bootstrap: CommandResult,
    selected: &str,
    retry: CommandResult,
) -> CommandResult {
    if retry.ok {
        let initial_text = command_text(&initial);
        let bootstrap_text = command_text(&bootstrap);
        let retry_text = command_text(&retry);
        return CommandResult {
            ok: true,
            code: retry.code,
            stdout: format!(
                "initial update failed; helium bootstrap recovered the server\ninitial failure:\n{}\nhelium bootstrap:\n{}\n{}\nretry:\n{}",
                initial_text.trim(),
                bootstrap_text.trim(),
                selected,
                retry_text.trim()
            ),
            stderr: String::new(),
        };
    }

    let initial_text = command_text(&initial);
    let bootstrap_text = command_text(&bootstrap);
    let retry_text = command_text(&retry);
    CommandResult {
        ok: false,
        code: retry.code.or(initial.code),
        stdout: retry.stdout,
        stderr: format!(
            "initial update failed:\n{}\nhelium bootstrap succeeded:\n{}\n{}\nretry failed:\n{}",
            initial_text.trim(),
            bootstrap_text.trim(),
            selected,
            retry_text.trim()
        ),
    }
}

fn command_text(result: &CommandResult) -> String {
    let stdout = result.stdout.trim();
    let stderr = result.stderr.trim();
    match (stdout.is_empty(), stderr.is_empty()) {
        (true, true) => format!("exit={:?}", result.code),
        (false, true) => stdout.to_string(),
        (true, false) => stderr.to_string(),
        (false, false) => format!("{stdout}\n{stderr}"),
    }
}

fn log_command(
    storage: &Storage,
    server_id: i64,
    action: &str,
    result: &CommandResult,
) -> Result<(), String> {
    let status = if result.ok { "ok" } else { "error" };
    storage.add_log(Some(server_id), action, status, &command_summary(result))
}

async fn create_indexed_backup(
    storage: Storage,
    server: Server,
    reason: String,
    label: Option<String>,
) -> Result<BackupSnapshot, String> {
    run_blocking(move || create_indexed_backup_sync(&storage, &server, &reason, label.as_deref()))
        .await
}

fn create_indexed_backup_sync(
    storage: &Storage,
    server: &Server,
    reason: &str,
    label: Option<&str>,
) -> Result<BackupSnapshot, String> {
    let remote = backup::create(server, reason)?;
    let snapshot = storage.insert_backup_snapshot(
        server.id,
        reason,
        label,
        &remote.remote_dir,
        &remote.files,
    )?;
    storage.add_log(
        Some(server.id),
        "create_backup",
        "ok",
        &format!(
            "{} {}",
            reason,
            snapshot.label.as_deref().unwrap_or(&snapshot.remote_dir)
        ),
    )?;
    prune_backup_retention(storage, server)?;
    Ok(snapshot)
}

fn prune_backup_retention(storage: &Storage, server: &Server) -> Result<(), String> {
    let snapshots = storage.list_backup_snapshots(server.id)?;
    for snapshot in snapshots.into_iter().skip(20) {
        let result = backup::delete_remote(server, &snapshot.remote_dir).unwrap_or_else(|err| {
            CommandResult {
                ok: false,
                code: None,
                stdout: String::new(),
                stderr: err,
            }
        });
        if result.ok {
            storage.delete_backup_record(snapshot.id)?;
            continue;
        }
        storage.update_backup_status(snapshot.id, "delete_failed")?;
        storage.add_log(
            Some(server.id),
            "prune_backup",
            "error",
            &command_summary(&result),
        )?;
    }
    Ok(())
}

async fn run_blocking<T, F>(operation: F) -> Result<T, String>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T, String> + Send + 'static,
{
    tokio::task::spawn_blocking(operation)
        .await
        .map_err(|err| format!("blocking task failed: {err}"))?
}

fn command_summary(result: &CommandResult) -> String {
    let body = if result.ok {
        result.stdout.trim()
    } else {
        result.stderr.trim()
    };
    if body.is_empty() {
        format!("exit={:?}", result.code)
    } else {
        body.chars().take(600).collect()
    }
}

fn normalize_test_url(input: &str) -> Result<String, String> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Err("URL is required".to_string());
    }
    if trimmed.chars().any(char::is_control) || trimmed.chars().any(char::is_whitespace) {
        return Err("URL cannot contain spaces or control characters".to_string());
    }

    let candidate = if trimmed.contains("://") {
        trimmed.to_string()
    } else {
        format!("https://{trimmed}")
    };
    let parsed = Url::parse(&candidate).map_err(|err| format!("invalid URL: {err}"))?;
    match parsed.scheme() {
        "http" | "https" => Ok(candidate),
        _ => Err("only http and https URLs are supported".to_string()),
    }
}

fn normalize_subscription_input(mut input: SubscriptionInput) -> Result<SubscriptionInput, String> {
    input.url = normalize_subscription_url(&input.url)?;
    input.name = input
        .name
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .or_else(|| Some(subscription_name_from_url(&input.url)));
    Ok(input)
}

fn normalize_subscription_update_options(
    mut options: SubscriptionUpdateOptions,
) -> Result<SubscriptionUpdateOptions, String> {
    options.subscription_url = normalize_optional_subscription_url(options.subscription_url)?;
    Ok(options)
}

fn normalize_optional_subscription_url(value: Option<String>) -> Result<Option<String>, String> {
    match value {
        Some(url) if !url.trim().is_empty() => Ok(Some(normalize_subscription_url(&url)?)),
        _ => Ok(None),
    }
}

fn normalize_subscription_url(input: &str) -> Result<String, String> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Err("订阅链接不能为空".to_string());
    }
    if trimmed.chars().any(char::is_control) || trimmed.chars().any(char::is_whitespace) {
        return Err("订阅链接不能包含空格或控制字符".to_string());
    }
    let parsed = Url::parse(trimmed).map_err(|err| format!("无效订阅链接：{err}"))?;
    match parsed.scheme() {
        "http" | "https" => Ok(trimmed.to_string()),
        _ => Err("订阅链接只支持 http 和 https".to_string()),
    }
}

fn subscription_name_from_url(url: &str) -> String {
    Url::parse(url)
        .ok()
        .and_then(|parsed| parsed.host_str().map(ToString::to_string))
        .map(|host| host.trim_start_matches("www.").to_string())
        .filter(|host| !host.is_empty())
        .unwrap_or_else(|| "Subscription".to_string())
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

fn parse_egress_output(url: &str, output: CommandResult) -> EgressTestResult {
    let status = output
        .stdout
        .lines()
        .find_map(|line| line.strip_prefix("status=").map(str::trim))
        .filter(|value| !value.is_empty())
        .map(ToString::to_string);
    let elapsed_ms = output.stdout.lines().find_map(|line| {
        let value = line.strip_prefix("elapsed=")?.trim();
        parse_elapsed_ms(value)
    });
    let ok = output.ok
        && status
            .as_deref()
            .map(|value| {
                value == "reachable"
                    || value
                        .parse::<u16>()
                        .map(|code| (200..400).contains(&code))
                        .unwrap_or(false)
            })
            .unwrap_or(false);

    EgressTestResult {
        url: url.to_string(),
        ok,
        status,
        elapsed_ms,
        output,
    }
}

fn parse_elapsed_ms(value: &str) -> Option<u64> {
    let seconds = value.parse::<f64>().ok()?;
    if !seconds.is_finite() || seconds < 0.0 {
        return None;
    }
    Some((seconds * 1000.0).round() as u64)
}

fn normalize_manual_server_input(
    mut input: ManualServerInput,
) -> Result<ManualServerInput, String> {
    input.host_name = input.host_name.trim().to_string();
    input.user = input.user.trim().to_string();
    input.display_name = input
        .display_name
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    input.port = Some(input.port.unwrap_or(22));
    validate_server_fields(&input.host_name, &input.user, input.port.unwrap_or(22))?;
    Ok(input)
}

fn normalize_bootstrap_input(
    mut input: ServerBootstrapInput,
) -> Result<ServerBootstrapInput, String> {
    input.host_name = input.host_name.trim().to_string();
    input.user = input.user.trim().to_string();
    input.display_name = input
        .display_name
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    input.port = Some(input.port.unwrap_or(22));
    validate_server_fields(&input.host_name, &input.user, input.port.unwrap_or(22))?;
    if input.password.is_empty() {
        return Err("password is required for bootstrap".to_string());
    }
    Ok(input)
}

fn validate_server_fields(host_name: &str, user: &str, port: u16) -> Result<(), String> {
    if host_name.is_empty() {
        return Err("host is required".to_string());
    }
    if user.is_empty() {
        return Err("user is required".to_string());
    }
    if host_name.chars().any(char::is_whitespace) || user.chars().any(char::is_whitespace) {
        return Err("host and user cannot contain spaces".to_string());
    }
    if port == 0 {
        return Err("port must be greater than 0".to_string());
    }
    Ok(())
}

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_process::init())
        .setup(|app| {
            #[cfg(desktop)]
            {
                if option_env!("MIHOMO_ENABLE_UPDATER").is_some() {
                    app.handle()
                        .plugin(tauri_plugin_updater::Builder::new().build())
                        .map_err(Box::<dyn std::error::Error>::from)?;
                }
            }

            let app_dir = app.path().app_data_dir().map_err(|err| {
                Box::<dyn std::error::Error>::from(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    err.to_string(),
                ))
            })?;
            let storage = Storage::new(&app_dir).map_err(|err| {
                Box::<dyn std::error::Error>::from(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    err,
                ))
            })?;
            app.manage(AppState {
                storage,
                tunnels: controller::TunnelRegistry::default(),
                app_dir,
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            list_servers,
            import_ssh_hosts,
            get_managed_ssh_key,
            add_manual_server,
            bootstrap_server_with_password,
            delete_server,
            list_operation_logs,
            list_backups,
            create_backup,
            restore_backup,
            delete_backup,
            list_subscriptions,
            save_subscription,
            delete_subscription,
            mark_subscription_used,
            test_connection,
            test_server_egress,
            inspect_server,
            install_or_repair_mihomo,
            update_subscription,
            set_mihomo_service,
            set_mihomo_tun_enabled,
            inspect_remote_proxy,
            save_remote_proxy,
            set_remote_proxy_enabled,
            open_controller_tunnel,
            close_controller_tunnel,
            list_proxy_groups,
            select_proxy_node,
            measure_proxy_delay,
            measure_proxy_node_delay,
            auto_recover_proxy_node,
            read_mihomo_logs,
            read_mihomo_config
        ])
        .run(tauri::generate_context!())
        .expect("error while running Tauri application");
}

#[cfg(test)]
mod tests {
    use super::{
        bootstrap_node_candidates, is_preferred_bootstrap_region,
        normalize_subscription_update_options, normalize_test_url, parse_egress_output,
        recovery_node_candidates,
    };
    use crate::models::{CommandResult, ProxyGroup, ProxyNode, SubscriptionUpdateOptions};

    #[test]
    fn normalizes_external_test_urls() {
        assert_eq!(
            normalize_test_url("example.com/path").unwrap(),
            "https://example.com/path"
        );
        assert_eq!(
            normalize_test_url("http://example.com").unwrap(),
            "http://example.com"
        );
        assert!(normalize_test_url("ftp://example.com").is_err());
        assert!(normalize_test_url("https://example.com/a b").is_err());
    }

    #[test]
    fn validates_subscription_urls_for_remote_commands() {
        let install = normalize_subscription_update_options(SubscriptionUpdateOptions {
            subscription_url: Some(" https://example.com/sub ".to_string()),
        })
        .unwrap();
        assert_eq!(
            install.subscription_url.as_deref(),
            Some("https://example.com/sub")
        );

        assert!(
            normalize_subscription_update_options(SubscriptionUpdateOptions {
                subscription_url: Some("file:///tmp/config.yaml".to_string()),
            })
            .is_err()
        );
        assert!(
            normalize_subscription_update_options(SubscriptionUpdateOptions {
                subscription_url: Some("https://example.com/a b".to_string()),
            })
            .is_err()
        );
    }

    #[test]
    fn parses_curl_egress_output() {
        let result = parse_egress_output(
            "https://example.com",
            CommandResult {
                ok: true,
                code: Some(0),
                stdout: "status=204\nelapsed=0.153\nremote_ip=93.184.216.34\n".to_string(),
                stderr: String::new(),
            },
        );

        assert!(result.ok);
        assert_eq!(result.status.as_deref(), Some("204"));
        assert_eq!(result.elapsed_ms, Some(153));
    }

    #[test]
    fn prefers_japan_and_singapore_bootstrap_nodes() {
        let groups = vec![
            ProxyGroup {
                name: "GLOBAL".to_string(),
                now: Some("DIRECT".to_string()),
                nodes: vec![
                    proxy_node("DIRECT", "Direct"),
                    proxy_node("自动选择", "URLTest"),
                    proxy_node("US 01", "Trojan"),
                ],
            },
            ProxyGroup {
                name: "Cyber Paws".to_string(),
                now: Some("自动选择".to_string()),
                nodes: vec![
                    proxy_node("自动选择", "URLTest"),
                    proxy_node("JP 01", "Trojan"),
                    proxy_node("Singapore 02", "Shadowsocks"),
                    proxy_node("US 01", "Trojan"),
                ],
            },
        ];

        let candidates = bootstrap_node_candidates(&groups);

        assert_eq!(candidates[0].group, "Cyber Paws");
        assert_eq!(candidates[0].node, "JP 01");
        assert!(candidates[0].preferred_region);
        assert!(candidates
            .iter()
            .all(|candidate| candidate.node != "DIRECT"));
        assert!(candidates
            .iter()
            .all(|candidate| candidate.node != "自动选择"));
    }

    #[test]
    fn detects_short_region_tokens_for_bootstrap() {
        assert!(is_preferred_bootstrap_region("sg-01"));
        assert!(is_preferred_bootstrap_region("[JP] Tokyo"));
        assert!(!is_preferred_bootstrap_region("spring-us"));
    }

    #[test]
    fn recovery_candidates_skip_management_and_current_nodes() {
        let groups = vec![ProxyGroup {
            name: "Cyber Paws".to_string(),
            now: Some("HK-01".to_string()),
            nodes: vec![
                proxy_node("HK-01", "Trojan"),
                proxy_node("剩余流量：196 GB", "Trojan"),
                proxy_node("DIRECT", "Direct"),
                proxy_node("SG-02", "Shadowsocks"),
                proxy_node("US-03", "Trojan"),
            ],
        }];

        let candidates = recovery_node_candidates(&groups, "Cyber Paws", "HK-01");

        assert_eq!(candidates[0].node, "SG-02");
        assert!(candidates.iter().all(|candidate| candidate.node != "HK-01"));
        assert!(candidates
            .iter()
            .all(|candidate| !candidate.node.contains("剩余流量")));
        assert!(candidates
            .iter()
            .all(|candidate| candidate.node != "DIRECT"));
    }

    fn proxy_node(name: &str, node_type: &str) -> ProxyNode {
        ProxyNode {
            name: name.to_string(),
            node_type: Some(node_type.to_string()),
            udp: None,
            delay_ms: None,
            alive: None,
        }
    }
}
