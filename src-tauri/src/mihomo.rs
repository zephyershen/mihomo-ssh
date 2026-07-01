use std::{
    fs::File,
    io,
    path::{Path, PathBuf},
    time::Duration,
};

use chrono::Utc;
use flate2::read::GzDecoder;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use tempfile::tempdir;
use yaml_serde::{Mapping, Number, Value};

use crate::{
    models::{
        CommandResult, InstallOptions, Server, ServerHealth, ServiceCommandResult,
        SharedRulesConfig, SubscriptionUpdateOptions, TunConfig,
    },
    ssh,
};

const MIHOMO_RELEASE_API: &str = "https://api.github.com/repos/MetaCubeX/mihomo/releases/latest";
const GEOIP_METADB_URL: &str =
    "https://github.com/MetaCubeX/meta-rules-dat/releases/download/latest/geoip.metadb";
const GEOSITE_DAT_URL: &str =
    "https://github.com/MetaCubeX/meta-rules-dat/releases/download/latest/geosite.dat";
const REMOTE_TMP_DIR: &str = "/tmp/mihomo-manager";
const MAX_MIHOMO_ARCHIVE_BYTES: u64 = 80 * 1024 * 1024;
const MAX_GEODATA_BYTES: u64 = 32 * 1024 * 1024;
const MAX_SHARED_RULES_BYTES: usize = 64 * 1024;
const SHARED_RULES_PATH: &str = "/etc/mihomo/manager-shared-rules.txt";
const BASE_TUN_EXCLUDES: &[&str] = &[
    "10.0.0.0/8",
    "100.64.0.0/10",
    "127.0.0.0/8",
    "169.254.0.0/16",
    "172.16.0.0/12",
    "192.168.0.0/16",
    "224.0.0.0/4",
    "::1/128",
    "fc00::/7",
    "fe80::/10",
    "ff00::/8",
];

#[derive(Clone, Copy)]
enum SubscriptionDownloadMode {
    Proxy,
    Direct,
}

#[derive(Clone, Copy)]
struct GeodataAsset {
    file_name: &'static str,
    url: &'static str,
}

pub async fn inspect_server(server: &Server) -> Result<ServerHealth, String> {
    let script = r#"
set +e
if [ -r /etc/os-release ]; then . /etc/os-release; fi
kv() { printf '%s=%s\n' "$1" "$2"; }
kv os_pretty_name "${PRETTY_NAME:-unknown}"
kv os_id "${ID:-unknown}"
kv arch "$(uname -m 2>/dev/null || true)"
if command -v systemctl >/dev/null 2>&1; then kv has_systemd true; else kv has_systemd false; fi
kv mihomo_path "$(command -v mihomo 2>/dev/null || true)"
if command -v mihomo >/dev/null 2>&1; then kv mihomo_version "$(mihomo -v 2>/dev/null | head -n 1 || true)"; fi
if command -v systemctl >/dev/null 2>&1; then
  kv service_active "$(systemctl is-active mihomo 2>/dev/null || true)"
  kv service_enabled "$(systemctl is-enabled mihomo 2>/dev/null || true)"
fi
[ -f /etc/mihomo/config.yaml ] && kv has_config true || kv has_config false
[ -f /etc/mihomo/subscription.url ] && kv has_subscription true || kv has_subscription false
if [ -f /etc/mihomo/config.yaml ]; then
  awk '
    /^[[:space:]]*mixed-port:/ {print "mixed_port=" $2}
    /^[[:space:]]*external-controller:/ {sub(/^[^:]+:[[:space:]]*/, ""); print "controller=" $0}
    /^[[:space:]]*allow-lan:/ {print "allow_lan=" $2}
    /^[[:space:]]*geo-auto-update:/ {print "geo_auto_update=" $2}
  ' /etc/mihomo/config.yaml
fi
"#;

    let output =
        run_ssh_script_blocking(server, script.to_string(), Duration::from_secs(20)).await?;
    if !output.ok {
        return Err(format!("inspect failed: {}", output.stderr));
    }

    Ok(parse_health(&output.stdout))
}

pub async fn read_remote_config(server: &Server) -> Result<String, String> {
    let output = run_ssh_script_blocking(
        server,
        "test -f /etc/mihomo/config.yaml && sed -n '1,260p' /etc/mihomo/config.yaml || true"
            .to_string(),
        Duration::from_secs(20),
    )
    .await?;
    if output.ok {
        Ok(output.stdout)
    } else {
        Err(output.stderr)
    }
}

pub async fn read_shared_rules(server: &Server) -> Result<SharedRulesConfig, String> {
    let rules = fetch_shared_rules_text(server).await?;
    let rules = normalize_shared_rules_text(&rules)?;
    Ok(SharedRulesConfig {
        remote_path: SHARED_RULES_PATH.to_string(),
        applied_count: active_shared_rule_lines(&rules)?.len(),
        rules,
    })
}

pub async fn save_shared_rules(server: &Server, rules: String) -> Result<CommandResult, String> {
    let old_rules = fetch_shared_rules_text(server)
        .await
        .and_then(|rules| normalize_shared_rules_text(&rules))?;
    let rules = normalize_shared_rules_text(&rules)?;
    let applied_count = active_shared_rule_lines(&rules)?.len();

    let dir = tempdir().map_err(|err| err.to_string())?;
    let local_rules = dir.path().join("manager-shared-rules.txt");
    let current_config = dir.path().join("config.current.yaml");
    let patched_config = dir.path().join("config.shared-rules.yaml");
    blocking({
        let local_rules = local_rules.clone();
        let rules = rules.clone();
        move || std::fs::write(local_rules, rules).map_err(|err| err.to_string())
    })
    .await?;

    let prep = run_ssh_script_blocking(
        server,
        format!(
            r#"
set -euo pipefail
install -d -m 0755 /etc/mihomo
install -d -m 0700 {REMOTE_TMP_DIR}
if [ -f /etc/mihomo/config.yaml ]; then
  printf 'has_config\n'
fi
"#
        ),
        Duration::from_secs(20),
    )
    .await?;
    if !prep.ok {
        return Ok(prep);
    }

    let pushed_rules = scp_to_remote_blocking(
        server,
        local_rules,
        format!("{REMOTE_TMP_DIR}/manager-shared-rules.txt"),
        Duration::from_secs(40),
    )
    .await?;
    if !pushed_rules.ok {
        return Ok(pushed_rules);
    }

    if !prep.stdout.lines().any(|line| line.trim() == "has_config") {
        return run_ssh_script_blocking(
            server,
            format!(
                r#"
set -euo pipefail
install -d -m 0755 /etc/mihomo
install -m 0600 {REMOTE_TMP_DIR}/manager-shared-rules.txt {SHARED_RULES_PATH}
rm -rf {REMOTE_TMP_DIR}
printf 'shared rules saved: {applied_count} active rule(s); config missing, will apply on next subscription update\n'
"#
            ),
            Duration::from_secs(30),
        )
        .await;
    }

    let pulled = scp_from_remote_blocking(
        server,
        "/etc/mihomo/config.yaml".to_string(),
        current_config.clone(),
        Duration::from_secs(40),
    )
    .await?;
    if !pulled.ok {
        return Ok(pulled);
    }

    patch_config_file_replacing_shared_rules_blocking(
        current_config,
        patched_config.clone(),
        old_rules,
        rules,
    )
    .await?;
    let pushed_config = scp_to_remote_blocking(
        server,
        patched_config,
        format!("{REMOTE_TMP_DIR}/config.shared-rules.yaml"),
        Duration::from_secs(40),
    )
    .await?;
    if !pushed_config.ok {
        return Ok(pushed_config);
    }

    run_ssh_script_blocking(
        server,
        format!(
            r#"
set -euo pipefail
ts="$(date +%Y%m%d%H%M%S)"
/usr/local/bin/mihomo -t -d /etc/mihomo -f {REMOTE_TMP_DIR}/config.shared-rules.yaml
cp -a /etc/mihomo/config.yaml "/etc/mihomo/config.yaml.shared-rules-bak.${{ts}}"
install -m 0600 {REMOTE_TMP_DIR}/manager-shared-rules.txt {SHARED_RULES_PATH}
install -m 0644 {REMOTE_TMP_DIR}/config.shared-rules.yaml /etc/mihomo/config.yaml
systemctl restart mihomo
rm -rf {REMOTE_TMP_DIR}
printf 'shared rules saved and applied: {applied_count} active rule(s) at %s\n' "{updated_at}"
"#,
            updated_at = Utc::now().to_rfc3339()
        ),
        Duration::from_secs(90),
    )
    .await
}

pub async fn inspect_tun_config(server: &Server) -> Result<Option<TunConfig>, String> {
    let output = run_ssh_script_blocking(
        server,
        r#"
set +e
test -f /etc/mihomo/config.yaml || exit 0
awk '
  /^tun:[[:space:]]*/ { in_tun=1; print; next }
  in_tun && /^[^[:space:]]/ { exit }
  in_tun { print }
' /etc/mihomo/config.yaml
"#
        .to_string(),
        Duration::from_secs(20),
    )
    .await?;
    if output.ok {
        parse_tun_config_yaml(&output.stdout)
    } else {
        Err(output.stderr)
    }
}

pub async fn set_tun_enabled(server: &Server, enabled: bool) -> Result<CommandResult, String> {
    let prep = run_ssh_script_blocking(
        server,
        format!("set -euo pipefail\ninstall -d -m 0700 {REMOTE_TMP_DIR}\ntest -f /etc/mihomo/config.yaml"),
        Duration::from_secs(20),
    )
    .await?;
    if !prep.ok {
        return Ok(prep);
    }

    let ssh_excludes = if enabled {
        collect_ssh_tun_excludes(server).await?
    } else {
        Vec::new()
    };

    let dir = tempdir().map_err(|err| err.to_string())?;
    let local_config = dir.path().join("config.yaml");
    let patched_config = dir.path().join("config.tun.yaml");
    let pulled = scp_from_remote_blocking(
        server,
        "/etc/mihomo/config.yaml".to_string(),
        local_config.clone(),
        Duration::from_secs(40),
    )
    .await?;
    if !pulled.ok {
        return Ok(pulled);
    }

    patch_tun_config_file_blocking(local_config, patched_config.clone(), enabled, ssh_excludes)
        .await?;
    let pushed = scp_to_remote_blocking(
        server,
        patched_config,
        format!("{REMOTE_TMP_DIR}/config.tun.yaml"),
        Duration::from_secs(40),
    )
    .await?;
    if !pushed.ok {
        return Ok(pushed);
    }

    let state = if enabled { "enabled" } else { "disabled" };
    let install_script = format!(
        r#"
set -euo pipefail
ts="$(date +%Y%m%d%H%M%S)"
rollback="/etc/mihomo/config.yaml.tun-rollback.${{ts}}"
marker="/tmp/mihomo-manager-tun-ok.${{ts}}"
cp -a /etc/mihomo/config.yaml "$rollback"
/usr/local/bin/mihomo -t -d /etc/mihomo -f {REMOTE_TMP_DIR}/config.tun.yaml
(
  sleep 35
  if [ ! -f "$marker" ]; then
    cp -a "$rollback" /etc/mihomo/config.yaml
    systemctl restart mihomo || true
    logger -t mihomo-manager "TUN change rolled back because SSH confirmation marker was missing"
  fi
  rm -f "$marker" "$rollback"
) >/dev/null 2>&1 &
install -m 0644 {REMOTE_TMP_DIR}/config.tun.yaml /etc/mihomo/config.yaml
systemctl restart mihomo
sleep 3
systemctl is-active mihomo
touch "$marker"
rm -rf {REMOTE_TMP_DIR}
printf 'tun {state} at %s\n' "{updated_at}"
"#,
        state = state,
        updated_at = Utc::now().to_rfc3339()
    );

    run_ssh_script_blocking(server, install_script, Duration::from_secs(90)).await
}

pub async fn install_or_repair(
    server: &Server,
    options: InstallOptions,
) -> Result<CommandResult, String> {
    let health = inspect_server(server).await?;
    let arch = health.arch.unwrap_or_else(|| "x86_64".to_string());
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(120))
        .build()
        .map_err(|err| err.to_string())?;
    let asset = latest_asset_for_arch(&client, &arch).await?;
    let should_update_subscription = options
        .subscription_url
        .as_deref()
        .map(|value| !value.trim().is_empty())
        .unwrap_or(false);

    let dir = tempdir().map_err(|err| err.to_string())?;
    let archive_path = dir.path().join("mihomo.gz");
    let binary_path = dir.path().join("mihomo");
    download_asset(&client, &asset, &archive_path).await?;
    decompress_gzip_blocking(archive_path.clone(), binary_path.clone()).await?;

    let prep = run_ssh_script_blocking(
        server,
        format!("set -euo pipefail\ninstall -d -m 0700 {REMOTE_TMP_DIR}"),
        Duration::from_secs(20),
    )
    .await?;
    if !prep.ok {
        return Ok(prep);
    }

    let uploaded = scp_to_remote_blocking(
        server,
        binary_path.clone(),
        format!("{REMOTE_TMP_DIR}/mihomo"),
        Duration::from_secs(90),
    )
    .await?;
    if !uploaded.ok {
        return Ok(uploaded);
    }

    let subscription_write = match options.subscription_url {
        Some(url) if !url.trim().is_empty() => format!(
            "cat > /etc/mihomo/subscription.url <<'SUBEOF'\n{}\nSUBEOF\nchmod 600 /etc/mihomo/subscription.url\n",
            shell_safe_multiline(&url)
        ),
        _ => String::new(),
    };

    let script = format!(
        r#"
set -euo pipefail
ts="$(date +%Y%m%d%H%M%S)"
install -d -m 0755 /etc/mihomo
if [ -x /usr/local/bin/mihomo ]; then
  cp -a /usr/local/bin/mihomo "/usr/local/bin/mihomo.bak.${{ts}}"
fi
install -m 0755 {REMOTE_TMP_DIR}/mihomo /usr/local/bin/mihomo
if [ -f /etc/mihomo/config.yaml ]; then
  cp -a /etc/mihomo/config.yaml "/etc/mihomo/config.yaml.bak.${{ts}}"
else
  cat > /etc/mihomo/config.yaml <<'CFG'
mixed-port: 7890
allow-lan: false
external-controller: 127.0.0.1:9090
geo-auto-update: false
mode: rule
log-level: info
rules:
  - MATCH,DIRECT
CFG
fi
{subscription_write}
cat > /etc/systemd/system/mihomo.service <<'UNIT'
[Unit]
Description=mihomo proxy service
After=network-online.target
Wants=network-online.target

[Service]
Type=simple
User=root
WorkingDirectory=/etc/mihomo
ExecStart=/usr/local/bin/mihomo -d /etc/mihomo -f /etc/mihomo/config.yaml
Restart=on-failure
RestartSec=3
LimitNOFILE=1048576

[Install]
WantedBy=multi-user.target
UNIT
systemctl daemon-reload
/usr/local/bin/mihomo -t -d /etc/mihomo -f /etc/mihomo/config.yaml
systemctl enable --now mihomo
rm -rf {REMOTE_TMP_DIR}
printf 'mihomo %s installed from %s\n' "$(/usr/local/bin/mihomo -v | head -n 1)" "{asset_name}"
"#,
        asset_name = asset.name
    );

    let mut result = run_ssh_script_blocking(server, script, Duration::from_secs(90)).await?;
    if result.ok && should_update_subscription {
        let update = update_subscription(
            server,
            SubscriptionUpdateOptions {
                subscription_url: None,
            },
        )
        .await?;
        result.ok = update.ok;
        result.code = update.code;
        result.stdout = format!(
            "{}\n{}",
            result.stdout.trim_end(),
            update.stdout.trim_start()
        );
        result.stderr = update.stderr;
    }
    Ok(result)
}

pub async fn update_subscription(
    server: &Server,
    options: SubscriptionUpdateOptions,
) -> Result<CommandResult, String> {
    update_subscription_with_download_mode(server, options, SubscriptionDownloadMode::Proxy).await
}

pub async fn update_subscription_direct(
    server: &Server,
    options: SubscriptionUpdateOptions,
) -> Result<CommandResult, String> {
    update_subscription_with_download_mode(server, options, SubscriptionDownloadMode::Direct).await
}

async fn update_subscription_with_download_mode(
    server: &Server,
    options: SubscriptionUpdateOptions,
    download_mode: SubscriptionDownloadMode,
) -> Result<CommandResult, String> {
    if let Some(url) = options.subscription_url.as_deref() {
        if !url.trim().is_empty() {
            let script = format!(
                "set -euo pipefail\ninstall -d -m 0755 /etc/mihomo\ncat > /etc/mihomo/subscription.url <<'SUBEOF'\n{}\nSUBEOF\nchmod 600 /etc/mihomo/subscription.url\n",
                shell_safe_multiline(url)
            );
            let written = run_ssh_script_blocking(server, script, Duration::from_secs(15)).await?;
            if !written.ok {
                return Ok(written);
            }
        }
    }

    let proxy_arg = match download_mode {
        SubscriptionDownloadMode::Proxy => "  --proxy http://127.0.0.1:7890 \\\n",
        SubscriptionDownloadMode::Direct => "",
    };
    let download_script = format!(
        r#"
set -euo pipefail
test -f /etc/mihomo/subscription.url
install -d -m 0700 {REMOTE_TMP_DIR}
sub="$(sed -n '1p' /etc/mihomo/subscription.url)"
case "$sub" in
  http://*|https://*) ;;
  *) echo "subscription URL must start with http:// or https://" >&2; exit 2 ;;
esac
if printf '%s' "$sub" | grep -q '[[:space:][:cntrl:]]'; then
  echo "subscription URL cannot contain spaces or control characters" >&2
  exit 2
fi
curl -fsSL --connect-timeout 20 --max-time 90 --retry 2 \
  -A 'clash-verge' \
{proxy_arg}  -o {REMOTE_TMP_DIR}/config.yaml \
  "$sub"
test -s {REMOTE_TMP_DIR}/config.yaml
"#
    );
    let downloaded =
        run_ssh_script_blocking(server, download_script, Duration::from_secs(120)).await?;
    if !downloaded.ok {
        return Ok(downloaded);
    }

    let dir = tempdir().map_err(|err| err.to_string())?;
    let local_config = dir.path().join("config.yaml");
    let current_config = dir.path().join("config.current.yaml");
    let patched_config = dir.path().join("config.patched.yaml");
    let shared_rules = fetch_shared_rules_text(server).await?;
    let current_pulled = scp_from_remote_blocking(
        server,
        "/etc/mihomo/config.yaml".to_string(),
        current_config.clone(),
        Duration::from_secs(40),
    )
    .await?;
    let pulled = scp_from_remote_blocking(
        server,
        format!("{REMOTE_TMP_DIR}/config.yaml"),
        local_config.clone(),
        Duration::from_secs(40),
    )
    .await?;
    if !pulled.ok {
        return Ok(pulled);
    }

    patch_config_file_with_shared_rules_blocking(
        local_config,
        patched_config.clone(),
        shared_rules,
    )
    .await?;
    if current_pulled.ok {
        preserve_tun_config_file_blocking(current_config, patched_config.clone()).await?;
    }
    let pushed = scp_to_remote_blocking(
        server,
        patched_config.clone(),
        format!("{REMOTE_TMP_DIR}/config.patched.yaml"),
        Duration::from_secs(40),
    )
    .await?;
    if !pushed.ok {
        return Ok(pushed);
    }

    let install_script = format!(
        r#"
set -euo pipefail
ts="$(date +%Y%m%d%H%M%S)"
/usr/local/bin/mihomo -t -d /etc/mihomo -f {REMOTE_TMP_DIR}/config.patched.yaml
if [ -f /etc/mihomo/config.yaml ]; then
  cp -a /etc/mihomo/config.yaml "/etc/mihomo/config.yaml.bak.${{ts}}"
fi
install -m 0644 {REMOTE_TMP_DIR}/config.patched.yaml /etc/mihomo/config.yaml
systemctl restart mihomo
rm -rf {REMOTE_TMP_DIR}
printf 'subscription updated at %s\n' "{updated_at}"
"#,
        updated_at = Utc::now().to_rfc3339()
    );

    let mut installed =
        run_ssh_script_blocking(server, install_script.clone(), Duration::from_secs(90)).await?;
    if !installed.ok && has_geodata_download_error(&installed) {
        let geodata = ensure_geodata_files(server, &installed).await?;
        if geodata.ok {
            let retry =
                run_ssh_script_blocking(server, install_script, Duration::from_secs(90)).await?;
            installed = merge_geodata_retry_result(installed, geodata, retry);
        } else {
            installed = merge_geodata_repair_failure(installed, geodata);
        }
    }
    Ok(installed)
}

pub fn set_service(server: &Server, state: &str) -> Result<ServiceCommandResult, String> {
    let command = match state {
        "start" => "systemctl start mihomo && systemctl is-active mihomo",
        "stop" => "systemctl stop mihomo && systemctl is-active mihomo || true",
        "restart" => "systemctl restart mihomo && systemctl is-active mihomo",
        "enable" => "systemctl enable --now mihomo && systemctl is-enabled mihomo",
        "disable" => "systemctl disable --now mihomo && systemctl is-enabled mihomo || true",
        other => return Err(format!("unsupported service state: {other}")),
    };
    let output = ssh::run_ssh_script(server, command, Duration::from_secs(45))?;
    Ok(ServiceCommandResult {
        state: state.to_string(),
        output,
    })
}

pub fn read_logs(server: &Server, lines: u32) -> Result<String, String> {
    let lines = lines.clamp(20, 1000);
    let script = format!("journalctl -u mihomo --no-pager -n {lines} 2>&1 || true");
    let output = ssh::run_ssh_script(server, &script, Duration::from_secs(20))?;
    if output.ok {
        Ok(output.stdout)
    } else {
        Err(output.stderr)
    }
}

async fn ensure_geodata_files(
    server: &Server,
    failed: &CommandResult,
) -> Result<CommandResult, String> {
    let assets = geodata_assets_for_error(failed);
    if assets.is_empty() {
        return Ok(CommandResult {
            ok: false,
            code: None,
            stdout: String::new(),
            stderr: "no missing geodata asset was detected".to_string(),
        });
    }

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(120))
        .build()
        .map_err(|err| err.to_string())?;
    let dir = tempdir().map_err(|err| err.to_string())?;

    let prep = run_ssh_script_blocking(
        server,
        format!("set -euo pipefail\ninstall -d -m 0700 {REMOTE_TMP_DIR}"),
        Duration::from_secs(20),
    )
    .await?;
    if !prep.ok {
        return Ok(prep);
    }

    for asset in &assets {
        let local_path = dir.path().join(asset.file_name);
        download_limited_file(&client, asset.url, &local_path, MAX_GEODATA_BYTES).await?;
        let uploaded = scp_to_remote_blocking(
            server,
            local_path,
            format!("{REMOTE_TMP_DIR}/{}", asset.file_name),
            Duration::from_secs(90),
        )
        .await?;
        if !uploaded.ok {
            return Ok(uploaded);
        }
    }

    let mut install_lines = String::new();
    for asset in &assets {
        install_lines.push_str(&format!(
            r#"if [ -f /etc/mihomo/{file} ]; then
  cp -a /etc/mihomo/{file} "/etc/mihomo/{file}.bak.${{ts}}"
fi
install -m 0644 {tmp}/{file} /etc/mihomo/{file}
"#,
            file = asset.file_name,
            tmp = REMOTE_TMP_DIR
        ));
    }
    let asset_names = assets
        .iter()
        .map(|asset| asset.file_name)
        .collect::<Vec<_>>()
        .join(", ");
    let script = format!(
        r#"
set -euo pipefail
install -d -m 0755 /etc/mihomo
ts="$(date +%Y%m%d%H%M%S)"
{install_lines}
printf 'geodata repaired: {asset_names}\n'
"#
    );
    run_ssh_script_blocking(server, script, Duration::from_secs(30)).await
}

fn geodata_assets_for_error(result: &CommandResult) -> Vec<GeodataAsset> {
    let text = format!("{}\n{}", result.stdout, result.stderr).to_ascii_lowercase();
    let wants_geosite = text.contains("geosite");
    let wants_geoip = text.contains("geoip") || text.contains("mmdb");

    let mut assets = Vec::new();
    if wants_geoip {
        assets.push(GeodataAsset {
            file_name: "geoip.metadb",
            url: GEOIP_METADB_URL,
        });
    }
    if wants_geosite {
        assets.push(GeodataAsset {
            file_name: "GeoSite.dat",
            url: GEOSITE_DAT_URL,
        });
    }
    assets
}

fn has_geodata_download_error(result: &CommandResult) -> bool {
    !geodata_assets_for_error(result).is_empty()
}

fn merge_geodata_retry_result(
    initial: CommandResult,
    geodata: CommandResult,
    retry: CommandResult,
) -> CommandResult {
    let initial_text = command_text(&initial);
    let retry_text = command_text(&retry);
    let stdout = if retry.ok {
        format!(
            "initial config test needed geodata; repaired and retried\n{}\n{}",
            geodata.stdout.trim(),
            retry_text.trim()
        )
    } else {
        retry.stdout.clone()
    };
    let stderr = if retry.ok {
        String::new()
    } else {
        format!(
            "initial failure:\n{}\nretry failure:\n{}",
            initial_text.trim(),
            command_text(&retry).trim()
        )
    };

    CommandResult {
        ok: retry.ok,
        code: retry.code,
        stdout,
        stderr,
    }
}

fn merge_geodata_repair_failure(initial: CommandResult, geodata: CommandResult) -> CommandResult {
    let initial_text = command_text(&initial);
    let geodata_text = command_text(&geodata);
    CommandResult {
        ok: false,
        code: geodata.code.or(initial.code),
        stdout: initial.stdout,
        stderr: format!(
            "{}\ngeodata repair failed:\n{}",
            initial_text.trim(),
            geodata_text.trim()
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

fn parse_health(stdout: &str) -> ServerHealth {
    let mut health = ServerHealth {
        checked_at: Utc::now().to_rfc3339(),
        ..ServerHealth::default()
    };

    for line in stdout.lines() {
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let value = value.trim();
        match key {
            "os_pretty_name" => health.os_pretty_name = non_empty(value),
            "os_id" => health.os_id = non_empty(value),
            "arch" => health.arch = non_empty(value),
            "has_systemd" => health.has_systemd = value == "true",
            "mihomo_path" => health.mihomo_path = non_empty(value),
            "mihomo_version" => health.mihomo_version = non_empty(value),
            "service_active" => health.service_active = non_empty(value),
            "service_enabled" => health.service_enabled = non_empty(value),
            "has_config" => health.has_config = value == "true",
            "has_subscription" => health.has_subscription = value == "true",
            "mixed_port" => health.mixed_port = value.parse::<u16>().ok(),
            "controller" => health.controller = non_empty(value),
            "allow_lan" => health.allow_lan = parse_bool(value),
            "geo_auto_update" => health.geo_auto_update = parse_bool(value),
            _ => {}
        }
    }

    health
}

fn non_empty(value: &str) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() || trimmed == "unknown" {
        None
    } else {
        Some(trimmed.to_string())
    }
}

fn parse_bool(value: &str) -> Option<bool> {
    match value.trim().to_ascii_lowercase().as_str() {
        "true" => Some(true),
        "false" => Some(false),
        _ => None,
    }
}

fn patch_config_file_with_shared_rules(
    input: &Path,
    output: &Path,
    shared_rules: &str,
) -> Result<(), String> {
    let content = std::fs::read_to_string(input).map_err(|err| err.to_string())?;
    let patched = patch_config_yaml_with_shared_rules(&content, shared_rules)?;
    std::fs::write(output, patched).map_err(|err| err.to_string())
}

pub fn patch_config_yaml_with_shared_rules(
    content: &str,
    shared_rules: &str,
) -> Result<String, String> {
    patch_config_yaml_replacing_shared_rules(content, "", shared_rules)
}

pub fn patch_config_yaml_replacing_shared_rules(
    content: &str,
    old_shared_rules: &str,
    new_shared_rules: &str,
) -> Result<String, String> {
    let mut value: Value = yaml_serde::from_str(content).map_err(|err| err.to_string())?;
    let map = value
        .as_mapping_mut()
        .ok_or_else(|| "subscription config must be a YAML mapping".to_string())?;

    put_number(map, "mixed-port", 7890);
    put_string(map, "external-controller", "127.0.0.1:9090");
    put_bool(map, "geo-auto-update", false);
    if !map.contains_key(&Value::String("allow-lan".to_string())) {
        put_bool(map, "allow-lan", false);
    }

    replace_shared_rules(map, old_shared_rules, new_shared_rules)?;

    yaml_serde::to_string(&value).map_err(|err| err.to_string())
}

pub fn parse_tun_config_yaml(content: &str) -> Result<Option<TunConfig>, String> {
    if content.trim().is_empty() {
        return Ok(None);
    }
    let value: Value = yaml_serde::from_str(content).map_err(|err| err.to_string())?;
    let Some(map) = value.as_mapping() else {
        return Ok(None);
    };
    let Some(tun) = get_value(map, "tun") else {
        return Ok(None);
    };
    let Some(tun_map) = tun.as_mapping() else {
        return Ok(None);
    };
    let route_exclude_address = string_list(tun_map, "route-exclude-address")?;
    let ssh_protection = route_exclude_address
        .iter()
        .filter(|value| is_ssh_safe_exclude(value))
        .cloned()
        .collect();

    Ok(Some(TunConfig {
        enabled: get_bool(tun_map, "enable").unwrap_or(false),
        stack: get_string(tun_map, "stack"),
        auto_route: get_bool(tun_map, "auto-route"),
        auto_detect_interface: get_bool(tun_map, "auto-detect-interface"),
        auto_redirect: get_bool(tun_map, "auto-redirect"),
        dns_hijack: string_list(tun_map, "dns-hijack")?,
        route_exclude_address,
        ssh_protection,
    }))
}

pub fn patch_tun_config_yaml(
    content: &str,
    enabled: bool,
    ssh_excludes: &[String],
) -> Result<String, String> {
    let mut value: Value = yaml_serde::from_str(content).map_err(|err| err.to_string())?;
    let map = value
        .as_mapping_mut()
        .ok_or_else(|| "mihomo config must be a YAML mapping".to_string())?;
    let tun_key = Value::String("tun".to_string());
    let tun_value = map
        .entry(tun_key)
        .or_insert_with(|| Value::Mapping(Mapping::new()));
    if tun_value.as_mapping_mut().is_none() {
        *tun_value = Value::Mapping(Mapping::new());
    }
    let tun_map = tun_value
        .as_mapping_mut()
        .ok_or_else(|| "tun config must be a YAML mapping".to_string())?;

    put_bool(tun_map, "enable", enabled);
    if enabled {
        if get_string(tun_map, "stack").is_none() {
            put_string(tun_map, "stack", "system");
        }
        put_bool(tun_map, "auto-route", true);
        put_bool(tun_map, "auto-detect-interface", true);
        if string_list(tun_map, "dns-hijack")?.is_empty() {
            put_string_list(tun_map, "dns-hijack", &["any:53".to_string()]);
        }
        let mut excludes = string_list(tun_map, "route-exclude-address")?;
        excludes.extend(BASE_TUN_EXCLUDES.iter().map(|value| (*value).to_string()));
        excludes.extend(ssh_excludes.iter().cloned());
        let excludes = normalize_excludes(excludes);
        put_string_list(tun_map, "route-exclude-address", &excludes);
    }

    yaml_serde::to_string(&value).map_err(|err| err.to_string())
}

pub fn preserve_tun_config_yaml(current: &str, next: &str) -> Result<String, String> {
    let current_value: Value = yaml_serde::from_str(current).map_err(|err| err.to_string())?;
    let Some(current_map) = current_value.as_mapping() else {
        return Ok(next.to_string());
    };
    let Some(tun) = get_value(current_map, "tun") else {
        return Ok(next.to_string());
    };

    let mut next_value: Value = yaml_serde::from_str(next).map_err(|err| err.to_string())?;
    let next_map = next_value
        .as_mapping_mut()
        .ok_or_else(|| "mihomo config must be a YAML mapping".to_string())?;
    next_map.insert(Value::String("tun".to_string()), tun.clone());
    yaml_serde::to_string(&next_value).map_err(|err| err.to_string())
}

async fn collect_ssh_tun_excludes(server: &Server) -> Result<Vec<String>, String> {
    let output = run_ssh_script_blocking(
        server,
        r#"
set +e
emit_exclude() {
  value="$1"
  case "$value" in
    *[!0-9A-Fa-f.:/]*|"") return ;;
    *) printf 'EXCLUDE\t%s\n' "$value" ;;
  esac
}
client_ip="$(printf '%s\n' "${SSH_CONNECTION:-}" | awk '{print $1}')"
if [ -n "$client_ip" ]; then
  case "$client_ip" in
    *:*) emit_exclude "$client_ip/128" ;;
    *) emit_exclude "$client_ip/32" ;;
  esac
  if command -v ip >/dev/null 2>&1; then
    dev="$(ip -o route get "$client_ip" 2>/dev/null | awk '{for (i=1;i<=NF;i++) if ($i=="dev") {print $(i+1); exit}}')"
    if [ -n "$dev" ]; then
      ip -o -4 addr show dev "$dev" scope global 2>/dev/null | awk '{print "EXCLUDE\t"$4}'
      ip -o -6 addr show dev "$dev" scope global 2>/dev/null | awk '{print "EXCLUDE\t"$4}'
    fi
  fi
fi
"#
        .to_string(),
        Duration::from_secs(20),
    )
    .await?;
    if !output.ok {
        return Err(output.stderr);
    }
    let excludes = output
        .stdout
        .lines()
        .filter_map(|line| line.strip_prefix("EXCLUDE\t"))
        .map(str::trim)
        .filter(|value| is_valid_route_exclude(value))
        .map(ToString::to_string)
        .collect::<Vec<_>>();
    Ok(normalize_excludes(excludes))
}

fn get_value<'a>(map: &'a Mapping, key: &str) -> Option<&'a Value> {
    map.get(&Value::String(key.to_string()))
}

fn get_string(map: &Mapping, key: &str) -> Option<String> {
    get_value(map, key)
        .and_then(Value::as_str)
        .map(ToString::to_string)
}

fn get_bool(map: &Mapping, key: &str) -> Option<bool> {
    get_value(map, key).and_then(Value::as_bool)
}

fn string_list(map: &Mapping, key: &str) -> Result<Vec<String>, String> {
    let Some(value) = get_value(map, key) else {
        return Ok(Vec::new());
    };
    let Some(items) = value.as_sequence() else {
        return Err(format!("{key} must be a list"));
    };
    let mut values = Vec::new();
    for item in items {
        let Some(value) = item.as_str() else {
            return Err(format!("{key} must contain only strings"));
        };
        let value = value.trim();
        if !value.is_empty() {
            values.push(value.to_string());
        }
    }
    Ok(values)
}

fn put_number(map: &mut Mapping, key: &str, number: i64) {
    map.insert(
        Value::String(key.to_string()),
        Value::Number(Number::from(number)),
    );
}

fn put_string(map: &mut Mapping, key: &str, value: &str) {
    map.insert(
        Value::String(key.to_string()),
        Value::String(value.to_string()),
    );
}

fn put_bool(map: &mut Mapping, key: &str, value: bool) {
    map.insert(Value::String(key.to_string()), Value::Bool(value));
}

fn put_string_list(map: &mut Mapping, key: &str, values: &[String]) {
    map.insert(
        Value::String(key.to_string()),
        Value::Sequence(values.iter().cloned().map(Value::String).collect()),
    );
}

fn replace_shared_rules(
    map: &mut Mapping,
    old_shared_rules: &str,
    new_shared_rules: &str,
) -> Result<(), String> {
    let mut removable = active_shared_rule_lines(old_shared_rules)?;
    for rule in active_shared_rule_lines(new_shared_rules)? {
        if !removable.iter().any(|item| item == &rule) {
            removable.push(rule);
        }
    }
    let new_rules = active_shared_rule_lines(new_shared_rules)?;
    if removable.is_empty() && new_rules.is_empty() {
        return Ok(());
    }

    let rules_key = Value::String("rules".to_string());
    let mut existing = match map.remove(&rules_key) {
        Some(Value::Sequence(items)) => items,
        Some(_) => return Err("rules must be a list".to_string()),
        None => Vec::new(),
    };
    existing.retain(|item| match item.as_str() {
        Some(rule) => !removable.iter().any(|value| value == rule.trim()),
        None => true,
    });

    let mut rules = new_rules.into_iter().map(Value::String).collect::<Vec<_>>();
    rules.extend(existing);
    map.insert(rules_key, Value::Sequence(rules));
    Ok(())
}

fn normalize_shared_rules_text(input: &str) -> Result<String, String> {
    if input.len() > MAX_SHARED_RULES_BYTES {
        return Err(format!(
            "shared rules is too large: {} bytes, max {} bytes",
            input.len(),
            MAX_SHARED_RULES_BYTES
        ));
    }

    let mut lines = Vec::new();
    for line in input.replace("\r\n", "\n").replace('\r', "\n").lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if line
            .chars()
            .any(|ch| ch.is_control() && !matches!(ch, '\t'))
        {
            return Err("shared rules cannot contain control characters".to_string());
        }
        lines.push(line.to_string());
    }

    if lines.is_empty() {
        Ok(String::new())
    } else {
        Ok(format!("{}\n", lines.join("\n")))
    }
}

fn active_shared_rule_lines(input: &str) -> Result<Vec<String>, String> {
    let normalized = normalize_shared_rules_text(input)?;
    let mut rules = Vec::new();
    for line in normalized.lines() {
        if line.starts_with('#') {
            continue;
        }
        if !rules.iter().any(|item| item == line) {
            rules.push(line.to_string());
        }
    }
    Ok(rules)
}

fn normalize_excludes(values: Vec<String>) -> Vec<String> {
    let mut normalized = Vec::new();
    for value in values {
        let value = value.trim();
        if is_valid_route_exclude(value) && !normalized.iter().any(|item| item == value) {
            normalized.push(value.to_string());
        }
    }
    normalized
}

fn is_valid_route_exclude(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 80
        && value
            .chars()
            .all(|ch| ch.is_ascii_hexdigit() || matches!(ch, '.' | ':' | '/'))
}

fn is_ssh_safe_exclude(value: &str) -> bool {
    BASE_TUN_EXCLUDES.iter().any(|item| value == *item)
        || value.ends_with("/32")
        || value.ends_with("/128")
}

#[derive(Debug, Deserialize)]
struct GitHubRelease {
    assets: Vec<GitHubAsset>,
}

#[derive(Debug, Deserialize)]
struct GitHubAsset {
    name: String,
    browser_download_url: String,
    digest: Option<String>,
}

async fn latest_asset_for_arch(
    client: &reqwest::Client,
    arch: &str,
) -> Result<GitHubAsset, String> {
    let release: GitHubRelease = client
        .get(MIHOMO_RELEASE_API)
        .header("User-Agent", "mihomo-server-manager")
        .send()
        .await
        .map_err(|err| err.to_string())?
        .error_for_status()
        .map_err(|err| err.to_string())?
        .json()
        .await
        .map_err(|err| err.to_string())?;

    let needle = match arch {
        "x86_64" | "amd64" => "linux-amd64-compatible",
        "aarch64" | "arm64" => "linux-arm64",
        other => return Err(format!("unsupported architecture: {other}")),
    };

    release
        .assets
        .into_iter()
        .find(|asset| {
            let name = asset.name.to_ascii_lowercase();
            name.contains(needle) && name.ends_with(".gz")
        })
        .ok_or_else(|| format!("no mihomo release asset matched {needle}"))
}

async fn download_asset(
    client: &reqwest::Client,
    asset: &GitHubAsset,
    path: &Path,
) -> Result<(), String> {
    let response = client
        .get(&asset.browser_download_url)
        .header("User-Agent", "mihomo-server-manager")
        .send()
        .await
        .map_err(|err| err.to_string())?
        .error_for_status()
        .map_err(|err| err.to_string())?;
    if let Some(length) = response.content_length() {
        if length > MAX_MIHOMO_ARCHIVE_BYTES {
            return Err(format!("mihomo archive is too large: {length} bytes"));
        }
    }

    let bytes = response.bytes().await.map_err(|err| err.to_string())?;
    if bytes.len() as u64 > MAX_MIHOMO_ARCHIVE_BYTES {
        return Err(format!(
            "mihomo archive is too large: {} bytes",
            bytes.len()
        ));
    }
    verify_asset_digest(asset.digest.as_deref(), &bytes)?;

    let path = path.to_path_buf();
    blocking(move || std::fs::write(path, bytes).map_err(|err| err.to_string())).await
}

async fn download_limited_file(
    client: &reqwest::Client,
    url: &str,
    path: &Path,
    max_bytes: u64,
) -> Result<(), String> {
    let response = client
        .get(url)
        .header("User-Agent", "mihomo-server-manager")
        .send()
        .await
        .map_err(|err| err.to_string())?
        .error_for_status()
        .map_err(|err| err.to_string())?;
    if let Some(length) = response.content_length() {
        if length > max_bytes {
            return Err(format!("download is too large: {length} bytes"));
        }
    }
    let bytes = response.bytes().await.map_err(|err| err.to_string())?;
    if bytes.len() as u64 > max_bytes {
        return Err(format!("download is too large: {} bytes", bytes.len()));
    }

    let path = path.to_path_buf();
    blocking(move || std::fs::write(path, bytes).map_err(|err| err.to_string())).await
}

fn verify_asset_digest(expected: Option<&str>, bytes: &[u8]) -> Result<(), String> {
    let expected = expected.ok_or_else(|| "release asset is missing SHA256 digest".to_string())?;
    let Some(expected) = expected.strip_prefix("sha256:") else {
        return Err("release asset digest must use sha256".to_string());
    };
    if expected.len() != 64 || !expected.chars().all(|ch| ch.is_ascii_hexdigit()) {
        return Err("release asset SHA256 digest is invalid".to_string());
    }

    let mut hasher = Sha256::new();
    hasher.update(bytes);
    let actual = format!("{:x}", hasher.finalize());
    if actual.eq_ignore_ascii_case(expected) {
        Ok(())
    } else {
        Err("mihomo archive SHA256 digest mismatch".to_string())
    }
}

async fn fetch_shared_rules_text(server: &Server) -> Result<String, String> {
    let output = run_ssh_script_blocking(
        server,
        format!("test -f {SHARED_RULES_PATH} && cat {SHARED_RULES_PATH} || true"),
        Duration::from_secs(20),
    )
    .await?;
    if output.ok {
        Ok(output.stdout)
    } else {
        Err(output.stderr)
    }
}

async fn run_ssh_script_blocking(
    server: &Server,
    script: String,
    timeout: Duration,
) -> Result<CommandResult, String> {
    let server = server.clone();
    blocking(move || ssh::run_ssh_script(&server, &script, timeout)).await
}

async fn scp_to_remote_blocking(
    server: &Server,
    local_path: PathBuf,
    remote_path: String,
    timeout: Duration,
) -> Result<CommandResult, String> {
    let server = server.clone();
    blocking(move || ssh::scp_to_remote(&server, &local_path, &remote_path, timeout)).await
}

async fn scp_from_remote_blocking(
    server: &Server,
    remote_path: String,
    local_path: PathBuf,
    timeout: Duration,
) -> Result<CommandResult, String> {
    let server = server.clone();
    blocking(move || ssh::scp_from_remote(&server, &remote_path, &local_path, timeout)).await
}

async fn decompress_gzip_blocking(input: PathBuf, output: PathBuf) -> Result<(), String> {
    blocking(move || decompress_gzip(&input, &output)).await
}

async fn patch_config_file_with_shared_rules_blocking(
    input: PathBuf,
    output: PathBuf,
    shared_rules: String,
) -> Result<(), String> {
    blocking(move || patch_config_file_with_shared_rules(&input, &output, &shared_rules)).await
}

async fn patch_config_file_replacing_shared_rules_blocking(
    input: PathBuf,
    output: PathBuf,
    old_shared_rules: String,
    new_shared_rules: String,
) -> Result<(), String> {
    blocking(move || {
        let content = std::fs::read_to_string(&input).map_err(|err| err.to_string())?;
        let patched = patch_config_yaml_replacing_shared_rules(
            &content,
            &old_shared_rules,
            &new_shared_rules,
        )?;
        std::fs::write(output, patched).map_err(|err| err.to_string())
    })
    .await
}

async fn patch_tun_config_file_blocking(
    input: PathBuf,
    output: PathBuf,
    enabled: bool,
    ssh_excludes: Vec<String>,
) -> Result<(), String> {
    blocking(move || {
        let content = std::fs::read_to_string(&input).map_err(|err| err.to_string())?;
        let patched = patch_tun_config_yaml(&content, enabled, &ssh_excludes)?;
        std::fs::write(output, patched).map_err(|err| err.to_string())
    })
    .await
}

async fn preserve_tun_config_file_blocking(current: PathBuf, next: PathBuf) -> Result<(), String> {
    blocking(move || {
        let current_content = std::fs::read_to_string(current).map_err(|err| err.to_string())?;
        let next_content = std::fs::read_to_string(&next).map_err(|err| err.to_string())?;
        let patched = preserve_tun_config_yaml(&current_content, &next_content)?;
        std::fs::write(next, patched).map_err(|err| err.to_string())
    })
    .await
}

async fn blocking<T, F>(operation: F) -> Result<T, String>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T, String> + Send + 'static,
{
    tokio::task::spawn_blocking(operation)
        .await
        .map_err(|err| format!("blocking task failed: {err}"))?
}

fn decompress_gzip(input: &Path, output: &Path) -> Result<(), String> {
    let mut decoder = GzDecoder::new(File::open(input).map_err(|err| err.to_string())?);
    let mut out = File::create(output).map_err(|err| err.to_string())?;
    io::copy(&mut decoder, &mut out).map_err(|err| err.to_string())?;
    Ok(())
}

fn shell_safe_multiline(input: &str) -> String {
    input.lines().next().unwrap_or("").trim().replace('\r', "")
}

#[cfg(test)]
mod tests {
    use super::{
        geodata_assets_for_error, parse_tun_config_yaml, patch_config_yaml_replacing_shared_rules,
        patch_config_yaml_with_shared_rules, patch_tun_config_yaml, preserve_tun_config_yaml,
        verify_asset_digest,
    };
    use crate::models::CommandResult;

    #[test]
    fn patches_required_keys_and_preserves_allow_lan() {
        let patched = patch_config_yaml_with_shared_rules(
            r#"
port: 7891
allow-lan: true
proxies: []
"#,
            "",
        )
        .unwrap();

        assert!(patched.contains("mixed-port: 7890"));
        assert!(patched.contains("external-controller: 127.0.0.1:9090"));
        assert!(patched.contains("geo-auto-update: false"));
        assert!(patched.contains("allow-lan: true"));
    }

    #[test]
    fn defaults_allow_lan_to_false_when_missing() {
        let patched = patch_config_yaml_with_shared_rules("proxies: []", "").unwrap();
        assert!(patched.contains("allow-lan: false"));
    }

    #[test]
    fn prepends_shared_rules_and_deduplicates_existing_rules() {
        let patched = patch_config_yaml_with_shared_rules(
            r#"
proxies: []
rules:
  - PROCESS-NAME,curl,DIRECT
  - MATCH,GLOBAL
"#,
            r#"
# local process rules
PROCESS-NAME,curl,DIRECT
IP-CIDR,1.1.1.1/32,DIRECT,no-resolve
"#,
        )
        .unwrap();

        let curl = patched.find("PROCESS-NAME,curl,DIRECT").unwrap();
        let ip = patched
            .find("IP-CIDR,1.1.1.1/32,DIRECT,no-resolve")
            .unwrap();
        let matched = patched.find("MATCH,GLOBAL").unwrap();
        assert!(curl < matched);
        assert!(ip < matched);
        assert_eq!(patched.matches("PROCESS-NAME,curl,DIRECT").count(), 1);
    }

    #[test]
    fn replaces_old_shared_rules_when_saving_new_rules() {
        let patched = patch_config_yaml_replacing_shared_rules(
            r#"
proxies: []
rules:
  - PROCESS-NAME,curl,DIRECT
  - MATCH,GLOBAL
"#,
            "PROCESS-NAME,curl,DIRECT\n",
            "",
        )
        .unwrap();

        assert!(!patched.contains("PROCESS-NAME,curl,DIRECT"));
        assert!(patched.contains("MATCH,GLOBAL"));
    }

    #[test]
    fn enables_tun_with_ssh_safe_excludes() {
        let patched = patch_tun_config_yaml(
            r#"
mixed-port: 7890
tun:
  enable: false
  route-exclude-address:
    - 203.0.113.9/32
proxies: []
"#,
            true,
            &["10.40.2.10/32".to_string(), "10.40.2.39/24".to_string()],
        )
        .unwrap();
        let tun = parse_tun_config_yaml(&patched).unwrap().unwrap();

        assert!(tun.enabled);
        assert_eq!(tun.stack.as_deref(), Some("system"));
        assert_eq!(tun.auto_route, Some(true));
        assert_eq!(tun.auto_detect_interface, Some(true));
        assert!(tun
            .route_exclude_address
            .iter()
            .any(|value| value == "10.40.2.10/32"));
        assert!(tun
            .route_exclude_address
            .iter()
            .any(|value| value == "10.0.0.0/8"));
        assert!(tun
            .route_exclude_address
            .iter()
            .any(|value| value == "203.0.113.9/32"));
    }

    #[test]
    fn disables_tun_without_removing_existing_options() {
        let patched = patch_tun_config_yaml(
            r#"
tun:
  enable: true
  stack: mixed
  auto-route: true
"#,
            false,
            &[],
        )
        .unwrap();
        let tun = parse_tun_config_yaml(&patched).unwrap().unwrap();

        assert!(!tun.enabled);
        assert_eq!(tun.stack.as_deref(), Some("mixed"));
        assert_eq!(tun.auto_route, Some(true));
    }

    #[test]
    fn preserves_tun_when_subscription_config_is_replaced() {
        let current = r#"
mixed-port: 7890
tun:
  enable: true
  stack: system
  route-exclude-address:
    - 10.40.2.10/32
"#;
        let next = r#"
proxies: []
rules:
  - MATCH,DIRECT
"#;
        let patched = preserve_tun_config_yaml(current, next).unwrap();
        let tun = parse_tun_config_yaml(&patched).unwrap().unwrap();

        assert!(tun.enabled);
        assert!(tun
            .route_exclude_address
            .iter()
            .any(|value| value == "10.40.2.10/32"));
    }

    #[test]
    fn verifies_release_asset_sha256_digest() {
        let empty_sha256 = "sha256:e3b0c44298fc1c149afbf4c8996fb924\
                            27ae41e4649b934ca495991b7852b855";
        assert!(verify_asset_digest(Some(empty_sha256), b"").is_ok());
        assert!(verify_asset_digest(Some(empty_sha256), b"changed").is_err());
        assert!(verify_asset_digest(None, b"").is_err());
    }

    #[test]
    fn detects_missing_geodata_assets_from_mihomo_errors() {
        let assets = geodata_assets_for_error(&CommandResult {
            ok: false,
            code: Some(1),
            stdout: "can't initial GeoIP: can't download MMDB".to_string(),
            stderr: "DNS FallbackGeosite[0] format error: can't download GeoSite.dat".to_string(),
        });
        let names = assets
            .iter()
            .map(|asset| asset.file_name)
            .collect::<Vec<_>>();

        assert_eq!(names, vec!["geoip.metadb", "GeoSite.dat"]);
    }
}
