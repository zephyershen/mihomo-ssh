use std::{
    env,
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::Duration,
};

use wait_timeout::ChildExt;

use crate::{
    models::{CommandResult, ImportedHost, Server},
    redaction::{identity_hint, redact},
};

pub fn importable_hosts_from_default_config() -> Result<Vec<ImportedHost>, String> {
    let path = default_ssh_config_path().ok_or_else(|| "could not locate SSH config".to_string())?;
    parse_ssh_config_file(&path)
}

pub fn default_ssh_config_path() -> Option<PathBuf> {
    if let Some(user_profile) = env::var_os("USERPROFILE") {
        let path = PathBuf::from(user_profile).join(".ssh").join("config");
        if path.exists() {
            return Some(path);
        }
    }

    if let (Some(home_drive), Some(home_path)) = (env::var_os("HOMEDRIVE"), env::var_os("HOMEPATH")) {
        let path = PathBuf::from(format!(
            "{}{}",
            home_drive.to_string_lossy(),
            home_path.to_string_lossy()
        ))
        .join(".ssh")
        .join("config");
        if path.exists() {
            return Some(path);
        }
    }

    if let Some(home) = env::var_os("HOME") {
        let path = PathBuf::from(home).join(".ssh").join("config");
        if path.exists() {
            return Some(path);
        }
    }

    None
}

pub fn parse_ssh_config_file(path: &Path) -> Result<Vec<ImportedHost>, String> {
    let content = fs::read_to_string(path).map_err(|err| err.to_string())?;
    Ok(parse_ssh_config(&content))
}

pub fn parse_ssh_config(content: &str) -> Vec<ImportedHost> {
    let mut hosts = Vec::new();
    let mut current: Option<ImportedHost> = None;

    for raw in content.lines() {
        let line = strip_comment(raw).trim();
        if line.is_empty() {
            continue;
        }

        let (key, value) = match split_directive(line) {
            Some(pair) => pair,
            None => continue,
        };

        if key.eq_ignore_ascii_case("host") {
            if let Some(host) = current.take() {
                hosts.push(host);
            }
            current = first_concrete_alias(value).map(|alias| ImportedHost {
                alias: alias.clone(),
                host_name: alias,
                user: None,
                port: None,
                identity_file_hint: None,
            });
            continue;
        }

        let Some(host) = current.as_mut() else {
            continue;
        };

        match key.to_ascii_lowercase().as_str() {
            "hostname" => host.host_name = unquote(value),
            "user" => host.user = Some(unquote(value)),
            "port" => host.port = value.parse::<u16>().ok(),
            "identityfile" => host.identity_file_hint = Some(identity_hint(&unquote(value))),
            _ => {}
        }
    }

    if let Some(host) = current.take() {
        hosts.push(host);
    }

    hosts
}

pub fn run_ssh_script(server: &Server, script: &str, timeout: Duration) -> Result<CommandResult, String> {
    let mut args = base_ssh_args();
    args.push(ssh_target(server));
    args.push("bash".to_string());
    args.push("-s".to_string());
    run_process(ssh_program(), &args, Some(script), timeout)
}

pub fn run_ssh_command(server: &Server, command: &str, timeout: Duration) -> Result<CommandResult, String> {
    let mut args = base_ssh_args();
    args.push(ssh_target(server));
    args.push(command.to_string());
    run_process(ssh_program(), &args, None, timeout)
}

pub fn scp_to_remote(
    server: &Server,
    local_path: &Path,
    remote_path: &str,
    timeout: Duration,
) -> Result<CommandResult, String> {
    let mut args = base_scp_args();
    args.push(local_path.to_string_lossy().to_string());
    args.push(format!("{}:{remote_path}", ssh_target(server)));
    run_process(scp_program(), &args, None, timeout)
}

pub fn scp_from_remote(
    server: &Server,
    remote_path: &str,
    local_path: &Path,
    timeout: Duration,
) -> Result<CommandResult, String> {
    let mut args = base_scp_args();
    args.push(format!("{}:{remote_path}", ssh_target(server)));
    args.push(local_path.to_string_lossy().to_string());
    run_process(scp_program(), &args, None, timeout)
}

pub fn spawn_tunnel(server: &Server, local_port: u16) -> Result<std::process::Child, String> {
    let mut args = base_ssh_args();
    args.push("-N".to_string());
    args.push("-L".to_string());
    args.push(format!("127.0.0.1:{local_port}:127.0.0.1:9090"));
    args.push(ssh_target(server));

    Command::new(ssh_program())
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|err| err.to_string())
}

fn run_process(
    program: &str,
    args: &[String],
    input: Option<&str>,
    timeout: Duration,
) -> Result<CommandResult, String> {
    let mut child = Command::new(program)
        .args(args)
        .stdin(if input.is_some() { Stdio::piped() } else { Stdio::null() })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|err| format!("{program} failed to start: {err}"))?;

    let mut stdout = child.stdout.take();
    let mut stderr = child.stderr.take();

    if let Some(input) = input {
        if let Some(mut stdin) = child.stdin.take() {
            stdin.write_all(input.as_bytes()).map_err(|err| err.to_string())?;
        }
    }

    let status = match child.wait_timeout(timeout).map_err(|err| err.to_string())? {
        Some(status) => status,
        None => {
            let _ = child.kill();
            let _ = child.wait();
            let stdout = read_pipe(&mut stdout)?;
            return Ok(CommandResult {
                ok: false,
                code: None,
                stdout: redact(&stdout),
                stderr: format!("timed out after {}s", timeout.as_secs()),
            });
        }
    };

    let stdout = read_pipe(&mut stdout)?;
    let stderr = read_pipe(&mut stderr)?;
    Ok(CommandResult {
        ok: status.success(),
        code: status.code(),
        stdout: redact(&stdout),
        stderr: redact(&stderr),
    })
}

fn read_pipe<T: Read>(pipe: &mut Option<T>) -> Result<String, String> {
    let mut buffer = String::new();
    if let Some(pipe) = pipe.as_mut() {
        pipe.read_to_string(&mut buffer).map_err(|err| err.to_string())?;
    }
    Ok(buffer)
}

fn base_ssh_args() -> Vec<String> {
    vec![
        "-o".to_string(),
        "BatchMode=yes".to_string(),
        "-o".to_string(),
        "StrictHostKeyChecking=accept-new".to_string(),
        "-o".to_string(),
        "ConnectTimeout=8".to_string(),
        "-o".to_string(),
        "ServerAliveInterval=15".to_string(),
        "-o".to_string(),
        "ServerAliveCountMax=2".to_string(),
    ]
}

fn base_scp_args() -> Vec<String> {
    vec![
        "-q".to_string(),
        "-o".to_string(),
        "BatchMode=yes".to_string(),
        "-o".to_string(),
        "StrictHostKeyChecking=accept-new".to_string(),
        "-o".to_string(),
        "ConnectTimeout=8".to_string(),
    ]
}

fn ssh_target(server: &Server) -> String {
    server.alias.clone()
}

fn ssh_program() -> &'static str {
    if cfg!(windows) {
        "ssh.exe"
    } else {
        "ssh"
    }
}

fn scp_program() -> &'static str {
    if cfg!(windows) {
        "scp.exe"
    } else {
        "scp"
    }
}

fn split_directive(line: &str) -> Option<(&str, &str)> {
    let mut iter = line.splitn(2, char::is_whitespace);
    let key = iter.next()?.trim();
    let value = iter.next()?.trim();
    Some((key, value))
}

fn strip_comment(line: &str) -> &str {
    match line.find('#') {
        Some(idx) => &line[..idx],
        None => line,
    }
}

fn first_concrete_alias(value: &str) -> Option<String> {
    value
        .split_whitespace()
        .map(unquote)
        .find(|alias| !alias.contains('*') && !alias.contains('?') && !alias.starts_with('!'))
}

fn unquote(value: &str) -> String {
    value
        .trim()
        .trim_matches('"')
        .trim_matches('\'')
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::parse_ssh_config;

    #[test]
    fn parses_concrete_hosts_and_skips_wildcards() {
        let hosts = parse_ssh_config(
            r#"
            Host *
              User ignored

            Host codex-box
              HostName 10.40.2.39
              User root
              Port 22
              IdentityFile ~/.ssh/codex_box_ed25519

            Host *.internal
              User nope
            "#,
        );

        assert_eq!(hosts.len(), 1);
        assert_eq!(hosts[0].alias, "codex-box");
        assert_eq!(hosts[0].host_name, "10.40.2.39");
        assert_eq!(hosts[0].user.as_deref(), Some("root"));
        assert_eq!(hosts[0].port, Some(22));
        assert_eq!(hosts[0].identity_file_hint.as_deref(), Some(".../codex_box_ed25519"));
    }
}
