use std::{collections::HashMap, net::TcpListener, process::Child, sync::Mutex, time::Duration};

use futures_util::{stream, StreamExt};
use serde_json::{json, Value};
use tokio::time::sleep;

use crate::{
    models::{ProxyGroup, ProxyNode, Server, TunnelInfo},
    ssh,
};

const MAX_CONCURRENT_NODE_DELAY_CHECKS: usize = 8;

#[derive(Default)]
pub struct TunnelRegistry {
    children: Mutex<HashMap<i64, TunnelProcess>>,
}

struct TunnelProcess {
    local_port: u16,
    child: Child,
}

impl TunnelRegistry {
    pub fn open(&self, server: &Server) -> Result<TunnelInfo, String> {
        if let Some(existing) = self.existing(server.id)? {
            return Ok(existing);
        }

        let local_port = free_local_port()?;
        let child = ssh::spawn_tunnel(server, local_port)?;
        let process = TunnelProcess { local_port, child };

        self.children
            .lock()
            .map_err(|_| "tunnel registry lock poisoned".to_string())?
            .insert(server.id, process);

        Ok(TunnelInfo {
            server_id: server.id,
            local_port,
            status: "opening".to_string(),
        })
    }

    pub fn close(&self, server_id: i64) -> Result<TunnelInfo, String> {
        let mut children = self
            .children
            .lock()
            .map_err(|_| "tunnel registry lock poisoned".to_string())?;
        let Some(mut process) = children.remove(&server_id) else {
            return Ok(TunnelInfo {
                server_id,
                local_port: 0,
                status: "closed".to_string(),
            });
        };
        let _ = process.child.kill();
        let _ = process.child.wait();
        Ok(TunnelInfo {
            server_id,
            local_port: process.local_port,
            status: "closed".to_string(),
        })
    }

    pub fn port(&self, server_id: i64) -> Result<Option<u16>, String> {
        self.prune_dead()?;
        Ok(self
            .children
            .lock()
            .map_err(|_| "tunnel registry lock poisoned".to_string())?
            .get(&server_id)
            .map(|process| process.local_port))
    }

    fn existing(&self, server_id: i64) -> Result<Option<TunnelInfo>, String> {
        self.prune_dead()?;
        Ok(self
            .children
            .lock()
            .map_err(|_| "tunnel registry lock poisoned".to_string())?
            .get(&server_id)
            .map(|process| TunnelInfo {
                server_id,
                local_port: process.local_port,
                status: "open".to_string(),
            }))
    }

    fn prune_dead(&self) -> Result<(), String> {
        let mut children = self
            .children
            .lock()
            .map_err(|_| "tunnel registry lock poisoned".to_string())?;
        let mut dead = Vec::new();
        for (server_id, process) in children.iter_mut() {
            if process
                .child
                .try_wait()
                .map_err(|err| err.to_string())?
                .is_some()
            {
                dead.push(*server_id);
            }
        }
        for server_id in dead {
            children.remove(&server_id);
        }
        Ok(())
    }
}

impl Drop for TunnelRegistry {
    fn drop(&mut self) {
        if let Ok(children) = self.children.get_mut() {
            for process in children.values_mut() {
                let _ = process.child.kill();
                let _ = process.child.wait();
            }
            children.clear();
        }
    }
}

pub async fn list_proxy_groups(port: u16) -> Result<Vec<ProxyGroup>, String> {
    let client = reqwest::Client::new();
    list_proxy_groups_with_client(&client, port).await
}

async fn list_proxy_groups_with_client(
    client: &reqwest::Client,
    port: u16,
) -> Result<Vec<ProxyGroup>, String> {
    wait_for_controller(client, port).await?;
    let url = format!("http://127.0.0.1:{port}/proxies");
    let data: Value = client
        .get(&url)
        .send()
        .await
        .map_err(|err| err.to_string())?
        .error_for_status()
        .map_err(|err| err.to_string())?
        .json()
        .await
        .map_err(|err| err.to_string())?;

    parse_proxy_groups(&data)
}

pub async fn select_proxy_node(port: u16, group: &str, node: &str) -> Result<(), String> {
    let group = urlencoding::encode(group);
    let url = format!("http://127.0.0.1:{port}/proxies/{group}");
    reqwest::Client::new()
        .put(url)
        .json(&json!({ "name": node }))
        .send()
        .await
        .map_err(|err| err.to_string())?
        .error_for_status()
        .map_err(|err| err.to_string())?;
    Ok(())
}

pub async fn measure_proxy_delay(port: u16, group: &str) -> Result<Vec<ProxyNode>, String> {
    let client = reqwest::Client::new();
    let groups = list_proxy_groups_with_client(&client, port).await?;
    let Some(target) = groups.into_iter().find(|item| item.name == group) else {
        return Err(format!("proxy group not found: {group}"));
    };

    let test_url = urlencoding::encode("https://www.gstatic.com/generate_204").into_owned();
    let mut measured = stream::iter(target.nodes.into_iter().enumerate())
        .map(|(index, node)| {
            let client = client.clone();
            let test_url = test_url.clone();
            async move {
                let node = measure_node_delay(&client, port, &test_url, node).await;
                (index, node)
            }
        })
        .buffer_unordered(MAX_CONCURRENT_NODE_DELAY_CHECKS)
        .collect::<Vec<_>>()
        .await;

    measured.sort_by_key(|(index, _)| *index);
    Ok(measured.into_iter().map(|(_, node)| node).collect())
}

pub async fn measure_proxy_node_delay(port: u16, node: &str) -> Result<ProxyNode, String> {
    let client = reqwest::Client::new();
    wait_for_controller(&client, port).await?;
    let test_url = urlencoding::encode("https://www.gstatic.com/generate_204").into_owned();
    let node = ProxyNode {
        name: node.to_string(),
        node_type: None,
        udp: None,
        delay_ms: None,
        alive: None,
    };
    Ok(measure_node_delay(&client, port, &test_url, node).await)
}

async fn measure_node_delay(
    client: &reqwest::Client,
    port: u16,
    test_url: &str,
    mut node: ProxyNode,
) -> ProxyNode {
    let encoded = urlencoding::encode(&node.name);
    let url =
        format!("http://127.0.0.1:{port}/proxies/{encoded}/delay?timeout=5000&url={test_url}");

    match client.get(url).send().await {
        Ok(response) if response.status().is_success() => {
            let value: Value = response.json().await.unwrap_or(Value::Null);
            node.delay_ms = value.get("delay").and_then(Value::as_u64);
            node.alive = Some(node.delay_ms.is_some());
        }
        Ok(response) => {
            node.delay_ms = None;
            node.alive = Some(response.status().is_success());
        }
        Err(_) => {
            node.delay_ms = None;
            node.alive = Some(false);
        }
    }

    node
}

pub fn parse_proxy_groups(data: &Value) -> Result<Vec<ProxyGroup>, String> {
    let proxies = data
        .get("proxies")
        .and_then(Value::as_object)
        .ok_or_else(|| "controller response missing proxies".to_string())?;

    let mut groups = Vec::new();
    for (name, item) in proxies {
        let Some(all) = item.get("all").and_then(Value::as_array) else {
            continue;
        };
        let nodes = all
            .iter()
            .filter_map(Value::as_str)
            .map(|node_name| {
                let detail = proxies.get(node_name);
                ProxyNode {
                    name: node_name.to_string(),
                    node_type: detail
                        .and_then(|value| value.get("type"))
                        .and_then(Value::as_str)
                        .map(ToString::to_string),
                    udp: detail
                        .and_then(|value| value.get("udp"))
                        .and_then(Value::as_bool),
                    delay_ms: detail
                        .and_then(|value| value.get("history"))
                        .and_then(Value::as_array)
                        .and_then(|history| history.last())
                        .and_then(|last| last.get("delay"))
                        .and_then(Value::as_u64),
                    alive: None,
                }
            })
            .collect::<Vec<_>>();

        groups.push(ProxyGroup {
            name: name.to_string(),
            now: item
                .get("now")
                .and_then(Value::as_str)
                .map(ToString::to_string),
            nodes,
        });
    }

    groups.sort_by_cached_key(|group| group.name.to_ascii_lowercase());
    Ok(groups)
}

async fn wait_for_controller(client: &reqwest::Client, port: u16) -> Result<(), String> {
    let url = format!("http://127.0.0.1:{port}/version");
    for _ in 0..20 {
        if let Ok(response) = client.get(&url).send().await {
            if response.status().is_success() {
                if let Ok(value) = response.json::<Value>().await {
                    if value.get("version").and_then(Value::as_str).is_some() {
                        return Ok(());
                    }
                }
            }
        }
        sleep(Duration::from_millis(150)).await;
    }
    Err("controller tunnel did not become ready".to_string())
}

fn free_local_port() -> Result<u16, String> {
    let listener = TcpListener::bind("127.0.0.1:0").map_err(|err| err.to_string())?;
    let port = listener.local_addr().map_err(|err| err.to_string())?.port();
    drop(listener);
    Ok(port)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::parse_proxy_groups;

    #[test]
    fn parses_proxy_groups_from_controller_response() {
        let data = json!({
            "proxies": {
                "Cyber Paws": {
                    "type": "Selector",
                    "now": "A",
                    "all": ["A", "B"]
                },
                "A": {
                    "type": "Shadowsocks",
                    "udp": true,
                    "history": [{ "delay": 123 }]
                },
                "B": {
                    "type": "Trojan",
                    "udp": false,
                    "history": []
                }
            }
        });

        let groups = parse_proxy_groups(&data).unwrap();
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].name, "Cyber Paws");
        assert_eq!(groups[0].now.as_deref(), Some("A"));
        assert_eq!(groups[0].nodes[0].delay_ms, Some(123));
    }
}
