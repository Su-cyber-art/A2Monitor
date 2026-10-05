use crate::{
    error::{ApiError, ApiResult},
    runtime::Shared,
};
use argon2::{
    password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
    Argon2,
};
use axum::{
    extract::{ConnectInfo, Request, State},
    http::{header, HeaderMap, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
    Json,
};
use serde::Deserialize;
use serde_json::json;
use sha2::{Digest, Sha256};

pub fn token() -> String {
    hex::encode(rand::random::<[u8; 32]>())
}
pub fn hash(value: &str) -> String {
    hex::encode(Sha256::digest(value.as_bytes()))
}
pub fn password_hash(password: &str) -> anyhow::Result<String> {
    let salt =
        SaltString::encode_b64(&rand::random::<[u8; 16]>()).map_err(|e| anyhow::anyhow!("{e}"))?;
    Ok(Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map_err(|e| anyhow::anyhow!("{e}"))?
        .to_string())
}
pub fn session(headers: &HeaderMap) -> Option<String> {
    headers
        .get(header::COOKIE)?
        .to_str()
        .ok()?
        .split(';')
        .filter_map(|c| c.trim().split_once('='))
        .find(|(k, _)| *k == "monitor_session")
        .map(|(_, v)| v.to_owned())
}
pub async fn authed(app: &Shared, headers: &HeaderMap) -> bool {
    if let Some(token) = session(headers) {
        app.db.valid_session(hash(&token)).await.unwrap_or(false)
    } else {
        false
    }
}
pub fn same_origin(headers: &HeaderMap) -> bool {
    let Some(origin) = headers.get(header::ORIGIN).and_then(|v| v.to_str().ok()) else {
        return true;
    };
    let Some(host) = headers.get(header::HOST).and_then(|v| v.to_str().ok()) else {
        return false;
    };
    let Ok(url) = reqwest::Url::parse(origin) else {
        return false;
    };
    let authority = match url.port() {
        Some(port) => format!("{}:{port}", url.host_str().unwrap_or("")),
        None => url.host_str().unwrap_or("").to_owned(),
    };
    authority == host
}
pub async fn admin(State(app): State<Shared>, request: Request, next: Next) -> Response {
    if !authed(&app, request.headers()).await {
        return ApiError::unauthorized().into_response();
    }
    if !matches!(
        *request.method(),
        axum::http::Method::GET | axum::http::Method::HEAD
    ) && !same_origin(request.headers())
    {
        return ApiError(StatusCode::FORBIDDEN, "请求来源无效".into()).into_response();
    }
    next.run(request).await
}
#[derive(Deserialize)]
pub struct Login {
    pub password: String,
}
pub async fn login(
    State(app): State<Shared>,
    ConnectInfo(peer): ConnectInfo<std::net::SocketAddr>,
    headers: HeaderMap,
    Json(input): Json<Login>,
) -> ApiResult<Response> {
    if !same_origin(&headers) {
        return Err(ApiError(StatusCode::FORBIDDEN, "请求来源无效".into()));
    }
    if input.password.len() > 1024 {
        return Err(ApiError::bad("密码过长"));
    }
    let address = if peer.ip().is_loopback() {
        headers
            .get("x-forwarded-for")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.split(',').next_back())
            .and_then(|v| v.trim().parse::<std::net::IpAddr>().ok())
            .unwrap_or(peer.ip())
            .to_string()
    } else {
        peer.ip().to_string()
    };
    let now = monitor_protocol::now_ms();
    {
        let mut attempts = app.login_attempts.lock().await;
        attempts.retain(|_, (_, time)| now - *time < 900000);
        if attempts.get(&address).is_some_and(|(count, _)| *count >= 5) {
            return Err(ApiError(
                StatusCode::TOO_MANY_REQUESTS,
                "尝试过多，请在 15 分钟后重试".into(),
            ));
        }
    }
    let _permit = app.login_gate.try_acquire().map_err(|_| {
        ApiError(
            StatusCode::TOO_MANY_REQUESTS,
            "登录正在校验，请稍后重试".into(),
        )
    })?;
    let stored = app.db.get("password_hash").await?.unwrap_or_default();
    let valid = tokio::task::spawn_blocking(move || {
        PasswordHash::new(&stored).is_ok_and(|hash| {
            Argon2::default()
                .verify_password(input.password.as_bytes(), &hash)
                .is_ok()
        })
    })
    .await
    .map_err(|e| ApiError::from(anyhow::anyhow!(e)))?;
    if !valid {
        let mut attempts = app.login_attempts.lock().await;
        let entry = attempts.entry(address).or_insert((0, now));
        entry.0 += 1;
        return Err(ApiError(StatusCode::UNAUTHORIZED, "密码不正确".into()));
    }
    app.login_attempts.lock().await.remove(&address);
    let secret = token();
    app.db.add_session(hash(&secret)).await?;
    let secure = headers
        .get("x-forwarded-proto")
        .is_some_and(|v| v == "https");
    let cookie = format!(
        "monitor_session={secret}; Path=/; HttpOnly; SameSite=Lax; Max-Age=1209600{}",
        if secure { "; Secure" } else { "" }
    );
    Ok(([(header::SET_COOKIE, cookie)], Json(json!({"ok":true}))).into_response())
}
pub async fn logout(State(app): State<Shared>, headers: HeaderMap) -> ApiResult<Response> {
    if !same_origin(&headers) {
        return Err(ApiError(StatusCode::FORBIDDEN, "请求来源无效".into()));
    }
    if let Some(token) = session(&headers) {
        app.db.remove_session(Some(hash(&token))).await?;
    }
    Ok((
        [(
            header::SET_COOKIE,
            "monitor_session=; Path=/; HttpOnly; SameSite=Lax; Max-Age=0",
        )],
        Json(json!({"ok":true})),
    )
        .into_response())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cross_origin_mutations_are_rejected() {
        let mut h = HeaderMap::new();
        h.insert(header::HOST, "example.com".parse().unwrap());
        h.insert(header::ORIGIN, "https://evil.com".parse().unwrap());
        assert!(!same_origin(&h));
        h.insert(header::ORIGIN, "https://example.com".parse().unwrap());
        assert!(same_origin(&h));
    }
}
