use std::{
    collections::HashMap,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
};

use axum::{
    Json, Router,
    extract::{Path, State},
    http::{HeaderMap, HeaderValue, Method, StatusCode, header},
    response::IntoResponse,
    routing::{get, post},
};
use rand::{Rng, distr::Alphanumeric};
use serde::{Deserialize, Serialize};
use tokio_util::sync::CancellationToken;
use tower_http::cors::CorsLayer;

use crate::app::{DownloadOptions, run_download};

const ADDRESS: &str = "127.0.0.1:19110";
const ALLOWED_ORIGIN: &str = "https://ereserves.lib.tsinghua.edu.cn";

struct Job {
    status: Arc<Mutex<String>>,
    current: Arc<AtomicUsize>,
    total: Arc<AtomicUsize>,
    output: Mutex<Option<String>>,
    error: Mutex<Option<String>>,
    cancel: CancellationToken,
}

struct AppState {
    secret: String,
    jobs: Mutex<HashMap<String, Arc<Job>>>,
}

#[derive(Deserialize)]
struct CreateJob {
    url: String,
    token: String,
    #[serde(default = "default_threads")]
    threads: u8,
    #[serde(default = "default_quality")]
    quality: u8,
    #[serde(default)]
    auto_resize: bool,
    #[serde(default = "default_true")]
    delete_images: bool,
}

#[derive(Serialize)]
struct JobView {
    id: String,
    status: String,
    current: usize,
    total: usize,
    percent: f32,
    output: Option<String>,
    error: Option<String>,
}

fn default_threads() -> u8 {
    4
}
fn default_quality() -> u8 {
    10
}
fn default_true() -> bool {
    true
}

fn random_string(length: usize) -> String {
    rand::rng()
        .sample_iter(Alphanumeric)
        .take(length)
        .map(char::from)
        .collect()
}

fn authorized(headers: &HeaderMap, state: &AppState) -> bool {
    headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value == format!("Bearer {}", state.secret))
}

async fn health() -> Json<serde_json::Value> {
    Json(serde_json::json!({"ok": true, "name": "thubookrs", "api": 1}))
}

async fn pair(State(state): State<Arc<AppState>>, headers: HeaderMap) -> impl IntoResponse {
    if headers
        .get("x-thubookrs-pair")
        .and_then(|v| v.to_str().ok())
        != Some("userscript-v1")
    {
        return (
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({"error": "拒绝配对"})),
        );
    }
    (
        StatusCode::OK,
        Json(serde_json::json!({"secret": state.secret})),
    )
}

fn validate_request(request: &CreateJob) -> Result<(), String> {
    let url = reqwest::Url::parse(&request.url).map_err(|_| "书籍地址无效")?;
    if url.scheme() != "https"
        || url.host_str() != Some("ereserves.lib.tsinghua.edu.cn")
        || !url.path().starts_with("/bookDetail/")
    {
        return Err("只允许清华教参平台的书籍详情地址".into());
    }
    if request.token.trim().is_empty() || request.token.len() > 8192 {
        return Err("登录 token 无效".into());
    }
    if !(1..=16).contains(&request.threads) || !(3..=10).contains(&request.quality) {
        return Err("线程数或清晰度超出允许范围".into());
    }
    Ok(())
}

async fn create_job(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(request): Json<CreateJob>,
) -> impl IntoResponse {
    if !authorized(&headers, &state) {
        return (
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({"error": "未配对"})),
        );
    }
    if let Err(error) = validate_request(&request) {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": error})),
        );
    }

    let id = random_string(16);
    let job = Arc::new(Job {
        status: Arc::new(Mutex::new("queued".into())),
        current: Arc::new(AtomicUsize::new(0)),
        total: Arc::new(AtomicUsize::new(0)),
        output: Mutex::new(None),
        error: Mutex::new(None),
        cancel: CancellationToken::new(),
    });
    state.jobs.lock().unwrap().insert(id.clone(), job.clone());

    let output_root = match std::env::current_dir() {
        Ok(path) => path.join("downloads"),
        Err(error) => {
            state.jobs.lock().unwrap().remove(&id);
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": error.to_string()})),
            );
        }
    };
    let options = DownloadOptions {
        url: request.url,
        token: request.token,
        thread_number: request.threads as usize,
        quality: request.quality as u32,
        del_img: request.delete_images,
        auto_resize: request.auto_resize,
        output_root,
    };
    tokio::spawn(async move {
        let result = run_download(
            options,
            job.cancel.clone(),
            Some(job.current.clone()),
            Some(job.total.clone()),
            Some(job.status.clone()),
        )
        .await;
        match result {
            Ok(path) => {
                *job.output.lock().unwrap() = Some(path.display().to_string());
                *job.status.lock().unwrap() = "completed".into();
            }
            Err(error) => {
                *job.error.lock().unwrap() = Some(error);
                *job.status.lock().unwrap() = if job.cancel.is_cancelled() {
                    "cancelled"
                } else {
                    "failed"
                }
                .into();
            }
        }
    });

    (StatusCode::ACCEPTED, Json(serde_json::json!({"id": id})))
}

async fn get_job(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> impl IntoResponse {
    if !authorized(&headers, &state) {
        return (
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({"error": "未配对"})),
        );
    }
    let job = state.jobs.lock().unwrap().get(&id).cloned();
    let Some(job) = job else {
        return (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "任务不存在"})),
        );
    };
    let current = job.current.load(Ordering::Relaxed);
    let total = job.total.load(Ordering::Relaxed);
    let view = JobView {
        id,
        status: job.status.lock().unwrap().clone(),
        current,
        total,
        percent: if total == 0 {
            0.0
        } else {
            current as f32 / total as f32 * 100.0
        },
        output: job.output.lock().unwrap().clone(),
        error: job.error.lock().unwrap().clone(),
    };
    (StatusCode::OK, Json(serde_json::to_value(view).unwrap()))
}

async fn cancel_job(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> impl IntoResponse {
    if !authorized(&headers, &state) {
        return (
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({"error": "未配对"})),
        );
    }
    let job = state.jobs.lock().unwrap().get(&id).cloned();
    let Some(job) = job else {
        return (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "任务不存在"})),
        );
    };
    job.cancel.cancel();
    (StatusCode::OK, Json(serde_json::json!({"ok": true})))
}

pub async fn serve() -> Result<(), Box<dyn std::error::Error>> {
    let state = Arc::new(AppState {
        secret: random_string(48),
        jobs: Mutex::new(HashMap::new()),
    });
    let cors = CorsLayer::new()
        .allow_origin(ALLOWED_ORIGIN.parse::<HeaderValue>()?)
        .allow_methods([Method::GET, Method::POST, Method::DELETE])
        .allow_headers([
            header::AUTHORIZATION,
            header::CONTENT_TYPE,
            "x-thubookrs-pair".parse()?,
        ]);
    let app = Router::new()
        .route("/v1/health", get(health))
        .route("/v1/pair", post(pair))
        .route("/v1/jobs", post(create_job))
        .route("/v1/jobs/{id}", get(get_job).delete(cancel_job))
        .layer(cors)
        .with_state(state);
    let listener = tokio::net::TcpListener::bind(ADDRESS).await?;
    println!("thubookrs 浏览器服务已启动：http://{ADDRESS}");
    println!("请保持此窗口运行，然后在教参书籍页面点击下载按钮。");
    axum::serve(listener, app).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(url: &str) -> CreateJob {
        CreateJob {
            url: url.into(),
            token: "a-valid-looking-token".into(),
            threads: 4,
            quality: 10,
            auto_resize: false,
            delete_images: true,
        }
    }

    #[test]
    fn accepts_book_detail_url() {
        assert!(
            validate_request(&request(
                "https://ereserves.lib.tsinghua.edu.cn/bookDetail/abc123"
            ))
            .is_ok()
        );
    }

    #[test]
    fn rejects_external_or_non_book_urls() {
        assert!(validate_request(&request("https://example.com/bookDetail/abc")).is_err());
        assert!(
            validate_request(&request("https://ereserves.lib.tsinghua.edu.cn/search")).is_err()
        );
    }
}
