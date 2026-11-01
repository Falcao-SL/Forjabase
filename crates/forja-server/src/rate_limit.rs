use axum::{
    Json,
    extract::{Request, State},
    http::{HeaderValue, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
};
use forja_core::store::RateLimitRule;
use serde_json::json;
use std::{
    collections::HashMap,
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};
use tokio::sync::Mutex;

use crate::Estado;

/// Gerenciador de limite de taxa de requisições em memória.
#[derive(Debug, Clone, Default)]
pub struct RateLimiter {
    // Mapeia `chave` ("{idx}:{label}:{ip}") -> (inicio_janela_s, contagem)
    entradas: Arc<Mutex<HashMap<String, (u64, u64)>>>,
}

impl RateLimiter {
    pub fn novo() -> Self {
        Self {
            entradas: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Verifica se a requisição excede alguma das regras ativas.
    /// Devolve `Ok(())` se permitida, ou `Err(retry_after_secs)` se bloqueada.
    pub async fn verificar(
        &self,
        regras: &[RateLimitRule],
        path: &str,
        client_ip: &str,
        is_auth: bool,
    ) -> Result<(), u64> {
        if regras.is_empty() {
            return Ok(());
        }

        let now_s = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        let mut lock = self.entradas.lock().await;

        for (idx, regra) in regras.iter().enumerate() {
            // 1. Confere público alvo (audience):
            let aud = regra.audience.trim().to_lowercase();
            if aud == "auth" && !is_auth {
                continue;
            }
            if aud == "guest" && is_auth {
                continue;
            }

            // 2. Confere rota / label:
            if !regra.label.is_empty()
                && !path.starts_with(&regra.label)
                && !path.contains(&regra.label)
            {
                continue;
            }

            let duracao = regra.duration.max(1);
            let limite = regra.max_requests.max(1);

            let chave = format!("{idx}:{}:{}", regra.label, client_ip);
            let entrada = lock.entry(chave).or_insert((now_s, 0));

            if now_s >= entrada.0 + duracao {
                // Janela expirou, reinicia a contagem
                *entrada = (now_s, 1);
            } else {
                entrada.1 += 1;
                if entrada.1 > limite {
                    let retry_after = (entrada.0 + duracao).saturating_sub(now_s).max(1);
                    return Err(retry_after);
                }
            }
        }

        // Limpeza de entradas muito antigas se o mapa crescer
        if lock.len() > 10_000 {
            lock.retain(|_, (inicio, _)| now_s < *inicio + 86400);
        }

        Ok(())
    }
}

/// Middleware assíncrono para limitar taxa de requisições nas rotas `/api/...`.
pub async fn rate_limit_middleware(State(e): State<Estado>, req: Request, next: Next) -> Response {
    let cfg = e.banco.configuracoes().await;
    if !cfg.rate_limits.enabled || cfg.rate_limits.rules.is_empty() {
        return next.run(req).await;
    }

    let path = req.uri().path().to_string();

    // Obtém o IP do cliente através de proxies confiáveis ou fallback local
    let client_ip = req
        .headers()
        .get("x-forwarded-for")
        .or_else(|| req.headers().get("x-real-ip"))
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.split(',').next())
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|| "127.0.0.1".into());

    let is_auth = req.headers().contains_key("authorization");

    match e
        .rate_limiter
        .verificar(&cfg.rate_limits.rules, &path, &client_ip, is_auth)
        .await
    {
        Ok(()) => next.run(req).await,
        Err(retry_after) => {
            let mut res = (
                StatusCode::TOO_MANY_REQUESTS,
                Json(json!({
                    "code": 429,
                    "message": "Too Many Requests.",
                    "data": {}
                })),
            )
                .into_response();

            if let Ok(val) = HeaderValue::from_str(&retry_after.to_string()) {
                res.headers_mut().insert("retry-after", val);
            }
            res
        }
    }
}
