use std::{
    fs::File,
    io::{Read, Write},
    path::Path,
    time::Duration,
};

use chrono::Utc;
use flate2::read::GzDecoder;
use serde::Deserialize;
use serde_yaml::{Mapping, Number, Value};
use tempfile::tempdir;

use crate::{
    models::{CommandResult, InstallOptions, Server, ServerHealth, ServiceCommandResult, SubscriptionUpdateOptions},
    ssh,
};

const MIHOMO_RELEASE_API: &str = "https://api.github.com/repos/MetaCubeX/mihomo/releases/latest";
const REMOTE_TMP_DIR: &str = "/tmp/mihomo-manager";

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

    let output = ssh::run_ssh_script(server, script, Duration::from_secs(20))?;
    if !output.ok {
        return Err(format!("inspect failed: {}", output.stderr));
    }

    Ok(parse_health(&output.stdout))
}

pub async fn read_remote_config(server: &Server) -> Result<String, String> {
    let output = ssh::run_ssh_script(
        server,
        "test -f /etc/mihomo/config.yaml && sed -n '1,260p' /etc/mihomo/config.yaml || true",
        Duration::from_secs(20),
    )?;
    if output.ok {
        Ok(output.stdout)
    } else {
        Err(output.stderr)
    }
}

pub async fn install_or_repair(server: &Server, options: InstallOptions) -> Result<CommandResult, String> {
    let health = inspect_server(server).await?;
    let arch = health.arch.unwrap_or_else(|| "x86_64".to_string());
    let asset = latest_asset_for_arch(&arch).await?;
    let should_update_subscription = options
        .subscription_url
        .as_deref()
        .map(|value| !value.trim().is_empty())
        .unwrap_or(false);

    let dir = tempdir().map_err(|err| err.to_string())?;
    let archive_path = dir.path().join("mihomo.gz");
    let binary_path = dir.path().join("mihomo");
    download_asset(&asset.browser_download_url, &archive_path).await?;
    decompress_gzip(&archive_path, &binary_path)?;

    let prep = ssh::run_ssh_script(
        server,
        &format!("set -euo pipefail\ninstall -d -m 0700 {REMOTE_TMP_DIR}"),
        Duration::from_secs(20),
    )?;
    if !prep.ok {
        return Ok(prep);
    }

    let uploaded = ssh::scp_to_remote(
        server,
        &binary_path,
        &format!("{REMOTE_TMP_DIR}/mihomo"),
        Duration::from_secs(90),
    )?;
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

    let mut result = ssh::run_ssh_script(server, &script, Duration::from_secs(90))?;
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
        result.stdout = format!("{}\n{}", result.stdout.trim_end(), update.stdout.trim_start());
        result.stderr = update.stderr;
    }
    Ok(result)
}

pub async fn update_subscription(
    server: &Server,
    options: SubscriptionUpdateOptions,
) -> Result<CommandResult, String> {
    if let Some(url) = options.subscription_url.as_deref() {
        if !url.trim().is_empty() {
            let script = format!(
                "set -euo pipefail\ninstall -d -m 0755 /etc/mihomo\ncat > /etc/mihomo/subscription.url <<'SUBEOF'\n{}\nSUBEOF\nchmod 600 /etc/mihomo/subscription.url\n",
                shell_safe_multiline(url)
            );
            let written = ssh::run_ssh_script(server, &script, Duration::from_secs(15))?;
            if !written.ok {
                return Ok(written);
            }
        }
    }

    let download_script = format!(
        r#"
set -euo pipefail
test -f /etc/mihomo/subscription.url
install -d -m 0700 {REMOTE_TMP_DIR}
sub="$(sed -n '1p' /etc/mihomo/subscription.url)"
curl -fsSL --connect-timeout 20 --max-time 90 --retry 2 \
  -A 'clash-verge' \
  --proxy http://127.0.0.1:7890 \
  -o {REMOTE_TMP_DIR}/config.yaml \
  "$sub"
test -s {REMOTE_TMP_DIR}/config.yaml
"#
    );
    let downloaded = ssh::run_ssh_script(server, &download_script, Duration::from_secs(120))?;
    if !downloaded.ok {
        return Ok(downloaded);
    }

    let dir = tempdir().map_err(|err| err.to_string())?;
    let local_config = dir.path().join("config.yaml");
    let patched_config = dir.path().join("config.patched.yaml");
    let pulled = ssh::scp_from_remote(
        server,
        &format!("{REMOTE_TMP_DIR}/config.yaml"),
        &local_config,
        Duration::from_secs(40),
    )?;
    if !pulled.ok {
        return Ok(pulled);
    }

    patch_config_file(&local_config, &patched_config)?;
    let pushed = ssh::scp_to_remote(
        server,
        &patched_config,
        &format!("{REMOTE_TMP_DIR}/config.patched.yaml"),
        Duration::from_secs(40),
    )?;
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

    ssh::run_ssh_script(server, &install_script, Duration::from_secs(90))
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

fn patch_config_file(input: &Path, output: &Path) -> Result<(), String> {
    let content = std::fs::read_to_string(input).map_err(|err| err.to_string())?;
    let patched = patch_config_yaml(&content)?;
    std::fs::write(output, patched).map_err(|err| err.to_string())
}

pub fn patch_config_yaml(content: &str) -> Result<String, String> {
    let mut value: Value = serde_yaml::from_str(content).map_err(|err| err.to_string())?;
    let map = value
        .as_mapping_mut()
        .ok_or_else(|| "subscription config must be a YAML mapping".to_string())?;

    put_number(map, "mixed-port", 7890);
    put_string(map, "external-controller", "127.0.0.1:9090");
    put_bool(map, "geo-auto-update", false);
    if !map.contains_key(&Value::String("allow-lan".to_string())) {
        put_bool(map, "allow-lan", false);
    }

    serde_yaml::to_string(&value).map_err(|err| err.to_string())
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

#[derive(Debug, Deserialize)]
struct GitHubRelease {
    assets: Vec<GitHubAsset>,
}

#[derive(Debug, Deserialize)]
struct GitHubAsset {
    name: String,
    browser_download_url: String,
}

async fn latest_asset_for_arch(arch: &str) -> Result<GitHubAsset, String> {
    let release: GitHubRelease = reqwest::Client::new()
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

async fn download_asset(url: &str, path: &Path) -> Result<(), String> {
    let bytes = reqwest::Client::new()
        .get(url)
        .header("User-Agent", "mihomo-server-manager")
        .send()
        .await
        .map_err(|err| err.to_string())?
        .error_for_status()
        .map_err(|err| err.to_string())?
        .bytes()
        .await
        .map_err(|err| err.to_string())?;

    std::fs::write(path, bytes).map_err(|err| err.to_string())
}

fn decompress_gzip(input: &Path, output: &Path) -> Result<(), String> {
    let mut decoder = GzDecoder::new(File::open(input).map_err(|err| err.to_string())?);
    let mut out = File::create(output).map_err(|err| err.to_string())?;
    let mut buffer = Vec::new();
    decoder.read_to_end(&mut buffer).map_err(|err| err.to_string())?;
    out.write_all(&buffer).map_err(|err| err.to_string())?;
    Ok(())
}

fn shell_safe_multiline(input: &str) -> String {
    input.lines().next().unwrap_or("").trim().replace('\r', "")
}

#[cfg(test)]
mod tests {
    use super::patch_config_yaml;

    #[test]
    fn patches_required_keys_and_preserves_allow_lan() {
        let patched = patch_config_yaml(
            r#"
port: 7891
allow-lan: true
proxies: []
"#,
        )
        .unwrap();

        assert!(patched.contains("mixed-port: 7890"));
        assert!(patched.contains("external-controller: 127.0.0.1:9090"));
        assert!(patched.contains("geo-auto-update: false"));
        assert!(patched.contains("allow-lan: true"));
    }

    #[test]
    fn defaults_allow_lan_to_false_when_missing() {
        let patched = patch_config_yaml("proxies: []").unwrap();
        assert!(patched.contains("allow-lan: false"));
    }
}
