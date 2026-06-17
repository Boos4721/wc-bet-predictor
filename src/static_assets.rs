// 把 web/dist 嵌入二进制(release),debug 下从磁盘读取(便于开发)。
// 作为路由 fallback 提供静态资源;/api/* 路由优先匹配。
use axum::{
    body::Body,
    http::{header, StatusCode, Uri},
    response::{IntoResponse, Response},
};
use rust_embed::RustEmbed;

#[derive(RustEmbed)]
#[folder = "web/dist"]
struct Assets;

/// 根据扩展名推断 Content-Type。
fn content_type(path: &str) -> &'static str {
    match path.rsplit('.').next() {
        Some("html") => "text/html; charset=utf-8",
        Some("js") => "text/javascript; charset=utf-8",
        Some("css") => "text/css; charset=utf-8",
        Some("json") => "application/json",
        Some("svg") => "image/svg+xml",
        Some("png") => "image/png",
        Some("jpg") | Some("jpeg") => "image/jpeg",
        Some("ico") => "image/x-icon",
        Some("woff2") => "font/woff2",
        Some("woff") => "font/woff",
        _ => "application/octet-stream",
    }
}

fn serve(path: &str) -> Option<Response> {
    Assets::get(path).map(|file| {
        (
            [(header::CONTENT_TYPE, content_type(path))],
            Body::from(file.data.into_owned()),
        )
            .into_response()
    })
}

/// 静态资源 fallback:`/` → index.html(落地页);其余按路径取嵌入文件;
/// 找不到则回退到 index.html(单页导航友好)。
pub async fn static_handler(uri: Uri) -> Response {
    let raw = uri.path().trim_start_matches('/');
    let path = if raw.is_empty() { "index.html" } else { raw };

    if let Some(resp) = serve(path) {
        return resp;
    }
    // 回退:落地页
    if let Some(resp) = serve("index.html") {
        return resp;
    }
    (StatusCode::NOT_FOUND, "404").into_response()
}
