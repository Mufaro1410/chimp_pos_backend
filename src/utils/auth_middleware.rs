use axum::{
    extract::State,
    http::{Request, StatusCode, header::AUTHORIZATION},
    middleware::Next,
    response::IntoResponse,
    response::Response,
};

use jsonwebtoken::{Algorithm, DecodingKey, Validation, decode};

use uuid::Uuid;

use crate::{models::auth::AuthUser, state::SharedState, utils::jwt::Claims};

/// Endpoints a user may still call while `must_change_password` is set.
const PASSWORD_CHANGE_ALLOWLIST: &[&str] = &[
    "/api/v1/auth/me",
    "/api/v1/auth/change-password",
    "/api/v1/auth/logout-all",
];

enum Reject {
    Status(StatusCode),
    PasswordChangeRequired,
}

impl From<StatusCode> for Reject {
    fn from(s: StatusCode) -> Self {
        Reject::Status(s)
    }
}

async fn authenticate(
    State(state): State<SharedState>,
    mut request: Request<axum::body::Body>,
    next: Next,
) -> Result<Response, Reject> {
    // 1. Obtain the Authorization header.
    let authorization = request
        .headers()
        .get(AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .ok_or(StatusCode::UNAUTHORIZED)?;

    // 2. Require the Bearer authentication scheme.
    let token = authorization
        .strip_prefix("Bearer ")
        .ok_or(StatusCode::UNAUTHORIZED)?;

    if token.is_empty() {
        return Err(StatusCode::UNAUTHORIZED.into());
    }

    // 3. Configure JWT validation.
    let mut validation = Validation::new(Algorithm::HS256);

    validation.set_issuer(&["chimp-pos"]);
    validation.set_required_spec_claims(&["exp", "iss", "sub"]);

    validation.validate_exp = true;
    validation.leeway = 0;

    // 4. Verify signature and claims.
    let token_data = decode::<Claims>(
        token,
        &DecodingKey::from_secret(state.jwt_secret.as_bytes()),
        &validation,
    )
    .map_err(|_| StatusCode::UNAUTHORIZED)?;

    let claims = token_data.claims;

    // 5. Convert token identities into UUIDs.
    let user_id = Uuid::parse_str(&claims.sub).map_err(|_| StatusCode::UNAUTHORIZED)?;

    let company_id = Uuid::parse_str(&claims.company_id).map_err(|_| StatusCode::UNAUTHORIZED)?;

    let token_id = Uuid::parse_str(&claims.jti).map_err(|_| StatusCode::UNAUTHORIZED)?;

    // 6. Confirm that the user and company are still active.
    let account_status = sqlx::query_as::<_, (String, String, bool, i32)>(
        r#"
        SELECT u.status, c.status, u.must_change_password, u.token_version
        FROM users u
        JOIN companies c ON c.id = u.company_id
        WHERE u.id = $1
          AND u.company_id = $2
        "#,
    )
    .bind(user_id)
    .bind(company_id)
    .fetch_optional(&state.db)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let (user_status, company_status, must_change, token_version) =
        account_status.ok_or(StatusCode::UNAUTHORIZED)?;

    if user_status != "active" || company_status != "active" {
        return Err(StatusCode::UNAUTHORIZED.into());
    }

    // A password change/reset/sign-out-everywhere bumps token_version, killing older tokens.
    if claims.tv != token_version {
        return Err(StatusCode::UNAUTHORIZED.into());
    }

    if must_change && !PASSWORD_CHANGE_ALLOWLIST.contains(&request.uri().path()) {
        return Err(Reject::PasswordChangeRequired);
    }

    // 7. Attach the authenticated identity to this request.
    request.extensions_mut().insert(AuthUser {
        user_id,
        company_id,
        token_id,
    });

    // 8. Continue to the requested handler.
    Ok(next.run(request).await)
}

pub async fn auth_middleware(
    state: State<SharedState>,
    request: Request<axum::body::Body>,
    next: Next,
) -> Response {
    match authenticate(state, request, next).await {
        Ok(response) => response,
        Err(Reject::Status(status)) => crate::errors::api_error::status_response(status),
        Err(Reject::PasswordChangeRequired) => crate::errors::api_error::ApiError::new(
            StatusCode::FORBIDDEN,
            "PASSWORD_CHANGE_REQUIRED",
            "You must change your password before continuing",
        )
        .into_response(),
    }
}
