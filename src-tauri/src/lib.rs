mod controller;
mod mihomo;
mod models;
mod redaction;
mod ssh;
mod storage;

use std::time::Duration;

use models::{
    CommandResult, EgressTestResult, InstallOptions, OperationLog, ProxyGroup, ProxyNode, Server,
    ServerHealth, ServiceCommandResult, SubscriptionUpdateOptions, TunnelInfo,
};
use reqwest::Url;
use storage::Storage;
use tauri::{Manager, State};

pub struct AppState {
    storage: Storage,
    tunnels: controller::TunnelRegistry,
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
fn test_connection(state: State<'_, AppState>, server_id: i64) -> Result<CommandResult, String> {
    let server = state.storage.get_server(server_id)?;
    let result = ssh::run_ssh_script(&server, "true", Duration::from_secs(12))?;
    let status = if result.ok { "online" } else { "offline" };
    state.storage.update_status(server_id, status)?;
    state.storage.add_log(
        Some(server_id),
        "test_connection",
        status,
        command_summary(&result).as_str(),
    )?;
    Ok(result)
}

#[tauri::command]
fn test_server_egress(
    state: State<'_, AppState>,
    server_id: i64,
    url: String,
) -> Result<EgressTestResult, String> {
    let server = state.storage.get_server(server_id)?;
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
    state.storage.add_log(
        Some(server_id),
        "test_server_egress",
        status,
        &format!("url={}", parsed.url),
    )?;
    Ok(parsed)
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
    let result = mihomo::install_or_repair(
        &server,
        options.unwrap_or(InstallOptions {
            subscription_url: None,
        }),
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
    let result = mihomo::update_subscription(
        &server,
        options.unwrap_or(SubscriptionUpdateOptions {
            subscription_url: None,
        }),
    )
    .await?;
    log_command(&state.storage, server_id, "update_subscription", &result)?;
    Ok(result)
}

#[tauri::command]
fn set_mihomo_service(
    state: State<'_, AppState>,
    server_id: i64,
    service_state: String,
) -> Result<ServiceCommandResult, String> {
    let server = state.storage.get_server(server_id)?;
    let result = mihomo::set_service(&server, &service_state)?;
    log_command(
        &state.storage,
        server_id,
        &format!("service:{service_state}"),
        &result.output,
    )?;
    Ok(result)
}

#[tauri::command]
fn open_controller_tunnel(
    state: State<'_, AppState>,
    server_id: i64,
) -> Result<TunnelInfo, String> {
    let server = state.storage.get_server(server_id)?;
    let info = state.tunnels.open(&server)?;
    state.storage.add_log(
        Some(server_id),
        "open_controller_tunnel",
        "ok",
        &format!("local port {}", info.local_port),
    )?;
    Ok(info)
}

#[tauri::command]
fn close_controller_tunnel(
    state: State<'_, AppState>,
    server_id: i64,
) -> Result<TunnelInfo, String> {
    let info = state.tunnels.close(server_id)?;
    state
        .storage
        .add_log(Some(server_id), "close_controller_tunnel", "ok", "closed")?;
    Ok(info)
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
    state.storage.add_log(
        Some(server_id),
        "select_proxy_node",
        "ok",
        &format!("group={group}"),
    )?;
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
fn read_mihomo_logs(
    state: State<'_, AppState>,
    server_id: i64,
    lines: Option<u32>,
) -> Result<String, String> {
    let server = state.storage.get_server(server_id)?;
    mihomo::read_logs(&server, lines.unwrap_or(200))
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

fn log_command(
    storage: &Storage,
    server_id: i64,
    action: &str,
    result: &CommandResult,
) -> Result<(), String> {
    let status = if result.ok { "ok" } else { "error" };
    storage.add_log(Some(server_id), action, status, &command_summary(result))
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

    let candidate = if trimmed.starts_with("http://") || trimmed.starts_with("https://") {
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
            let storage = Storage::new(app_dir).map_err(|err| {
                Box::<dyn std::error::Error>::from(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    err,
                ))
            })?;
            app.manage(AppState {
                storage,
                tunnels: controller::TunnelRegistry::default(),
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            list_servers,
            import_ssh_hosts,
            delete_server,
            list_operation_logs,
            test_connection,
            test_server_egress,
            inspect_server,
            install_or_repair_mihomo,
            update_subscription,
            set_mihomo_service,
            open_controller_tunnel,
            close_controller_tunnel,
            list_proxy_groups,
            select_proxy_node,
            measure_proxy_delay,
            read_mihomo_logs,
            read_mihomo_config
        ])
        .run(tauri::generate_context!())
        .expect("error while running Tauri application");
}

#[cfg(test)]
mod tests {
    use super::{normalize_test_url, parse_egress_output};
    use crate::models::CommandResult;

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
}
