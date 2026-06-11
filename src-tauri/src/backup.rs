use std::time::Duration;

use chrono::Utc;

use crate::{
    models::{BackupFile, BackupSnapshot, CommandResult, Server},
    ssh,
};

const BACKUP_ROOT: &str = "/etc/mihomo/manager-backups";
const CONFIG_PATH: &str = "/etc/mihomo/config.yaml";
const SUBSCRIPTION_PATH: &str = "/etc/mihomo/subscription.url";
const REMOTE_PROXY_PATH: &str = "/etc/profile.d/mihomo-manager-proxy.sh";

struct BackupTarget {
    kind: &'static str,
    remote_path: &'static str,
    backup_file: &'static str,
}

const TARGETS: &[BackupTarget] = &[
    BackupTarget {
        kind: "config",
        remote_path: CONFIG_PATH,
        backup_file: "config.yaml",
    },
    BackupTarget {
        kind: "subscription",
        remote_path: SUBSCRIPTION_PATH,
        backup_file: "subscription.url",
    },
    BackupTarget {
        kind: "remote_proxy",
        remote_path: REMOTE_PROXY_PATH,
        backup_file: "mihomo-manager-proxy.sh",
    },
];

pub struct RemoteBackup {
    pub remote_dir: String,
    pub files: Vec<BackupFile>,
}

pub fn create(server: &Server, reason: &str) -> Result<RemoteBackup, String> {
    let token = sanitize_path_token(reason);
    let stamp = Utc::now().format("%Y%m%d%H%M%S%3f");
    let remote_dir = format!("{BACKUP_ROOT}/{stamp}-{token}");
    let script = create_script(&remote_dir);
    let output = ssh::run_ssh_script(server, &script, Duration::from_secs(30))?;
    if !output.ok {
        return Err(output.stderr);
    }
    parse_create_output(&output.stdout)
}

pub fn restore(server: &Server, snapshot: &BackupSnapshot) -> Result<CommandResult, String> {
    validate_backup_dir(&snapshot.remote_dir)?;
    let script = restore_script(snapshot)?;
    ssh::run_ssh_script(server, &script, Duration::from_secs(45))
}

pub fn delete_remote(server: &Server, remote_dir: &str) -> Result<CommandResult, String> {
    validate_backup_dir(remote_dir)?;
    let script = format!(
        r#"
set -euo pipefail
dir={dir}
case "$dir" in
  {root}/*) ;;
  *) echo "invalid backup directory" >&2; exit 2 ;;
esac
rm -rf -- "$dir"
printf 'deleted backup %s\n' "$dir"
"#,
        dir = shell_quote(remote_dir),
        root = BACKUP_ROOT
    );
    ssh::run_ssh_script(server, &script, Duration::from_secs(20))
}

fn create_script(remote_dir: &str) -> String {
    let mut calls = String::new();
    for target in TARGETS {
        calls.push_str(&format!(
            "backup_file {} {} {}\n",
            shell_quote(target.kind),
            shell_quote(target.remote_path),
            shell_quote(target.backup_file)
        ));
    }

    format!(
        r#"
set -euo pipefail
backup_dir={backup_dir}
install -d -m 0700 "$backup_dir"
manifest="$backup_dir/manifest.tsv"
printf 'kind\tremote_path\tpresent\tsize_bytes\tsha256\tbackup_file\n' > "$manifest"
backup_file() {{
  kind="$1"
  remote_path="$2"
  backup_file="$3"
  if [ -f "$remote_path" ]; then
    size="$(wc -c < "$remote_path" | tr -d '[:space:]')"
    sha="$(sha256sum "$remote_path" | awk '{{print $1}}')"
    cp -a "$remote_path" "$backup_dir/$backup_file"
    printf '%s\t%s\ttrue\t%s\t%s\t%s\n' "$kind" "$remote_path" "$size" "$sha" "$backup_file" >> "$manifest"
    printf 'FILE\t%s\t%s\ttrue\t%s\t%s\t%s\n' "$kind" "$remote_path" "$size" "$sha" "$backup_file"
  else
    printf '%s\t%s\tfalse\t\t\t%s\n' "$kind" "$remote_path" "$backup_file" >> "$manifest"
    printf 'FILE\t%s\t%s\tfalse\t\t\t%s\n' "$kind" "$remote_path" "$backup_file"
  fi
}}
{calls}printf 'BACKUP_DIR\t%s\n' "$backup_dir"
"#,
        backup_dir = shell_quote(remote_dir)
    )
}

fn restore_script(snapshot: &BackupSnapshot) -> Result<String, String> {
    let mut commands = String::new();
    let mut touches_config = false;
    for file in &snapshot.files {
        let Some(target) = target_for_kind(&file.kind) else {
            continue;
        };
        if target.kind == "config" {
            touches_config = true;
        }
        let backup_path = format!("{}/{}", snapshot.remote_dir, target.backup_file);
        if file.present {
            let mode = match target.kind {
                "subscription" => "0600",
                _ => "0644",
            };
            if target.kind == "config" {
                commands.push_str(&format!(
                    "/usr/local/bin/mihomo -t -d /etc/mihomo -f {}\n",
                    shell_quote(&backup_path)
                ));
            }
            commands.push_str(&format!(
                "install -D -m {mode} {} {}\n",
                shell_quote(&backup_path),
                shell_quote(target.remote_path)
            ));
        } else {
            commands.push_str(&format!("rm -f -- {}\n", shell_quote(target.remote_path)));
        }
    }

    let restart = if touches_config {
        r#"
if [ -f /etc/mihomo/config.yaml ]; then
  systemctl restart mihomo
else
  systemctl stop mihomo || true
fi
"#
    } else {
        ""
    };

    Ok(format!(
        r#"
set -euo pipefail
backup_dir={backup_dir}
test -d "$backup_dir"
{commands}{restart}printf 'restored backup %s\n' "$backup_dir"
"#,
        backup_dir = shell_quote(&snapshot.remote_dir)
    ))
}

fn parse_create_output(stdout: &str) -> Result<RemoteBackup, String> {
    let mut remote_dir: Option<String> = None;
    let mut files = Vec::new();
    for line in stdout.lines() {
        let parts = line.split('\t').collect::<Vec<_>>();
        match parts.as_slice() {
            ["BACKUP_DIR", value] => remote_dir = Some((*value).to_string()),
            ["FILE", kind, remote_path, present, size, sha256, backup_file] => {
                files.push(BackupFile {
                    kind: (*kind).to_string(),
                    remote_path: (*remote_path).to_string(),
                    backup_file: (*backup_file).to_string(),
                    present: *present == "true",
                    size_bytes: if size.is_empty() {
                        None
                    } else {
                        Some(size.parse::<u64>().map_err(|err| err.to_string())?)
                    },
                    sha256: if sha256.is_empty() {
                        None
                    } else {
                        Some((*sha256).to_string())
                    },
                });
            }
            _ => {}
        }
    }
    let remote_dir =
        remote_dir.ok_or_else(|| "backup output missing remote directory".to_string())?;
    validate_backup_dir(&remote_dir)?;
    if files.len() != TARGETS.len() {
        return Err("backup output missing files".to_string());
    }
    Ok(RemoteBackup { remote_dir, files })
}

fn target_for_kind(kind: &str) -> Option<&'static BackupTarget> {
    TARGETS.iter().find(|target| target.kind == kind)
}

fn validate_backup_dir(remote_dir: &str) -> Result<(), String> {
    let prefix = format!("{BACKUP_ROOT}/");
    if !remote_dir.starts_with(&prefix)
        || remote_dir.len() <= prefix.len()
        || remote_dir.contains("..")
        || remote_dir.chars().any(char::is_control)
        || remote_dir.chars().any(char::is_whitespace)
    {
        return Err("invalid backup directory".to_string());
    }
    Ok(())
}

fn sanitize_path_token(value: &str) -> String {
    let token = value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
                ch.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>()
        .trim_matches('-')
        .to_string();
    if token.is_empty() {
        "manual".to_string()
    } else {
        token
    }
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

#[cfg(test)]
mod tests {
    use super::{parse_create_output, validate_backup_dir};

    #[test]
    fn parses_create_backup_output() {
        let output = "FILE\tconfig\t/etc/mihomo/config.yaml\ttrue\t12\tabc\tconfig.yaml\n\
FILE\tsubscription\t/etc/mihomo/subscription.url\tfalse\t\t\tsubscription.url\n\
FILE\tremote_proxy\t/etc/profile.d/mihomo-manager-proxy.sh\ttrue\t5\tdef\tmihomo-manager-proxy.sh\n\
BACKUP_DIR\t/etc/mihomo/manager-backups/20260611120000000-manual\n";
        let parsed = parse_create_output(output).unwrap();
        assert_eq!(
            parsed.remote_dir,
            "/etc/mihomo/manager-backups/20260611120000000-manual"
        );
        assert_eq!(parsed.files.len(), 3);
        assert!(parsed.files[0].present);
        assert!(!parsed.files[1].present);
    }

    #[test]
    fn rejects_unsafe_backup_dirs() {
        assert!(validate_backup_dir("/etc/mihomo/manager-backups/good").is_ok());
        assert!(validate_backup_dir("/etc/mihomo/manager-backups/../bad").is_err());
        assert!(validate_backup_dir("/tmp/backup").is_err());
        assert!(validate_backup_dir("/etc/mihomo/manager-backups/has space").is_err());
    }
}
