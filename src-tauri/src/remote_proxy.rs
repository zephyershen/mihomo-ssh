use std::time::Duration;

use crate::{
    models::{CommandResult, RemoteProxyConfig, RemoteProxyEnvVar, RemoteProxyInput, Server},
    ssh,
};

const PROFILE_PATH: &str = "/etc/profile.d/mihomo-manager-proxy.sh";
const DEFAULT_HTTP_PROXY: &str = "http://127.0.0.1:7890";
const DEFAULT_ALL_PROXY: &str = "socks5h://127.0.0.1:7890";
const DEFAULT_NO_PROXY: &str = "localhost,127.0.0.1,::1,10.40.2.0/24";

pub fn inspect(server: &Server) -> Result<RemoteProxyConfig, String> {
    let script = format!(
        r#"
set +e
profile={profile_path}
kv() {{ printf '%s=%s\n' "$1" "$2"; }}
kv profile_path "$profile"
if [ -f "$profile" ]; then
  kv managed true
  if grep -q '^# mihomo-manager-enabled=true$' "$profile"; then
    kv enabled true
  else
    kv enabled false
  fi
  marker() {{
    sed -n "s/^# mihomo-manager-$1=//p" "$profile" | tail -n 1
  }}
  kv http_proxy "$(marker http-proxy)"
  kv https_proxy "$(marker https-proxy)"
  kv all_proxy "$(marker all-proxy)"
  kv no_proxy "$(marker no-proxy)"
else
  kv managed false
  kv enabled false
fi
bash -lc 'env | grep -Ei "^(http_proxy|https_proxy|all_proxy|no_proxy|HTTP_PROXY|HTTPS_PROXY|ALL_PROXY|NO_PROXY)=" || true' |
while IFS= read -r line; do
  kv env "$line"
done
"#,
        profile_path = shell_quote(PROFILE_PATH)
    );

    let output = ssh::run_ssh_script(server, &script, Duration::from_secs(20))?;
    if !output.ok {
        return Err(output.stderr);
    }
    Ok(parse_proxy_config(&output.stdout))
}

pub fn save(server: &Server, input: RemoteProxyInput) -> Result<CommandResult, String> {
    let input = normalize_input(input)?;
    let profile = render_profile(&input);
    let script = format!(
        r#"
set -euo pipefail
install -d -m 0755 /etc/profile.d
cat > {profile_path} <<'PROXYEOF'
{profile}
PROXYEOF
chmod 0644 {profile_path}
printf 'remote proxy %s\n' "{state}"
printf 'profile: {raw_profile_path}\n'
printf 'new SSH login shells will use this configuration\n'
"#,
        profile_path = shell_quote(PROFILE_PATH),
        raw_profile_path = PROFILE_PATH,
        profile = profile,
        state = if input.enabled { "enabled" } else { "disabled" },
    );
    ssh::run_ssh_script(server, &script, Duration::from_secs(20))
}

pub fn set_enabled(server: &Server, enabled: bool) -> Result<CommandResult, String> {
    let current = inspect(server)?;
    let input = RemoteProxyInput {
        enabled,
        http_proxy: current
            .http_proxy
            .unwrap_or_else(|| DEFAULT_HTTP_PROXY.to_string()),
        https_proxy: current
            .https_proxy
            .unwrap_or_else(|| DEFAULT_HTTP_PROXY.to_string()),
        all_proxy: current
            .all_proxy
            .unwrap_or_else(|| DEFAULT_ALL_PROXY.to_string()),
        no_proxy: current
            .no_proxy
            .unwrap_or_else(|| DEFAULT_NO_PROXY.to_string()),
    };
    save(server, input)
}

fn empty_config() -> RemoteProxyConfig {
    RemoteProxyConfig {
        enabled: false,
        managed: false,
        profile_path: PROFILE_PATH.to_string(),
        http_proxy: None,
        https_proxy: None,
        all_proxy: None,
        no_proxy: None,
        detected_env: Vec::new(),
    }
}

fn parse_proxy_config(stdout: &str) -> RemoteProxyConfig {
    let mut config = empty_config();
    let mut marker_enabled: Option<bool> = None;

    for line in stdout.lines() {
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        match key {
            "profile_path" => config.profile_path = value.to_string(),
            "managed" => config.managed = value == "true",
            "enabled" => marker_enabled = Some(value == "true"),
            "http_proxy" => config.http_proxy = non_empty(value),
            "https_proxy" => config.https_proxy = non_empty(value),
            "all_proxy" => config.all_proxy = non_empty(value),
            "no_proxy" => config.no_proxy = non_empty(value),
            "env" => {
                if let Some((name, env_value)) = value.split_once('=') {
                    set_detected_value(&mut config, name, env_value);
                    config.detected_env.push(RemoteProxyEnvVar {
                        name: name.to_string(),
                        value: env_value.to_string(),
                    });
                }
            }
            _ => {}
        }
    }

    config.enabled = marker_enabled.unwrap_or_else(|| !config.detected_env.is_empty());
    fill_default_values(&mut config);
    config
}

fn fill_default_values(config: &mut RemoteProxyConfig) {
    if config.http_proxy.is_none() {
        config.http_proxy = Some(DEFAULT_HTTP_PROXY.to_string());
    }
    if config.https_proxy.is_none() {
        config.https_proxy = Some(DEFAULT_HTTP_PROXY.to_string());
    }
    if config.all_proxy.is_none() {
        config.all_proxy = Some(DEFAULT_ALL_PROXY.to_string());
    }
    if config.no_proxy.is_none() {
        config.no_proxy = Some(DEFAULT_NO_PROXY.to_string());
    }
}

fn set_detected_value(config: &mut RemoteProxyConfig, name: &str, value: &str) {
    if value.trim().is_empty() {
        return;
    }
    match name.to_ascii_lowercase().as_str() {
        "http_proxy" if config.http_proxy.is_none() => config.http_proxy = Some(value.to_string()),
        "https_proxy" if config.https_proxy.is_none() => {
            config.https_proxy = Some(value.to_string())
        }
        "all_proxy" if config.all_proxy.is_none() => config.all_proxy = Some(value.to_string()),
        "no_proxy" if config.no_proxy.is_none() => config.no_proxy = Some(value.to_string()),
        _ => {}
    }
}

fn normalize_input(mut input: RemoteProxyInput) -> Result<RemoteProxyInput, String> {
    input.http_proxy = clean_value("http proxy", &input.http_proxy)?;
    input.https_proxy = clean_value("https proxy", &input.https_proxy)?;
    input.all_proxy = clean_value("all proxy", &input.all_proxy)?;
    input.no_proxy = clean_value("no proxy", &input.no_proxy)?;

    if input.enabled
        && input.http_proxy.is_empty()
        && input.https_proxy.is_empty()
        && input.all_proxy.is_empty()
    {
        return Err("至少填写一个代理地址".to_string());
    }
    Ok(input)
}

fn clean_value(label: &str, value: &str) -> Result<String, String> {
    let value = value.trim();
    if value.chars().any(char::is_control) || value.chars().any(char::is_whitespace) {
        return Err(format!("{label} 不能包含空格或换行"));
    }
    Ok(value.to_string())
}

fn render_profile(input: &RemoteProxyInput) -> String {
    let mut lines = vec![
        "# Managed by Mihomo Manager. Manual edits may be overwritten.".to_string(),
        format!("# mihomo-manager-enabled={}", input.enabled),
        format!("# mihomo-manager-http-proxy={}", input.http_proxy),
        format!("# mihomo-manager-https-proxy={}", input.https_proxy),
        format!("# mihomo-manager-all-proxy={}", input.all_proxy),
        format!("# mihomo-manager-no-proxy={}", input.no_proxy),
        String::new(),
    ];

    if input.enabled {
        push_export_pair(&mut lines, "http_proxy", "HTTP_PROXY", &input.http_proxy);
        push_export_pair(&mut lines, "https_proxy", "HTTPS_PROXY", &input.https_proxy);
        push_export_pair(&mut lines, "all_proxy", "ALL_PROXY", &input.all_proxy);
        push_export_pair(&mut lines, "no_proxy", "NO_PROXY", &input.no_proxy);
    } else {
        lines.push(
            "unset http_proxy HTTP_PROXY https_proxy HTTPS_PROXY all_proxy ALL_PROXY no_proxy NO_PROXY"
                .to_string(),
        );
    }

    lines.push(String::new());
    lines.join("\n")
}

fn push_export_pair(lines: &mut Vec<String>, lower: &str, upper: &str, value: &str) {
    if value.is_empty() {
        lines.push(format!("unset {lower} {upper}"));
        return;
    }
    lines.push(format!("export {lower}={}", shell_quote(value)));
    lines.push(format!("export {upper}=\"${lower}\""));
}

fn non_empty(value: &str) -> Option<String> {
    let value = value.trim();
    if value.is_empty() {
        None
    } else {
        Some(value.to_string())
    }
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}
