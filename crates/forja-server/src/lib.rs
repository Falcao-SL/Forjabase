//! Servidor HTTP do Forja (axum). As rotas, os corpos e os erros seguem o
//! formato da API do PocketBase, para os SDKs existentes funcionarem.

use axum::extract::{FromRequestParts, Path, Query, State};
use axum::http::{StatusCode, request::Parts};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use forja_core::auth::{self, COLECAO_SUPERUSERS, VALIDADE_SUPERUSER};
use forja_core::store::{Banco, Consulta, Sqlite, Superusuario};
use forja_core::{Collection, Erro, Field};
use serde::Deserialize;
use serde_json::{Map, Value, json};
use std::net::SocketAddr;
use std::sync::Arc;

/// Estado compartilhado: o banco do projeto e o segredo dos tokens.
#[derive(Clone)]
pub struct Estado {
    pub banco: Arc<dyn Banco>,
    pub segredo: Arc<String>,
}

impl Estado {
    pub fn novo(banco: Arc<dyn Banco>) -> Result<Estado, Erro> {
        let segredo = banco.segredo()?;
        Ok(Estado {
            banco,
            segredo: Arc::new(segredo),
        })
    }
}

/// Erro HTTP no formato do PocketBase.
pub struct ErroApi(Erro);

impl From<Erro> for ErroApi {
    fn from(e: Erro) -> Self {
        ErroApi(e)
    }
}

impl IntoResponse for ErroApi {
    fn into_response(self) -> Response {
        let codigo = self.0.codigo_http();
        let (mensagem, dados) = match self.0 {
            Erro::Invalido { mensagem, campos } => (mensagem, Value::Object(campos)),
            Erro::Interno(m) => {
                eprintln!("erro interno: {m}");
                (
                    "Something went wrong while processing your request.".to_string(),
                    json!({}),
                )
            }
            outro => (outro.to_string(), json!({})),
        };
        let status = StatusCode::from_u16(codigo).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
        (
            status,
            Json(json!({ "code": codigo, "message": mensagem, "data": dados })),
        )
            .into_response()
    }
}

type Res<T> = Result<T, ErroApi>;

/// Roda uma operação do banco fora das threads do runtime.
async fn no_banco<T: Send + 'static>(
    e: &Estado,
    f: impl FnOnce(&dyn Banco) -> Result<T, Erro> + Send + 'static,
) -> Res<T> {
    let b = e.banco.clone();
    tokio::task::spawn_blocking(move || f(b.as_ref()))
        .await
        .map_err(|e| ErroApi(Erro::Interno(e.to_string())))?
        .map_err(ErroApi)
}

// ── Autenticação ─────────────────────────────────────────────────────────

/// Quem está chamando: nada ou um superusuário (usuários comuns vêm na 0.2).
pub struct Chamador(pub Option<Superusuario>);

impl Chamador {
    fn superusuario(&self) -> bool {
        self.0.is_some()
    }

    fn exigir_superusuario(&self) -> Res<()> {
        if self.superusuario() {
            Ok(())
        } else {
            Err(ErroApi(Erro::Proibido(
                "Only superusers can perform this action.".into(),
            )))
        }
    }
}

fn token_invalido() -> ErroApi {
    ErroApi(Erro::NaoAutorizado("Invalid or expired token.".into()))
}

impl FromRequestParts<Estado> for Chamador {
    type Rejection = ErroApi;

    async fn from_request_parts(parts: &mut Parts, e: &Estado) -> Result<Self, Self::Rejection> {
        let Some(bruto) = parts
            .headers
            .get("authorization")
            .and_then(|v| v.to_str().ok())
        else {
            return Ok(Chamador(None));
        };
        // O PocketBase aceita o token puro; aceitamos também "Bearer <token>".
        let token = bruto
            .strip_prefix("Bearer ")
            .unwrap_or(bruto)
            .trim()
            .to_string();
        if token.is_empty() {
            return Ok(Chamador(None));
        }
        let claims = auth::validar(&e.segredo, &token).map_err(|_| token_invalido())?;
        if claims.colecao != COLECAO_SUPERUSERS || claims.tipo != "auth" {
            return Err(token_invalido());
        }
        let id = claims.id.clone();
        let su = no_banco(e, move |b| b.superuser(&id))
            .await
            .map_err(|_| token_invalido())?;
        if su.token_key != claims.tk {
            return Err(token_invalido());
        }
        Ok(Chamador(Some(su)))
    }
}

#[derive(Deserialize)]
struct Login {
    identity: String,
    password: String,
}

async fn superuser_login(State(e): State<Estado>, Json(l): Json<Login>) -> Res<Json<Value>> {
    let falha = || ErroApi(Erro::invalido("Failed to authenticate."));
    let identidade = l.identity.clone();
    let su = no_banco(&e, move |b| b.superuser_por_email(&identidade))
        .await
        .map_err(|_| falha())?;
    let (hash, senha) = (su.senha.clone(), l.password);
    let ok = tokio::task::spawn_blocking(move || auth::conferir_senha(&hash, &senha))
        .await
        .unwrap_or(false);
    if !ok {
        return Err(falha());
    }
    resposta_auth(&e, su)
}

async fn superuser_refresh(State(e): State<Estado>, quem: Chamador) -> Res<Json<Value>> {
    match quem.0 {
        Some(su) => resposta_auth(&e, su),
        None => Err(ErroApi(Erro::NaoAutorizado(
            "The request requires valid record authorization token.".into(),
        ))),
    }
}

fn resposta_auth(e: &Estado, su: Superusuario) -> Res<Json<Value>> {
    let token = auth::emitir(
        &e.segredo,
        &su.id,
        COLECAO_SUPERUSERS,
        &su.token_key,
        VALIDADE_SUPERUSER,
    )?;
    Ok(Json(json!({ "token": token, "record": su })))
}

// ── Coleções ─────────────────────────────────────────────────────────────

async fn colecoes_listar(State(e): State<Estado>, quem: Chamador) -> Res<Json<Value>> {
    quem.exigir_superusuario()?;
    let itens = no_banco(&e, |b| b.colecoes()).await?;
    let total = itens.len();
    Ok(Json(
        json!({ "page": 1, "perPage": total.max(1), "totalItems": total, "totalPages": 1, "items": itens }),
    ))
}

async fn colecoes_criar(
    State(e): State<Estado>,
    quem: Chamador,
    Json(c): Json<Collection>,
) -> Res<Json<Collection>> {
    quem.exigir_superusuario()?;
    Ok(Json(no_banco(&e, move |b| b.criar_colecao(c)).await?))
}

async fn colecoes_ver(
    State(e): State<Estado>,
    quem: Chamador,
    Path(nome): Path<String>,
) -> Res<Json<Collection>> {
    quem.exigir_superusuario()?;
    Ok(Json(no_banco(&e, move |b| b.colecao(&nome)).await?))
}

async fn colecoes_apagar(
    State(e): State<Estado>,
    quem: Chamador,
    Path(nome): Path<String>,
) -> Res<StatusCode> {
    quem.exigir_superusuario()?;
    no_banco(&e, move |b| b.apagar_colecao(&nome)).await?;
    Ok(StatusCode::NO_CONTENT)
}

// ── Registros ────────────────────────────────────────────────────────────

/// A regra que vale para quem chama. `Ok(None)` = sem restrição (superusuário
/// ou regra pública); `Err` = só superusuário.
fn regra_para(quem: &Chamador, regra: &Option<String>) -> Res<Option<String>> {
    if quem.superusuario() {
        return Ok(None);
    }
    match regra {
        None => Err(ErroApi(Erro::Proibido(
            "Only superusers can perform this action.".into(),
        ))),
        Some(r) if r.trim().is_empty() => Ok(None),
        Some(r) => Ok(Some(r.clone())),
    }
}

/// Campos `hidden` só aparecem para superusuários.
fn esconder(c: &Collection, quem: &Chamador, mut v: Value) -> Value {
    if !quem.superusuario()
        && let Some(m) = v.as_object_mut()
    {
        for f in c.fields.iter().filter(|f: &&Field| f.hidden) {
            m.remove(&f.name);
        }
    }
    v
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ParamsLista {
    page: Option<u32>,
    per_page: Option<u32>,
    sort: Option<String>,
    filter: Option<String>,
    skip_total: Option<bool>,
}

async fn colecao_de(e: &Estado, nome: String) -> Res<Collection> {
    no_banco(e, move |b| b.colecao(&nome)).await
}

async fn registros_listar(
    State(e): State<Estado>,
    quem: Chamador,
    Path(nome): Path<String>,
    Query(p): Query<ParamsLista>,
) -> Res<Json<Value>> {
    let c = colecao_de(&e, nome).await?;
    let regra = regra_para(&quem, &c.list_rule)?;
    let q = Consulta {
        filtro: p.filter,
        ordem: p.sort,
        pagina: p.page.unwrap_or(1),
        por_pagina: p.per_page.unwrap_or(30),
        pular_total: p.skip_total.unwrap_or(false),
        regra,
    };
    let c2 = c.clone();
    let mut lista = no_banco(&e, move |b| b.listar(&c2, &q)).await?;
    lista.items = lista
        .items
        .into_iter()
        .map(|v| esconder(&c, &quem, v))
        .collect();
    Ok(Json(serde_json::to_value(lista).unwrap_or_default()))
}

async fn registros_ver(
    State(e): State<Estado>,
    quem: Chamador,
    Path((nome, id)): Path<(String, String)>,
) -> Res<Json<Value>> {
    let c = colecao_de(&e, nome).await?;
    let regra = regra_para(&quem, &c.view_rule)?;
    let c2 = c.clone();
    let r = no_banco(&e, move |b| b.registro(&c2, &id, regra.as_deref())).await?;
    Ok(Json(esconder(&c, &quem, r)))
}

async fn registros_criar(
    State(e): State<Estado>,
    quem: Chamador,
    Path(nome): Path<String>,
    Json(dados): Json<Map<String, Value>>,
) -> Res<Json<Value>> {
    let c = colecao_de(&e, nome).await?;
    let regra = regra_para(&quem, &c.create_rule)?;
    let c2 = c.clone();
    let r = no_banco(&e, move |b| {
        let novo = b.criar(&c2, &dados)?;
        // A regra de criação é conferida no registro criado; se não passar, ele sai.
        if let Some(regra) = regra {
            let id = novo["id"].as_str().unwrap_or_default().to_string();
            if b.registro(&c2, &id, Some(&regra)).is_err() {
                b.apagar(&c2, &id, None)?;
                return Err(Erro::invalido("Failed to create record."));
            }
        }
        Ok(novo)
    })
    .await?;
    Ok(Json(esconder(&c, &quem, r)))
}

async fn registros_atualizar(
    State(e): State<Estado>,
    quem: Chamador,
    Path((nome, id)): Path<(String, String)>,
    Json(dados): Json<Map<String, Value>>,
) -> Res<Json<Value>> {
    let c = colecao_de(&e, nome).await?;
    let regra = regra_para(&quem, &c.update_rule)?;
    let c2 = c.clone();
    let r = no_banco(&e, move |b| b.atualizar(&c2, &id, &dados, regra.as_deref())).await?;
    Ok(Json(esconder(&c, &quem, r)))
}

async fn registros_apagar(
    State(e): State<Estado>,
    quem: Chamador,
    Path((nome, id)): Path<(String, String)>,
) -> Res<StatusCode> {
    let c = colecao_de(&e, nome).await?;
    let regra = regra_para(&quem, &c.delete_rule)?;
    no_banco(&e, move |b| b.apagar(&c, &id, regra.as_deref())).await?;
    Ok(StatusCode::NO_CONTENT)
}

// ── Rotas ────────────────────────────────────────────────────────────────

/// `GET /api/health`, com o mesmo corpo do PocketBase.
async fn health() -> Json<Value> {
    Json(json!({ "code": 200, "message": "API is healthy.", "data": {} }))
}

/// As rotas do Forja.
pub fn router(e: Estado) -> Router {
    Router::new()
        .route("/api/health", get(health))
        .route(
            "/api/collections/_superusers/auth-with-password",
            post(superuser_login),
        )
        .route(
            "/api/collections/_superusers/auth-refresh",
            post(superuser_refresh),
        )
        .route(
            "/api/collections",
            get(colecoes_listar).post(colecoes_criar),
        )
        .route(
            "/api/collections/{colecao}",
            get(colecoes_ver).delete(colecoes_apagar),
        )
        .route(
            "/api/collections/{colecao}/records",
            get(registros_listar).post(registros_criar),
        )
        .route(
            "/api/collections/{colecao}/records/{id}",
            get(registros_ver)
                .patch(registros_atualizar)
                .delete(registros_apagar),
        )
        .with_state(e)
}

/// Abre o projeto padrão em `pasta/default` e atende até o processo terminar.
pub async fn serve(addr: SocketAddr, pasta: std::path::PathBuf) -> std::io::Result<()> {
    let banco =
        Sqlite::abrir(&pasta.join("default")).map_err(|e| std::io::Error::other(e.to_string()))?;
    let estado = Estado::novo(Arc::new(banco)).map_err(|e| std::io::Error::other(e.to_string()))?;
    if estado.banco.total_superusers().unwrap_or(0) == 0 {
        eprintln!(
            "Nenhum superusuário ainda. Crie um com:\n  forja superuser upsert <email> <senha> --dir {}",
            pasta.display()
        );
    }
    let listener = tokio::net::TcpListener::bind(addr).await?;
    eprintln!(
        "Forja {} em http://{addr} (dados em {})",
        forja_core::VERSION,
        pasta.display()
    );
    axum::serve(listener, router(estado)).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::Request;
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    fn estado() -> Estado {
        Estado::novo(Arc::new(Sqlite::em_memoria().unwrap())).unwrap()
    }

    async fn chamar(
        e: &Estado,
        metodo: &str,
        uri: &str,
        token: Option<&str>,
        corpo: Option<Value>,
    ) -> (StatusCode, Value) {
        let mut req = Request::builder().method(metodo).uri(uri);
        if let Some(t) = token {
            req = req.header("authorization", t);
        }
        let req = match corpo {
            Some(c) => req
                .header("content-type", "application/json")
                .body(Body::from(c.to_string()))
                .unwrap(),
            None => req.body(Body::empty()).unwrap(),
        };
        let resp = router(e.clone()).oneshot(req).await.unwrap();
        let status = resp.status();
        let bytes = resp.into_body().collect().await.unwrap().to_bytes();
        (
            status,
            serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        )
    }

    async fn login(e: &Estado) -> String {
        e.banco
            .superuser_salvar("admin@exemplo.com", "senha-forte-123")
            .unwrap();
        let (s, v) = chamar(
            e,
            "POST",
            "/api/collections/_superusers/auth-with-password",
            None,
            Some(json!({"identity": "admin@exemplo.com", "password": "senha-forte-123"})),
        )
        .await;
        assert_eq!(s, StatusCode::OK, "{v}");
        assert_eq!(v["record"]["email"], json!("admin@exemplo.com"));
        assert!(v["record"].get("password").is_none());
        v["token"].as_str().unwrap().to_string()
    }

    #[tokio::test]
    async fn health_igual_ao_pocketbase() {
        let (s, v) = chamar(&estado(), "GET", "/api/health", None, None).await;
        assert_eq!(s, StatusCode::OK);
        assert_eq!(
            v,
            json!({ "code": 200, "message": "API is healthy.", "data": {} })
        );
    }

    #[tokio::test]
    async fn login_errado_e_token_invalido() {
        let e = estado();
        e.banco
            .superuser_salvar("admin@exemplo.com", "senha-forte-123")
            .unwrap();
        let (s, v) = chamar(
            &e,
            "POST",
            "/api/collections/_superusers/auth-with-password",
            None,
            Some(json!({"identity": "admin@exemplo.com", "password": "errada"})),
        )
        .await;
        assert_eq!(s, StatusCode::BAD_REQUEST);
        assert_eq!(v["message"], json!("Failed to authenticate."));
        let (s, _) = chamar(&e, "GET", "/api/collections", Some("lixo"), None).await;
        assert_eq!(s, StatusCode::UNAUTHORIZED);
        let (s, _) = chamar(&e, "GET", "/api/collections", None, None).await;
        assert_eq!(s, StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn trocar_senha_invalida_tokens_antigos() {
        let e = estado();
        let t = login(&e).await;
        let (s, _) = chamar(
            &e,
            "POST",
            "/api/collections/_superusers/auth-refresh",
            Some(&t),
            None,
        )
        .await;
        assert_eq!(s, StatusCode::OK);
        e.banco
            .superuser_salvar("admin@exemplo.com", "nova-senha-456")
            .unwrap();
        let (s, _) = chamar(
            &e,
            "GET",
            "/api/collections",
            Some(&format!("Bearer {t}")),
            None,
        )
        .await;
        assert_eq!(s, StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn fluxo_completo_de_colecao_e_registros() {
        let e = estado();
        let t = login(&e).await;
        let col = json!({
            "name": "posts",
            "fields": [
                {"name": "titulo", "type": "text", "required": true},
                {"name": "publicado", "type": "bool"},
                {"name": "nota", "type": "text", "hidden": true}
            ],
            "listRule": "publicado = true",
            "viewRule": "publicado = true",
            "createRule": "",
            "updateRule": null,
            "deleteRule": null
        });
        let (s, v) = chamar(&e, "POST", "/api/collections", Some(&t), Some(col)).await;
        assert_eq!(s, StatusCode::OK, "{v}");
        assert!(v["id"].as_str().unwrap().starts_with("pbc_"));

        // Visitante cria (createRule pública), mas a validação vale.
        let (s, v) = chamar(
            &e,
            "POST",
            "/api/collections/posts/records",
            None,
            Some(json!({"publicado": true})),
        )
        .await;
        assert_eq!(s, StatusCode::BAD_REQUEST);
        assert_eq!(v["data"]["titulo"]["code"], json!("validation_required"));
        let (s, pub_) = chamar(
            &e,
            "POST",
            "/api/collections/posts/records",
            None,
            Some(json!({"titulo": "Olá", "publicado": true, "nota": "secreta"})),
        )
        .await;
        assert_eq!(s, StatusCode::OK, "{pub_}");
        assert!(
            pub_.get("nota").is_none(),
            "campo hidden apareceu para visitante"
        );
        let (_, rascunho) = chamar(
            &e,
            "POST",
            "/api/collections/posts/records",
            Some(&t),
            Some(json!({"titulo": "Rascunho", "publicado": false})),
        )
        .await;

        // A regra de listagem esconde o rascunho do visitante, não do superusuário.
        let (_, v) = chamar(&e, "GET", "/api/collections/posts/records", None, None).await;
        assert_eq!(v["totalItems"], json!(1));
        let (_, v) = chamar(
            &e,
            "GET",
            "/api/collections/posts/records?sort=-created&perPage=1",
            Some(&t),
            None,
        )
        .await;
        assert_eq!(
            (v["totalItems"].clone(), v["totalPages"].clone()),
            (json!(2), json!(2))
        );
        // O superusuário vê o campo hidden (qual registro vem primeiro não importa).
        assert!(v["items"][0].get("nota").is_some(), "{v}");
        let (_, v) = chamar(
            &e,
            "GET",
            "/api/collections/posts/records?filter=(titulo~'Ol')",
            None,
            None,
        )
        .await;
        assert_eq!(v["items"][0]["titulo"], json!("Olá"));
        let (s, v) = chamar(
            &e,
            "GET",
            "/api/collections/posts/records?filter=senha='x'",
            None,
            None,
        )
        .await;
        assert_eq!(s, StatusCode::BAD_REQUEST, "{v}");

        let rid = rascunho["id"].as_str().unwrap();
        let (s, _) = chamar(
            &e,
            "GET",
            &format!("/api/collections/posts/records/{rid}"),
            None,
            None,
        )
        .await;
        assert_eq!(s, StatusCode::NOT_FOUND);

        // Atualizar e apagar: regra nula = só superusuário.
        let pid = pub_["id"].as_str().unwrap();
        let (s, _) = chamar(
            &e,
            "PATCH",
            &format!("/api/collections/posts/records/{pid}"),
            None,
            Some(json!({"titulo": "x"})),
        )
        .await;
        assert_eq!(s, StatusCode::FORBIDDEN);
        let (s, v) = chamar(
            &e,
            "PATCH",
            &format!("/api/collections/posts/records/{pid}"),
            Some(&t),
            Some(json!({"titulo": "Novo"})),
        )
        .await;
        assert_eq!((s, v["titulo"].clone()), (StatusCode::OK, json!("Novo")));
        let (s, _) = chamar(
            &e,
            "DELETE",
            &format!("/api/collections/posts/records/{pid}"),
            Some(&t),
            None,
        )
        .await;
        assert_eq!(s, StatusCode::NO_CONTENT);

        let (s, _) = chamar(&e, "DELETE", "/api/collections/posts", Some(&t), None).await;
        assert_eq!(s, StatusCode::NO_CONTENT);
        let (s, v) = chamar(&e, "GET", "/api/collections/posts/records", Some(&t), None).await;
        assert_eq!(s, StatusCode::NOT_FOUND, "{v}");
    }
}
