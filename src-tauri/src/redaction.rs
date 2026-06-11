pub fn redact(input: &str) -> String {
    let mut output = String::with_capacity(input.len());
    let mut token = String::new();

    for ch in input.chars() {
        if ch.is_whitespace() {
            if !token.is_empty() {
                output.push_str(&redact_token(&token));
                token.clear();
            }
            output.push(ch);
        } else {
            token.push(ch);
        }
    }

    if !token.is_empty() {
        output.push_str(&redact_token(&token));
    }

    let mut compacted = output;

    for key in ["subscription", "token", "secret", "password", "passwd"] {
        compacted = redact_key_values(&compacted, key);
    }

    compacted
}

fn redact_token(token: &str) -> String {
    let lower = token.to_ascii_lowercase();
    if lower.starts_with("http://")
        || lower.starts_with("https://")
        || lower.starts_with("ss://")
        || lower.starts_with("ssr://")
        || lower.starts_with("trojan://")
        || lower.starts_with("vmess://")
        || lower.starts_with("vless://")
        || lower.starts_with("hysteria://")
        || lower.starts_with("hysteria2://")
    {
        return "[redacted-url]".to_string();
    }

    if lower.contains(".ssh") || lower.ends_with(".pem") || lower.contains("identityfile") {
        return "[redacted-path]".to_string();
    }

    token.to_string()
}

fn redact_key_values(input: &str, key: &str) -> String {
    let mut result = String::with_capacity(input.len());
    for part in input.split_inclusive(char::is_whitespace) {
        let trailing = part
            .chars()
            .rev()
            .take_while(|ch| ch.is_whitespace())
            .collect::<String>();
        let value = part.trim_end();
        let lower = part.to_ascii_lowercase();
        if lower.starts_with(&format!("{key}=")) || lower.starts_with(&format!("{key}:")) {
            result.push_str(key);
            result.push_str("=[redacted]");
            result.push_str(&trailing.chars().rev().collect::<String>());
        } else {
            result.push_str(value);
            result.push_str(&trailing.chars().rev().collect::<String>());
        }
    }
    result
}

pub fn identity_hint(path: &str) -> String {
    let normalized = path.replace('\\', "/");
    let mut segments = normalized.rsplit('/');
    match segments.next() {
        Some(file) if !file.is_empty() => format!(".../{file}"),
        _ => "[redacted-path]".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::{identity_hint, redact};

    #[test]
    fn redacts_subscription_urls_and_identity_paths() {
        let text =
            "curl https://example.com/sub?token=abc IdentityFile C:/Users/me/.ssh/id_ed25519";
        let redacted = redact(text);
        assert!(redacted.contains("[redacted-url]"));
        assert!(redacted.contains("[redacted-path]"));
        assert!(!redacted.contains("token=abc"));
    }

    #[test]
    fn keeps_only_identity_file_name_as_hint() {
        assert_eq!(
            identity_hint(r"C:\Users\me\.ssh\codex_box_ed25519"),
            ".../codex_box_ed25519"
        );
    }
}
