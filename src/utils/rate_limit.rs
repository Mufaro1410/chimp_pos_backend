//! Small in-memory fixed-window rate limiter for the public authentication endpoints.
//! It is per process: behind several instances each enforces its own budget, and the account
//! lockout in the database remains the shared protection against password guessing.
use std::{
    collections::HashMap,
    net::{IpAddr, SocketAddr},
    sync::{Mutex, OnceLock},
    time::{Duration, Instant},
};

use axum::{
    extract::{ConnectInfo, Request},
    http::{HeaderValue, StatusCode, header::RETRY_AFTER},
    middleware::Next,
    response::{IntoResponse, Response},
};

use crate::errors::api_error::ApiError;

const WINDOW: Duration = Duration::from_secs(60);

fn limit_per_window() -> u32 {
    static LIMIT: OnceLock<u32> = OnceLock::new();
    *LIMIT.get_or_init(|| {
        std::env::var("AUTH_RATE_LIMIT_PER_MINUTE")
            .ok()
            .and_then(|v| v.parse().ok())
            .filter(|v| *v > 0)
            // Strict in production; generous in development so test runs are not throttled.
            .unwrap_or(if crate::config::is_production() {
                30
            } else {
                600
            })
    })
}

fn trust_proxy() -> bool {
    static TRUST: OnceLock<bool> = OnceLock::new();
    *TRUST.get_or_init(|| {
        std::env::var("TRUST_PROXY")
            .map(|v| v.eq_ignore_ascii_case("true") || v == "1")
            .unwrap_or(false)
    })
}

static BUCKETS: OnceLock<Mutex<HashMap<IpAddr, (u32, Instant)>>> = OnceLock::new();

/// Returns Some(seconds_to_wait) when the caller is over budget.
fn check(ip: IpAddr, now: Instant) -> Option<u64> {
    let mut map = BUCKETS
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    if map.len() > 20_000 {
        map.retain(|_, (_, start)| now.duration_since(*start) < WINDOW);
    }
    let entry = map.entry(ip).or_insert((0, now));
    if now.duration_since(entry.1) >= WINDOW {
        *entry = (0, now);
    }
    entry.0 += 1;
    if entry.0 > limit_per_window() {
        Some(
            WINDOW
                .saturating_sub(now.duration_since(entry.1))
                .as_secs()
                .max(1),
        )
    } else {
        None
    }
}

fn client_ip(request: &Request) -> Option<IpAddr> {
    if trust_proxy() {
        // Only honour forwarding headers when the operator says a trusted proxy sets them.
        let forwarded = request
            .headers()
            .get("x-forwarded-for")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.split(',').next())
            .and_then(|v| v.trim().parse::<IpAddr>().ok());
        if forwarded.is_some() {
            return forwarded;
        }
    }
    request
        .extensions()
        .get::<ConnectInfo<SocketAddr>>()
        .map(|c| c.0.ip())
}

pub async fn auth_rate_limit(request: Request, next: Next) -> Response {
    if let Some(ip) = client_ip(&request) {
        if let Some(wait) = check(ip, Instant::now()) {
            let mut response = ApiError::new(
                StatusCode::TOO_MANY_REQUESTS,
                "RATE_LIMITED",
                "Too many requests. Please wait and try again",
            )
            .into_response();
            if let Ok(v) = HeaderValue::from_str(&wait.to_string()) {
                response.headers_mut().insert(RETRY_AFTER, v);
            }
            return response;
        }
    }
    next.run(request).await
}
