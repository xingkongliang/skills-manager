use std::sync::OnceLock;

use regex::Regex;

fn home_replacement() -> &'static [(Regex, &'static str)] {
    static CELL: OnceLock<Vec<(Regex, &'static str)>> = OnceLock::new();
    CELL.get_or_init(|| {
        let mut rules: Vec<(Regex, &'static str)> = Vec::new();
        if let Some(home) = dirs::home_dir() {
            let home_str = home.to_string_lossy().into_owned();
            if !home_str.is_empty() {
                let escaped = regex::escape(&home_str);
                if let Ok(re) = Regex::new(&escaped) {
                    rules.push((re, "~"));
                }
                let escaped_back = regex::escape(&home_str.replace('/', "\\"));
                if escaped_back != escaped {
                    if let Ok(re) = Regex::new(&escaped_back) {
                        rules.push((re, "~"));
                    }
                }
            }
        }
        // Generic user-home patterns to catch logs from other machines.
        for pat in [
            r#"/Users/[^/\s'"]+"#,
            r#"/home/[^/\s'"]+"#,
            r#"[A-Za-z]:\\Users\\[^\\\s'"]+"#,
        ] {
            if let Ok(re) = Regex::new(pat) {
                rules.push((re, "~"));
            }
        }
        rules
    })
}

fn other_rules() -> &'static [(Regex, &'static str)] {
    static CELL: OnceLock<Vec<(Regex, &'static str)>> = OnceLock::new();
    CELL.get_or_init(|| {
        let mut rules: Vec<(Regex, &'static str)> = Vec::new();
        // git remote URL credentials: https://user:token@host/...
        if let Ok(re) = Regex::new(r"https?://[^/\s:@]+:[^/\s@]+@") {
            rules.push((re, "https://<redacted>@"));
        }
        // SSH-style git remote with embedded user@host — keep host
        if let Ok(re) = Regex::new(r"(?:git@|ssh://[^/\s@]+@)") {
            rules.push((re, "git@"));
        }
        // Generic bearer / token-like long hex/base64 strings (>=32)
        if let Ok(re) = Regex::new(r"\b(?:gh[ps]_|sk-|xox[abprs]-|ghu_|ghr_)[A-Za-z0-9_\-]{16,}") {
            rules.push((re, "<token>"));
        }
        // Email addresses
        if let Ok(re) = Regex::new(r"[A-Za-z0-9._%+\-]+@[A-Za-z0-9.\-]+\.[A-Za-z]{2,}") {
            rules.push((re, "<email>"));
        }
        rules
    })
}

pub fn sanitize(input: &str) -> String {
    let mut out = input.to_string();
    for (re, replacement) in home_replacement() {
        out = re.replace_all(&out, *replacement).into_owned();
    }
    for (re, replacement) in other_rules() {
        out = re.replace_all(&out, *replacement).into_owned();
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redacts_email_and_token() {
        let s = sanitize("user alice@example.com pushed with token ghp_ABCDEFGHIJKLMNOPQRST");
        assert!(s.contains("<email>"));
        assert!(s.contains("<token>"));
    }

    #[test]
    fn redacts_git_credentials() {
        let s = sanitize("clone https://alice:pat_xyz@github.com/foo/bar.git");
        assert!(s.contains("<redacted>"));
        assert!(s.contains("github.com/foo/bar.git"));
    }

    #[test]
    fn replaces_generic_users_path() {
        let s = sanitize("opened /Users/somebody/Documents/x.txt");
        assert!(s.starts_with("opened ~/Documents"));
    }
}
