use std::{
    env, fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    thread,
    time::Duration,
};

#[cfg(windows)]
use std::os::windows::process::CommandExt;

use wait_timeout::ChildExt;

use crate::{
    models::{CommandResult, ImportedHost, ManagedSshKeyInfo, Server, ServerBootstrapInput},
    redaction::{identity_hint, redact},
};

pub fn importable_hosts_from_default_config() -> Result<Vec<ImportedHost>, String> {
    let path =
        default_ssh_config_path().ok_or_else(|| "could not locate SSH config".to_string())?;
    parse_ssh_config_file(&path)
}

pub fn default_ssh_config_path() -> Option<PathBuf> {
    if let Some(user_profile) = env::var_os("USERPROFILE") {
        let path = PathBuf::from(user_profile).join(".ssh").join("config");
        if path.exists() {
            return Some(path);
        }
    }

    if let (Some(home_drive), Some(home_path)) = (env::var_os("HOMEDRIVE"), env::var_os("HOMEPATH"))
    {
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

pub fn run_ssh_script(
    server: &Server,
    script: &str,
    timeout: Duration,
) -> Result<CommandResult, String> {
    let mut args = base_ssh_args(server, true);
    args.push(ssh_target(server));
    args.push("bash".to_string());
    args.push("-s".to_string());
    run_process(ssh_program(), &args, Some(script), timeout)
}

pub fn scp_to_remote(
    server: &Server,
    local_path: &Path,
    remote_path: &str,
    timeout: Duration,
) -> Result<CommandResult, String> {
    let mut args = base_scp_args(server);
    args.push(local_path.to_string_lossy().to_string());
    args.push(format!("{}:{remote_path}", scp_target(server)));
    run_process(scp_program(), &args, None, timeout)
}

pub fn scp_from_remote(
    server: &Server,
    remote_path: &str,
    local_path: &Path,
    timeout: Duration,
) -> Result<CommandResult, String> {
    let mut args = base_scp_args(server);
    args.push(format!("{}:{remote_path}", scp_target(server)));
    args.push(local_path.to_string_lossy().to_string());
    run_process(scp_program(), &args, None, timeout)
}

pub fn spawn_tunnel(server: &Server, local_port: u16) -> Result<std::process::Child, String> {
    let mut args = base_ssh_args(server, true);
    args.push("-o".to_string());
    args.push("ExitOnForwardFailure=yes".to_string());
    args.push("-N".to_string());
    args.push("-L".to_string());
    args.push(format!("127.0.0.1:{local_port}:127.0.0.1:9090"));
    args.push(ssh_target(server));

    let mut command = hidden_command(ssh_program());
    command
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|err| err.to_string())
}

pub fn ensure_managed_key(app_dir: &Path) -> Result<ManagedSshKeyInfo, String> {
    let key_dir = app_dir.join("ssh");
    fs::create_dir_all(&key_dir).map_err(|err| err.to_string())?;
    let private_key = key_dir.join("mihomo_manager_ed25519");
    let public_key = key_dir.join("mihomo_manager_ed25519.pub");

    if !private_key.exists() {
        if public_key.exists() {
            let _ = fs::remove_file(&public_key);
        }
        let args = vec![
            "-t".to_string(),
            "ed25519".to_string(),
            "-N".to_string(),
            String::new(),
            "-C".to_string(),
            "mihomo-server-manager".to_string(),
            "-f".to_string(),
            private_key.to_string_lossy().to_string(),
        ];
        let result = run_process(ssh_keygen_program(), &args, None, Duration::from_secs(20))?;
        if !result.ok {
            return Err(if result.stderr.trim().is_empty() {
                "ssh-keygen failed".to_string()
            } else {
                result.stderr
            });
        }
    }
    set_private_key_permissions(&private_key)?;

    if !public_key.exists() {
        let args = vec![
            "-y".to_string(),
            "-f".to_string(),
            private_key.to_string_lossy().to_string(),
        ];
        let result = run_process(ssh_keygen_program(), &args, None, Duration::from_secs(12))?;
        if !result.ok {
            return Err(if result.stderr.trim().is_empty() {
                "ssh-keygen failed to derive public key".to_string()
            } else {
                result.stderr
            });
        }
        fs::write(&public_key, result.stdout.trim()).map_err(|err| err.to_string())?;
    }

    let public_key_value = fs::read_to_string(&public_key).map_err(|err| err.to_string())?;
    let public_key_value = public_key_value.trim().to_string();
    if public_key_value.is_empty() {
        return Err("managed SSH public key is empty".to_string());
    }

    Ok(ManagedSshKeyInfo {
        public_key: public_key_value,
        public_key_hint: identity_hint(&public_key.to_string_lossy()),
        private_key_hint: identity_hint(&private_key.to_string_lossy()),
    })
}

pub fn managed_private_key_path(app_dir: &Path) -> PathBuf {
    app_dir.join("ssh").join("mihomo_manager_ed25519")
}

pub fn bootstrap_authorized_key(
    input: &ServerBootstrapInput,
    public_key: &str,
    timeout: Duration,
) -> Result<CommandResult, String> {
    let script = format!(
        r#"
set -e
umask 077
mkdir -p "$HOME/.ssh"
touch "$HOME/.ssh/authorized_keys"
pub={public_key}
if ! grep -qxF "$pub" "$HOME/.ssh/authorized_keys"; then
  printf '%s\n' "$pub" >> "$HOME/.ssh/authorized_keys"
fi
chmod 700 "$HOME/.ssh"
chmod 600 "$HOME/.ssh/authorized_keys"
echo "managed SSH key installed"
"#,
        public_key = shell_quote(public_key)
    );
    let args = password_ssh_args(input);
    run_process_with_askpass(
        ssh_program(),
        &args,
        Some(&script),
        timeout,
        &input.password,
    )
}

fn hidden_command(program: &str) -> Command {
    let mut command = Command::new(program);
    hide_child_window(&mut command);
    command
}

#[cfg(windows)]
fn hide_child_window(command: &mut Command) {
    const CREATE_NO_WINDOW: u32 = 0x08000000;
    command.creation_flags(CREATE_NO_WINDOW);
}

#[cfg(not(windows))]
fn hide_child_window(_command: &mut Command) {}

fn base_ssh_args(server: &Server, batch_mode: bool) -> Vec<String> {
    let mut args = vec![
        "-o".to_string(),
        format!("BatchMode={}", if batch_mode { "yes" } else { "no" }),
        "-o".to_string(),
        "StrictHostKeyChecking=accept-new".to_string(),
        "-o".to_string(),
        "ConnectTimeout=8".to_string(),
        "-o".to_string(),
        "ServerAliveInterval=15".to_string(),
        "-o".to_string(),
        "ServerAliveCountMax=2".to_string(),
    ];
    add_direct_connection_args(server, &mut args, false);
    args
}

fn base_scp_args(server: &Server) -> Vec<String> {
    let mut args = vec![
        "-q".to_string(),
        "-o".to_string(),
        "BatchMode=yes".to_string(),
        "-o".to_string(),
        "StrictHostKeyChecking=accept-new".to_string(),
        "-o".to_string(),
        "ConnectTimeout=8".to_string(),
    ];
    add_direct_connection_args(server, &mut args, true);
    args
}

fn password_ssh_args(input: &ServerBootstrapInput) -> Vec<String> {
    vec![
        "-o".to_string(),
        "BatchMode=no".to_string(),
        "-o".to_string(),
        "PreferredAuthentications=password,keyboard-interactive".to_string(),
        "-o".to_string(),
        "NumberOfPasswordPrompts=1".to_string(),
        "-o".to_string(),
        "StrictHostKeyChecking=accept-new".to_string(),
        "-o".to_string(),
        "ConnectTimeout=10".to_string(),
        "-p".to_string(),
        input.port.unwrap_or(22).to_string(),
        format!("{}@{}", input.user.trim(), input.host_name.trim()),
        "bash".to_string(),
        "-s".to_string(),
    ]
}

fn add_direct_connection_args(server: &Server, args: &mut Vec<String>, scp: bool) {
    if uses_ssh_config_alias(server) {
        return;
    }
    if let Some(port) = server.port {
        args.push(if scp { "-P" } else { "-p" }.to_string());
        args.push(port.to_string());
    }
    if let Some(path) = server
        .identity_file_path
        .as_deref()
        .filter(|value| !value.is_empty())
    {
        args.push("-i".to_string());
        args.push(path.to_string());
        args.push("-o".to_string());
        args.push("IdentitiesOnly=yes".to_string());
    }
}

fn ssh_target(server: &Server) -> String {
    if uses_ssh_config_alias(server) {
        server.alias.clone()
    } else if let Some(user) = server.user.as_deref().filter(|value| !value.is_empty()) {
        format!("{user}@{}", server.host_name)
    } else {
        server.host_name.clone()
    }
}

fn scp_target(server: &Server) -> String {
    let target = ssh_target(server);
    if uses_ssh_config_alias(server) || !server.host_name.contains(':') {
        return target;
    }
    match server.user.as_deref().filter(|value| !value.is_empty()) {
        Some(user) => format!("{user}@[{}]", server.host_name),
        None => format!("[{}]", server.host_name),
    }
}

fn uses_ssh_config_alias(server: &Server) -> bool {
    server.source == "ssh_config" && server.identity_file_path.is_none()
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

fn ssh_keygen_program() -> &'static str {
    if cfg!(windows) {
        "ssh-keygen.exe"
    } else {
        "ssh-keygen"
    }
}

fn run_process_with_askpass(
    program: &str,
    args: &[String],
    input: Option<&str>,
    timeout: Duration,
    password: &str,
) -> Result<CommandResult, String> {
    let temp = tempfile::tempdir().map_err(|err| err.to_string())?;
    let password_file = create_password_file(temp.path(), password)?;
    let askpass = create_askpass_helper(temp.path())?;
    let envs = vec![
        (
            "SSH_ASKPASS".to_string(),
            askpass.to_string_lossy().to_string(),
        ),
        ("SSH_ASKPASS_REQUIRE".to_string(), "force".to_string()),
        ("DISPLAY".to_string(), "mihomo-manager".to_string()),
        (
            "MIHOMO_SSH_PASSWORD_FILE".to_string(),
            password_file.to_string_lossy().to_string(),
        ),
    ];
    run_process_with_env(program, args, input, timeout, &envs)
}

fn run_process_with_env(
    program: &str,
    args: &[String],
    input: Option<&str>,
    timeout: Duration,
    envs: &[(String, String)],
) -> Result<CommandResult, String> {
    let mut command = hidden_command(program);
    for (key, value) in envs {
        command.env(key, value);
    }
    run_process_from_command(program, command, args, input, timeout)
}

fn run_process(
    program: &str,
    args: &[String],
    input: Option<&str>,
    timeout: Duration,
) -> Result<CommandResult, String> {
    let command = hidden_command(program);
    run_process_from_command(program, command, args, input, timeout)
}

fn run_process_from_command(
    program: &str,
    mut command: Command,
    args: &[String],
    input: Option<&str>,
    timeout: Duration,
) -> Result<CommandResult, String> {
    let mut child = command
        .args(args)
        .stdin(if input.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|err| format!("{program} failed to start: {err}"))?;

    let stdout = read_pipe_in_thread(child.stdout.take());
    let stderr = read_pipe_in_thread(child.stderr.take());

    if let Some(input) = input {
        if let Some(mut stdin) = child.stdin.take() {
            stdin
                .write_all(input.as_bytes())
                .map_err(|err| err.to_string())?;
        }
    }

    let status = match child.wait_timeout(timeout).map_err(|err| err.to_string())? {
        Some(status) => status,
        None => {
            let _ = child.kill();
            let _ = child.wait();
            let stdout = join_pipe_reader(stdout)?;
            let _ = join_pipe_reader(stderr);
            return Ok(CommandResult {
                ok: false,
                code: None,
                stdout: redact(&stdout),
                stderr: format!("timed out after {}s", timeout.as_secs()),
            });
        }
    };

    let stdout = join_pipe_reader(stdout)?;
    let stderr = join_pipe_reader(stderr)?;
    Ok(CommandResult {
        ok: status.success(),
        code: status.code(),
        stdout: redact(&stdout),
        stderr: redact(&stderr),
    })
}

fn read_pipe_in_thread<T>(pipe: Option<T>) -> thread::JoinHandle<Result<String, String>>
where
    T: Read + Send + 'static,
{
    thread::spawn(move || {
        let mut buffer = String::new();
        if let Some(mut pipe) = pipe {
            pipe.read_to_string(&mut buffer)
                .map_err(|err| err.to_string())?;
        }
        Ok(buffer)
    })
}

fn join_pipe_reader(handle: thread::JoinHandle<Result<String, String>>) -> Result<String, String> {
    handle
        .join()
        .map_err(|_| "process pipe reader panicked".to_string())?
}

fn create_askpass_helper(dir: &Path) -> Result<PathBuf, String> {
    let helper = if cfg!(windows) {
        dir.join("mihomo-askpass.cmd")
    } else {
        dir.join("mihomo-askpass.sh")
    };
    let body = if cfg!(windows) {
        "@echo off\r\npowershell -NoProfile -ExecutionPolicy Bypass -Command \"[Console]::Out.Write([IO.File]::ReadAllText($env:MIHOMO_SSH_PASSWORD_FILE))\"\r\n"
    } else {
        "#!/bin/sh\ncat \"$MIHOMO_SSH_PASSWORD_FILE\"\n"
    };
    fs::write(&helper, body).map_err(|err| err.to_string())?;
    set_executable_permissions(&helper)?;
    Ok(helper)
}

fn create_password_file(dir: &Path, password: &str) -> Result<PathBuf, String> {
    let path = dir.join("mihomo-ssh-password");
    fs::write(&path, password).map_err(|err| err.to_string())?;
    set_private_key_permissions(&path)?;
    Ok(path)
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

#[cfg(unix)]
fn set_executable_permissions(path: &Path) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o700)).map_err(|err| err.to_string())
}

#[cfg(not(unix))]
fn set_executable_permissions(_path: &Path) -> Result<(), String> {
    Ok(())
}

#[cfg(unix)]
fn set_private_key_permissions(path: &Path) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600)).map_err(|err| err.to_string())
}

#[cfg(not(unix))]
fn set_private_key_permissions(_path: &Path) -> Result<(), String> {
    Ok(())
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
    use super::{base_ssh_args, parse_ssh_config, ssh_target};
    use crate::models::Server;

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
        assert_eq!(
            hosts[0].identity_file_hint.as_deref(),
            Some(".../codex_box_ed25519")
        );
    }

    #[test]
    fn ssh_config_server_uses_alias_target() {
        let server = Server {
            id: 1,
            alias: "codex-box".to_string(),
            display_name: "codex-box".to_string(),
            host_name: "10.40.2.39".to_string(),
            user: Some("root".to_string()),
            port: Some(22),
            identity_file_hint: Some(".../id_ed25519".to_string()),
            identity_file_path: None,
            source: "ssh_config".to_string(),
            last_status: None,
            last_seen_at: None,
        };

        assert_eq!(ssh_target(&server), "codex-box");
        assert!(!base_ssh_args(&server, true).contains(&"-i".to_string()));
    }

    #[test]
    fn manual_server_uses_direct_target_and_identity_file() {
        let server = Server {
            id: 1,
            alias: "manual:root@10.40.2.39:22".to_string(),
            display_name: "codex-box".to_string(),
            host_name: "10.40.2.39".to_string(),
            user: Some("root".to_string()),
            port: Some(22),
            identity_file_hint: Some(".../mihomo_manager_ed25519".to_string()),
            identity_file_path: Some("D:/App/ssh/mihomo_manager_ed25519".to_string()),
            source: "manual".to_string(),
            last_status: None,
            last_seen_at: None,
        };
        let args = base_ssh_args(&server, true);

        assert_eq!(ssh_target(&server), "root@10.40.2.39");
        assert!(args
            .windows(2)
            .any(|pair| pair[0] == "-p" && pair[1] == "22"));
        assert!(args
            .windows(2)
            .any(|pair| pair[0] == "-i" && pair[1] == "D:/App/ssh/mihomo_manager_ed25519"));
    }
}
