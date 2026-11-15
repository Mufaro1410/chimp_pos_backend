//! Runtime configuration helpers and production safety checks.

pub fn is_production() -> bool {
    std::env::var("APP_ENV")
        .map(|v| v.eq_ignore_ascii_case("production"))
        .unwrap_or(false)
}

fn env_true(key: &str) -> bool {
    std::env::var(key)
        .map(|v| v.eq_ignore_ascii_case("true") || v == "1")
        .unwrap_or(false)
}

/// Returns human-readable problems with the current configuration.
/// In production (`APP_ENV=production`) any problem aborts startup; otherwise they are warnings.
pub fn configuration_problems() -> Vec<String> {
    let mut problems = Vec::new();

    let secret = std::env::var("JWT_SECRET").unwrap_or_default();
    if secret.len() < 32 {
        problems.push("JWT_SECRET must contain at least 32 characters".into());
    }
    let lowered = secret.to_lowercase();
    if [
        "dev-secret",
        "change-me",
        "changeme",
        "secret",
        "password",
        "example",
    ]
    .iter()
    .any(|weak| lowered.contains(weak))
    {
        problems.push(
            "JWT_SECRET looks like a placeholder; generate one with `openssl rand -base64 48`"
                .into(),
        );
    }

    if is_production() {
        match std::env::var("CORS_ALLOWED_ORIGINS") {
            Ok(v) if !v.trim().is_empty() && !v.contains('*') => {}
            _ => problems.push(
                "CORS_ALLOWED_ORIGINS must list the allowed origins explicitly (no wildcard) in production".into(),
            ),
        }
        if std::env::var("PASSWORD_RESET_DELIVERY").unwrap_or_default() == "log" {
            problems.push("PASSWORD_RESET_DELIVERY=log writes reset tokens to the log; do not use it in production".into());
        }
        if env_true("TRUST_PROXY") && std::env::var("HOST").unwrap_or_default() == "0.0.0.0" {
            // Not an error, but a reminder: only enable TRUST_PROXY when the proxy is the only way in.
            tracing::warn!(
                "TRUST_PROXY is on and HOST=0.0.0.0: make sure the port is firewalled so only the proxy can reach it"
            );
        }
    }
    problems
}
