//! Headless server binary: serves the same web UI and API over HTTP so
//! douyin-recorder can run on machines without a graphical environment.
//!
//! Environment:
//! - `DOUYIN_RECORDER_HOME`  config/database directory (same layout as desktop)
//! - `DOUYIN_RECORDER_BIND`  listen address, default `0.0.0.0:8787`
//! - `DOUYIN_RECORDER_TOKEN` optional shared token; when set, requests must
//!   send `X-Auth-Token: <token>` (or `?token=` for EventSource/SSE)
//!
//! Endpoints:
//! - `POST /api/invoke/{command}`  same commands as the desktop `invoke`
//! - `GET  /api/events`            Server-Sent Events (same event names)
//! - `GET  /api/health`
//! - `/*`                          embedded web UI

use axum::{
    body::Body,
    extract::{Path, State},
    http::{header, HeaderMap, StatusCode},
    middleware::{self, Next},
    response::{
        sse::{Event, KeepAlive, Sse},
        IntoResponse, Response,
    },
    routing::{get, post},
    Json, Router,
};
use douyin_recorder_core::bus::Host;
use douyin_recorder_core::service;
use futures_util::Stream;
use serde_json::{json, Value};
use tokio_stream::wrappers::BroadcastStream;
use tokio_stream::StreamExt;

include!(concat!(env!("OUT_DIR"), "/web_assets.rs"));

#[derive(Clone)]
struct AppConfig {
    token: Option<String>,
}

#[tokio::main]
async fn main() {
    let mut runtime = match douyin_recorder_core::Runtime::new() {
        Ok(runtime) => runtime,
        Err(error) => {
            eprintln!("初始化失败: {error}");
            std::process::exit(1);
        }
    };
    runtime.start_background();
    let host = runtime.host.clone();
    let token = std::env::var("DOUYIN_RECORDER_TOKEN")
        .ok()
        .filter(|value| !value.trim().is_empty());
    let bind = std::env::var("DOUYIN_RECORDER_BIND").unwrap_or_else(|_| "0.0.0.0:8787".to_string());
    if token.is_none() {
        eprintln!("警告: 未设置 DOUYIN_RECORDER_TOKEN，接口无认证。请仅在内网使用或设置令牌。");
    }

    let config = AppConfig { token };

    let app = Router::new()
        .route("/api/health", get(health))
        .route("/api/invoke/{command}", post(invoke))
        .route("/api/events", get(events))
        .fallback(get(static_asset))
        .layer(middleware::from_fn_with_state(config.clone(), auth))
        .with_state((host.clone(), config));

    let listener = match tokio::net::TcpListener::bind(&bind).await {
        Ok(listener) => listener,
        Err(error) => {
            eprintln!("无法监听 {bind}: {error}");
            std::process::exit(1);
        }
    };
    eprintln!("douyin-recorder 服务器已启动: http://{bind}");

    let server = axum::serve(listener, app).with_graceful_shutdown(shutdown_signal());
    if let Err(error) = server.await {
        eprintln!("服务器异常退出: {error}");
    }
    runtime.shutdown().await;
    eprintln!("已安全退出");
}

async fn shutdown_signal() {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };
    #[cfg(unix)]
    let terminate = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut signal) => {
                signal.recv().await;
            }
            Err(_) => std::future::pending::<()>().await,
        }
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();
    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }
    eprintln!("收到退出信号，正在停止录制并保存…");
}

async fn auth(
    State(config): State<AppConfig>,
    headers: HeaderMap,
    request: axum::extract::Request,
    next: Next,
) -> Response {
    // Only the API is protected. The UI shell and its static assets are public
    // so browsers can load the page and then supply a token in the app.
    // Liveness probes must also work without credentials.
    let path = request.uri().path();
    if !path.starts_with("/api/") || path == "/api/health" {
        return next.run(request).await;
    }
    let Some(expected) = config.token.as_deref() else {
        return next.run(request).await;
    };
    let header_token = headers
        .get("x-auth-token")
        .and_then(|value| value.to_str().ok());
    let query_token = request.uri().query().and_then(|query| {
        query
            .split('&')
            .find_map(|pair| pair.strip_prefix("token="))
    });
    let authorized = header_token.is_some_and(|value| constant_time_eq(value, expected))
        || query_token.is_some_and(|value| constant_time_eq(value, expected));
    if authorized {
        next.run(request).await
    } else {
        (StatusCode::UNAUTHORIZED, "未认证").into_response()
    }
}

/// Length-independent comparison so token checks do not leak timing.
fn constant_time_eq(candidate: &str, expected: &str) -> bool {
    let candidate = candidate.as_bytes();
    let expected = expected.as_bytes();
    if candidate.len() != expected.len() {
        return false;
    }
    candidate
        .iter()
        .zip(expected)
        .fold(0u8, |acc, (a, b)| acc | (a ^ b))
        == 0
}

async fn health() -> Json<Value> {
    Json(json!({"ok": true}))
}

async fn invoke(
    State((host, _)): State<(Host, AppConfig)>,
    Path(command): Path<String>,
    Json(args): Json<Value>,
) -> Response {
    match service::dispatch(&host, &command, args).await {
        Ok(value) => Json(value).into_response(),
        Err(error) => (StatusCode::BAD_REQUEST, Json(json!({ "error": error }))).into_response(),
    }
}

async fn events(
    State((host, _)): State<(Host, AppConfig)>,
) -> Sse<impl Stream<Item = Result<Event, std::convert::Infallible>>> {
    let receiver = host.bus.subscribe();
    let stream = BroadcastStream::new(receiver).filter_map(|message| match message {
        Ok((event, payload)) => Some(Ok(Event::default()
            .event(event.replace('-', "_"))
            .data(payload.to_string()))),
        Err(_) => None,
    });
    Sse::new(stream).keep_alive(KeepAlive::default())
}

async fn static_asset(uri: axum::http::Uri) -> Response {
    let path = uri.path().trim_start_matches('/');
    // Unknown API routes must not fall back to the SPA shell: clients (and
    // removed endpoints such as the old download route) should see a 404.
    if path == "api" || path.starts_with("api/") {
        return (StatusCode::NOT_FOUND, "未知接口").into_response();
    }
    if let Some(asset) = asset(path) {
        return Response::builder()
            .header(header::CONTENT_TYPE, asset.content_type)
            .header(header::CACHE_CONTROL, "public, max-age=300")
            .body(Body::from(asset.bytes))
            .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response());
    }
    // Single-page app fallback for unknown non-asset routes.
    match index_asset() {
        Some(asset) => Response::builder()
            .header(header::CONTENT_TYPE, asset.content_type)
            .body(Body::from(asset.bytes))
            .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response()),
        None => (
            StatusCode::NOT_FOUND,
            "前端资源未打包：请先运行 pnpm build 再编译服务器",
        )
            .into_response(),
    }
}
