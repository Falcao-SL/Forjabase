//! Servidor HTTP do Forja (axum). As rotas seguem o formato da API do
//! PocketBase, para os SDKs existentes funcionarem.

use axum::{Json, Router, routing::get};
use serde_json::{Value, json};
use std::net::SocketAddr;

/// As rotas do Forja.
pub fn router() -> Router {
    Router::new().route("/api/health", get(health))
}

/// `GET /api/health`, com o mesmo corpo do PocketBase.
async fn health() -> Json<Value> {
    Json(json!({ "code": 200, "message": "API is healthy.", "data": {} }))
}

/// Sobe o servidor e atende até o processo terminar.
pub async fn serve(addr: SocketAddr) -> std::io::Result<()> {
    let listener = tokio::net::TcpListener::bind(addr).await?;
    eprintln!("Forja {} em http://{addr}", forja_core::VERSION);
    axum::serve(listener, router()).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    #[tokio::test]
    async fn health_igual_ao_pocketbase() {
        let resp = router()
            .oneshot(Request::get("/api/health").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let corpo = resp.into_body().collect().await.unwrap().to_bytes();
        let v: Value = serde_json::from_slice(&corpo).unwrap();
        assert_eq!(
            v,
            json!({ "code": 200, "message": "API is healthy.", "data": {} })
        );
    }
}
