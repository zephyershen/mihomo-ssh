use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Server {
    pub id: i64,
    pub alias: String,
    pub display_name: String,
    pub host_name: String,
    pub user: Option<String>,
    pub port: Option<u16>,
    pub identity_file_hint: Option<String>,
    pub source: String,
    pub last_status: Option<String>,
    pub last_seen_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportedHost {
    pub alias: String,
    pub host_name: String,
    pub user: Option<String>,
    pub port: Option<u16>,
    pub identity_file_hint: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ServerHealth {
    pub os_pretty_name: Option<String>,
    pub os_id: Option<String>,
    pub arch: Option<String>,
    pub has_systemd: bool,
    pub mihomo_path: Option<String>,
    pub mihomo_version: Option<String>,
    pub service_active: Option<String>,
    pub service_enabled: Option<String>,
    pub has_config: bool,
    pub has_subscription: bool,
    pub mixed_port: Option<u16>,
    pub controller: Option<String>,
    pub allow_lan: Option<bool>,
    pub geo_auto_update: Option<bool>,
    pub config_preview: Option<String>,
    pub checked_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OperationLog {
    pub id: i64,
    pub server_id: Option<i64>,
    pub action: String,
    pub status: String,
    pub message: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CommandResult {
    pub ok: bool,
    pub code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServiceCommandResult {
    pub state: String,
    pub output: CommandResult,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProxyNode {
    pub name: String,
    pub node_type: Option<String>,
    pub udp: Option<bool>,
    pub delay_ms: Option<u64>,
    pub alive: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProxyGroup {
    pub name: String,
    pub now: Option<String>,
    pub nodes: Vec<ProxyNode>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TunnelInfo {
    pub server_id: i64,
    pub local_port: u16,
    pub status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallOptions {
    pub subscription_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubscriptionUpdateOptions {
    pub subscription_url: Option<String>,
}
