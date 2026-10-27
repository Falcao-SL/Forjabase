//! Painel administrativo: HTML no servidor, htmx no navegador.
//!
//! A sessão do painel é um cookie `httpOnly` com um JWT de superusuário. A API
//! continua igual — o cookie é só um atalho para o navegador não precisar
//! mandar `Authorization` em cada formulário.

use crate::Estado;
use axum::body::{Body, Bytes};
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{Html, IntoResponse, Redirect, Response};
use axum::routing::{get, post};
use axum::{Form, Router};
use forja_admin as admin;
use forja_core::auth::{self, COLECAO_SUPERUSERS, TIPO_AUTH, VALIDADE_SUPERUSER};
use forja_core::store::{Consulta, Lista, Superusuario};
use forja_core::{Collection, Erro, FieldType};
use maud::Markup;
use serde::Deserialize;
use serde_json::{Map, Value, json};

const COOKIE: &str = "forja_painel";

pub fn rotas() -> Router<Estado> {
    Router::new()
        .route("/_", get(raiz))
        .route("/_/", get(inicio))
        .route("/_/login", get(login_form).post(login))
        .route("/_/logout", post(logout))
        .route("/_/htmx.min.js", get(htmx))
        .route("/_/collections/{nome}", get(colecao_pagina).post(criar))
        .route("/_/collections/{nome}/new", get(novo_form))
        .route("/_/collections/{nome}/tabela", get(tabela))
        .route(
            "/_/collections/{nome}/records/{id}",
            get(editar_form).post(atualizar),
        )
        .route("/_/collections/{nome}/records/{id}/delete", post(apagar))
}

// ── Sessão ───────────────────────────────────────────────────────────────

fn token_do_cookie(headers: &HeaderMap) -> Option<String> {
    let bruto = headers.get(header::COOKIE)?.to_str().ok()?;
    bruto.split(';').find_map(|parte| {
        let (nome, valor) = parte.trim().split_once('=')?;
        (nome == COOKIE).then(|| valor.to_string())
    })
}

async fn sessao(e: &Estado, headers: &HeaderMap) -> Option<Superusuario> {
    let token = token_do_cookie(headers)?;
    let claims = auth::validar(&e.segredo, &token).ok()?;
    if claims.tipo != TIPO_AUTH || claims.colecao != COLECAO_SUPERUSERS {
        return None;
    }
    let su = e.banco.superuser(&claims.id).await.ok()?;
    (su.token_key == claims.tk).then_some(su)
}

fn entrar() -> Response {
    Redirect::to("/_/login").into_response()
}

fn com_cookie(mut resp: Response, valor: &str, max_age: u64) -> Response {
    let cookie = format!("{COOKIE}={valor}; Path=/; HttpOnly; SameSite=Strict; Max-Age={max_age}");
    if let Ok(v) = HeaderValue::from_str(&cookie) {
        resp.headers_mut().append(header::SET_COOKIE, v);
    }
    resp
}

// ── Telas ────────────────────────────────────────────────────────────────

async fn raiz() -> Response {
    Redirect::to("/_/").into_response()
}

async fn inicio(State(e): State<Estado>, headers: HeaderMap) -> Response {
    if sessao(&e, &headers).await.is_none() {
        return entrar();
    }
    match e.banco.colecoes().await {
        Ok(itens) => html(admin::pagina("Coleções", admin::colecoes(&itens))),
        Err(erro) => pagina_erro(erro),
    }
}

async fn login_form(State(e): State<Estado>, headers: HeaderMap) -> Response {
    if sessao(&e, &headers).await.is_some() {
        return Redirect::to("/_/").into_response();
    }
    html(admin::pagina_login(None))
}

#[derive(Deserialize)]
struct LoginForm {
    identity: String,
    password: String,
}

async fn login(State(e): State<Estado>, Form(l): Form<LoginForm>) -> Response {
    let falha = || html(admin::pagina_login(Some("E-mail ou senha inválidos.")));
    let Ok(su) = e.banco.superuser_por_email(&l.identity).await else {
        return falha();
    };
    let (hash, senha) = (su.senha.clone(), l.password);
    let ok = tokio::task::spawn_blocking(move || auth::conferir_senha(&hash, &senha))
        .await
        .unwrap_or(false);
    if !ok {
        return falha();
    }
    let Ok(token) = auth::emitir(
        &e.segredo,
        &su.id,
        COLECAO_SUPERUSERS,
        &su.token_key,
        VALIDADE_SUPERUSER,
    ) else {
        return falha();
    };
    com_cookie(
        Redirect::to("/_/").into_response(),
        &token,
        VALIDADE_SUPERUSER,
    )
}

async fn logout() -> Response {
    com_cookie(Redirect::to("/_/login").into_response(), "", 0)
}

async fn htmx() -> Response {
    (
        [
            (header::CONTENT_TYPE, "text/javascript; charset=utf-8"),
            (header::CACHE_CONTROL, "public, max-age=86400"),
        ],
        Body::from(admin::HTMX),
    )
        .into_response()
}

#[derive(Deserialize)]
struct ParamsPainel {
    page: Option<u32>,
    filter: Option<String>,
}

async fn colecao_pagina(
    State(e): State<Estado>,
    headers: HeaderMap,
    Path(nome): Path<String>,
    Query(p): Query<ParamsPainel>,
) -> Response {
    if sessao(&e, &headers).await.is_none() {
        return entrar();
    }
    let Ok(c) = e.banco.colecao(&nome).await else {
        return pagina_erro(Erro::NaoEncontrado(format!(
            "A coleção '{nome}' não existe."
        )));
    };
    let filtro = p.filter.unwrap_or_default();
    match listar(&e, &c, p.page.unwrap_or(1), &filtro).await {
        Ok(lista) => html(admin::pagina(&c.name, admin::colecao(&c, &lista, &filtro))),
        Err(erro) => pagina_erro(erro),
    }
}

async fn tabela(
    State(e): State<Estado>,
    headers: HeaderMap,
    Path(nome): Path<String>,
    Query(p): Query<ParamsPainel>,
) -> Response {
    if sessao(&e, &headers).await.is_none() {
        return entrar();
    }
    let Ok(c) = e.banco.colecao(&nome).await else {
        return html(admin::nao_encontrado("A coleção não existe."));
    };
    let filtro = p.filter.unwrap_or_default();
    match listar(&e, &c, p.page.unwrap_or(1), &filtro).await {
        Ok(lista) => html(admin::tabela(&c, &lista, &filtro)),
        Err(erro) => html(admin::nao_encontrado(&erro.to_string())),
    }
}

async fn novo_form(
    State(e): State<Estado>,
    headers: HeaderMap,
    Path(nome): Path<String>,
) -> Response {
    if sessao(&e, &headers).await.is_none() {
        return entrar();
    }
    let Ok(c) = e.banco.colecao(&nome).await else {
        return pagina_erro(Erro::NaoEncontrado("A coleção não existe.".into()));
    };
    html(admin::pagina(
        &format!("Novo em {}", c.name),
        admin::formulario(&c, None, &Map::new(), None),
    ))
}

async fn editar_form(
    State(e): State<Estado>,
    headers: HeaderMap,
    Path((nome, id)): Path<(String, String)>,
) -> Response {
    if sessao(&e, &headers).await.is_none() {
        return entrar();
    }
    let Ok(c) = e.banco.colecao(&nome).await else {
        return pagina_erro(Erro::NaoEncontrado("A coleção não existe.".into()));
    };
    match e.banco.registro(&c, &id, None).await {
        Ok(r) => html(admin::pagina(
            &format!("Editar {}", c.name),
            admin::formulario(&c, Some(&r), &Map::new(), None),
        )),
        Err(_) => html(admin::nao_encontrado("O registro não existe.")),
    }
}

// ── Escrita ──────────────────────────────────────────────────────────────

async fn criar(
    State(e): State<Estado>,
    headers: HeaderMap,
    Path(nome): Path<String>,
    corpo: Bytes,
) -> Response {
    if sessao(&e, &headers).await.is_none() {
        return entrar();
    }
    let Ok(c) = e.banco.colecao(&nome).await else {
        return pagina_erro(Erro::NaoEncontrado("A coleção não existe.".into()));
    };
    let dados = dados_do_form(&c, &pares(&corpo));
    match e.banco.criar(&c, &dados).await {
        Ok(_) => Redirect::to(&format!("/_/collections/{nome}")).into_response(),
        Err(erro) => {
            let (erros, mensagem) = separar_erro(erro);
            let valores = Value::Object(dados);
            html(admin::pagina(
                &format!("Novo em {}", c.name),
                admin::formulario(&c, Some(&valores), &erros, mensagem.as_deref()),
            ))
        }
    }
}

async fn atualizar(
    State(e): State<Estado>,
    headers: HeaderMap,
    Path((nome, id)): Path<(String, String)>,
    corpo: Bytes,
) -> Response {
    if sessao(&e, &headers).await.is_none() {
        return entrar();
    }
    let Ok(c) = e.banco.colecao(&nome).await else {
        return pagina_erro(Erro::NaoEncontrado("A coleção não existe.".into()));
    };
    let Ok(atual) = e.banco.registro(&c, &id, None).await else {
        return html(admin::nao_encontrado("O registro não existe."));
    };
    let dados = dados_do_form(&c, &pares(&corpo));
    match e.banco.atualizar(&c, &id, &dados, None).await {
        Ok(_) => Redirect::to(&format!("/_/collections/{nome}")).into_response(),
        Err(erro) => {
            let (erros, mensagem) = separar_erro(erro);
            // Mantém o que foi digitado junto com o que já existia.
            let mut mesclado = atual.as_object().cloned().unwrap_or_default();
            for (k, v) in dados {
                mesclado.insert(k, v);
            }
            let valores = Value::Object(mesclado);
            html(admin::pagina(
                &format!("Editar {}", c.name),
                admin::formulario(&c, Some(&valores), &erros, mensagem.as_deref()),
            ))
        }
    }
}

async fn apagar(
    State(e): State<Estado>,
    headers: HeaderMap,
    Path((nome, id)): Path<(String, String)>,
) -> Response {
    if sessao(&e, &headers).await.is_none() {
        return entrar();
    }
    let Ok(c) = e.banco.colecao(&nome).await else {
        return pagina_erro(Erro::NaoEncontrado("A coleção não existe.".into()));
    };
    if let Err(erro) = e.banco.apagar(&c, &id, None).await {
        return pagina_erro(erro);
    }
    // htmx troca só a tabela; sem JS, volta para a lista.
    if headers.contains_key("hx-request") {
        return match listar(&e, &c, 1, "").await {
            Ok(lista) => html(admin::tabela(&c, &lista, "")),
            Err(erro) => html(admin::nao_encontrado(&erro.to_string())),
        };
    }
    Redirect::to(&format!("/_/collections/{nome}")).into_response()
}

// ── Auxiliares ───────────────────────────────────────────────────────────

async fn listar(e: &Estado, c: &Collection, page: u32, filtro: &str) -> Result<Lista, Erro> {
    let q = Consulta {
        filtro: (!filtro.trim().is_empty()).then(|| filtro.to_string()),
        pagina: page.max(1),
        por_pagina: 30,
        ..Default::default()
    };
    e.banco.listar(c, &q).await
}

fn html(m: Markup) -> Response {
    Html(m.into_string()).into_response()
}

fn pagina_erro(erro: Erro) -> Response {
    if let Erro::Interno(m) = &erro {
        eprintln!("painel: {m}");
    }
    (
        StatusCode::from_u16(erro.codigo_http()).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR),
        html(admin::pagina(
            "Erro",
            admin::nao_encontrado(&erro.to_string()),
        )),
    )
        .into_response()
}

fn separar_erro(erro: Erro) -> (Map<String, Value>, Option<String>) {
    match erro {
        Erro::Invalido { campos, .. } if !campos.is_empty() => (campos, None),
        Erro::Invalido { mensagem, .. } => (Map::new(), Some(mensagem)),
        Erro::Interno(m) => {
            eprintln!("painel: {m}");
            (Map::new(), Some("Erro interno ao salvar.".into()))
        }
        outro => (Map::new(), Some(outro.to_string())),
    }
}

fn pares(corpo: &[u8]) -> Vec<(String, String)> {
    form_urlencoded::parse(corpo).into_owned().collect()
}

/// Traduz o formulário para o JSON que o núcleo espera, campo a campo.
fn dados_do_form(c: &Collection, pares: &[(String, String)]) -> Map<String, Value> {
    let mut dados = Map::new();
    for f in &c.fields {
        if f.system && f.name == "tokenKey" {
            continue;
        }
        if f.hidden && f.kind != FieldType::Password {
            continue;
        }
        let valores: Vec<&str> = pares
            .iter()
            .filter(|(k, _)| k == &f.name)
            .map(|(_, v)| v.as_str())
            .collect();
        if f.kind == FieldType::Password {
            // Senha em branco no formulário = manter a atual.
            if let Some(v) = valores.first().filter(|v| !v.is_empty()) {
                dados.insert(f.name.clone(), json!(v));
            }
            continue;
        }
        let Some(primeiro) = valores.first() else {
            continue;
        };
        let valor = match f.kind {
            FieldType::Bool => json!(*primeiro == "true"),
            FieldType::Number => primeiro
                .parse::<f64>()
                .map(|n| json!(n))
                .unwrap_or_else(|_| json!(primeiro)),
            FieldType::Json => {
                serde_json::from_str::<Value>(primeiro).unwrap_or_else(|_| json!(primeiro))
            }
            FieldType::Select if f.multiplo() => json!(valores),
            FieldType::Relation if f.multiplo() => {
                let texto = primeiro.trim();
                if texto.is_empty() {
                    json!([])
                } else if let Ok(Value::Array(a)) = serde_json::from_str::<Value>(texto) {
                    Value::Array(a)
                } else {
                    json!(
                        texto
                            .split(',')
                            .map(str::trim)
                            .filter(|s| !s.is_empty())
                            .collect::<Vec<_>>()
                    )
                }
            }
            FieldType::Relation => json!(primeiro.trim()),
            _ => json!(primeiro),
        };
        dados.insert(f.name.clone(), valor);
    }
    dados
}
