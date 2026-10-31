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
use forja_core::{Collection, CollectionType, Erro, Field, FieldType};
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
        .route("/_/htmx/campo-linha", get(htmx_campo_linha))
        .route(
            "/_/new-collection",
            get(nova_colecao_form).post(nova_colecao_criar),
        )
        .route(
            "/_/collections/{nome}/settings",
            get(colecao_settings_form).post(colecao_settings_salvar),
        )
        .route(
            "/_/collections/{nome}/delete-collection",
            post(colecao_apagar),
        )
        .route("/_/collections/{nome}", get(colecao_pagina).post(criar))
        .route("/_/collections/{nome}/new", get(novo_form))
        .route("/_/collections/{nome}/tabela", get(tabela))
        .route(
            "/_/collections/{nome}/records/{id}",
            get(editar_form).post(atualizar),
        )
        .route("/_/collections/{nome}/records/{id}/delete", post(apagar))
        .route("/_/logs", get(logs_pagina))
        .route("/_/crons", get(crons_pagina))
        .route("/_/crons/{id}/run", post(cron_executar_painel))
        .route("/_/backups", get(backups_pagina))
        .route("/_/backups/new", post(backup_novo_painel))
        .route("/_/backups/{key}/delete", post(backup_apagar_painel))
        .route("/_/backups/{key}/restore", post(backup_restaurar_painel))
        .route(
            "/_/settings",
            get(settings_pagina).post(settings_salvar_painel),
        )
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

async fn htmx_campo_linha(State(e): State<Estado>, headers: HeaderMap) -> Response {
    if sessao(&e, &headers).await.is_none() {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    Html(admin::linha_campo().into_string()).into_response()
}

async fn nova_colecao_form(State(e): State<Estado>, headers: HeaderMap) -> Response {
    if sessao(&e, &headers).await.is_none() {
        return entrar();
    }
    html(admin::pagina(
        "Nova Coleção",
        admin::pagina_nova_colecao(None),
    ))
}

async fn nova_colecao_criar(State(e): State<Estado>, headers: HeaderMap, corpo: Bytes) -> Response {
    if sessao(&e, &headers).await.is_none() {
        return entrar();
    }
    let p = pares(&corpo);
    let nome = p
        .iter()
        .find(|(k, _)| k == "name")
        .map(|(_, v)| v.trim())
        .unwrap_or("");
    if nome.is_empty() {
        return html(admin::pagina(
            "Nova Coleção",
            admin::pagina_nova_colecao(Some("O nome da coleção é obrigatório.")),
        ));
    }
    let tipo_str = p
        .iter()
        .find(|(k, _)| k == "type")
        .map(|(_, v)| v.trim())
        .unwrap_or("base");
    let kind = match tipo_str {
        "auth" => CollectionType::Auth,
        "view" => CollectionType::View,
        _ => CollectionType::Base,
    };
    let view_query = p
        .iter()
        .find(|(k, _)| k == "view_query")
        .map(|(_, v)| v.trim().to_string())
        .filter(|s| !s.is_empty());

    let list_rule = regra_do_form(
        p.iter()
            .find(|(k, _)| k == "list_rule_tipo")
            .map(|(_, v)| v.as_str()),
        p.iter()
            .find(|(k, _)| k == "list_rule_expr")
            .map(|(_, v)| v.as_str()),
    );
    let view_rule = regra_do_form(
        p.iter()
            .find(|(k, _)| k == "view_rule_tipo")
            .map(|(_, v)| v.as_str()),
        p.iter()
            .find(|(k, _)| k == "view_rule_expr")
            .map(|(_, v)| v.as_str()),
    );
    let create_rule = if kind == CollectionType::View {
        None
    } else {
        regra_do_form(
            p.iter()
                .find(|(k, _)| k == "create_rule_tipo")
                .map(|(_, v)| v.as_str()),
            p.iter()
                .find(|(k, _)| k == "create_rule_expr")
                .map(|(_, v)| v.as_str()),
        )
    };
    let update_rule = if kind == CollectionType::View {
        None
    } else {
        regra_do_form(
            p.iter()
                .find(|(k, _)| k == "update_rule_tipo")
                .map(|(_, v)| v.as_str()),
            p.iter()
                .find(|(k, _)| k == "update_rule_expr")
                .map(|(_, v)| v.as_str()),
        )
    };
    let delete_rule = if kind == CollectionType::View {
        None
    } else {
        regra_do_form(
            p.iter()
                .find(|(k, _)| k == "delete_rule_tipo")
                .map(|(_, v)| v.as_str()),
            p.iter()
                .find(|(k, _)| k == "delete_rule_expr")
                .map(|(_, v)| v.as_str()),
        )
    };

    let fields = if kind == CollectionType::View {
        Vec::new()
    } else {
        campos_do_form(&p)
    };

    let col = Collection {
        id: String::new(),
        name: nome.to_string(),
        kind,
        system: false,
        fields,
        list_rule,
        view_rule,
        create_rule,
        update_rule,
        delete_rule,
        indexes: Vec::new(),
        view_query,
        created: String::new(),
        updated: String::new(),
        ..Default::default()
    };

    match e.banco.criar_colecao(col).await {
        Ok(criada) => Redirect::to(&format!("/_/collections/{}", criada.name)).into_response(),
        Err(erro) => html(admin::pagina(
            "Nova Coleção",
            admin::pagina_nova_colecao(Some(&erro.to_string())),
        )),
    }
}

async fn colecao_settings_form(
    State(e): State<Estado>,
    headers: HeaderMap,
    Path(nome): Path<String>,
) -> Response {
    if sessao(&e, &headers).await.is_none() {
        return entrar();
    }
    let Ok(c) = e.banco.colecao(&nome).await else {
        return pagina_erro(Erro::NaoEncontrado(format!(
            "A coleção '{nome}' não existe."
        )));
    };
    html(admin::pagina(
        &format!("Configurações: {}", c.name),
        admin::pagina_editar_colecao(&c, None),
    ))
}

async fn colecao_settings_salvar(
    State(e): State<Estado>,
    headers: HeaderMap,
    Path(nome): Path<String>,
    corpo: Bytes,
) -> Response {
    if sessao(&e, &headers).await.is_none() {
        return entrar();
    }
    let Ok(antiga) = e.banco.colecao(&nome).await else {
        return pagina_erro(Erro::NaoEncontrado(format!(
            "A coleção '{nome}' não existe."
        )));
    };
    let p = pares(&corpo);
    let novo_nome = if antiga.system {
        antiga.name.clone()
    } else {
        p.iter()
            .find(|(k, _)| k == "name")
            .map(|(_, v)| v.trim())
            .filter(|s| !s.is_empty())
            .unwrap_or(&antiga.name)
            .to_string()
    };

    let view_query = if antiga.kind == CollectionType::View {
        p.iter()
            .find(|(k, _)| k == "view_query")
            .map(|(_, v)| v.trim().to_string())
            .filter(|s| !s.is_empty())
    } else {
        None
    };

    let list_rule = regra_do_form(
        p.iter()
            .find(|(k, _)| k == "list_rule_tipo")
            .map(|(_, v)| v.as_str()),
        p.iter()
            .find(|(k, _)| k == "list_rule_expr")
            .map(|(_, v)| v.as_str()),
    );
    let view_rule = regra_do_form(
        p.iter()
            .find(|(k, _)| k == "view_rule_tipo")
            .map(|(_, v)| v.as_str()),
        p.iter()
            .find(|(k, _)| k == "view_rule_expr")
            .map(|(_, v)| v.as_str()),
    );
    let create_rule = if antiga.kind == CollectionType::View {
        None
    } else {
        regra_do_form(
            p.iter()
                .find(|(k, _)| k == "create_rule_tipo")
                .map(|(_, v)| v.as_str()),
            p.iter()
                .find(|(k, _)| k == "create_rule_expr")
                .map(|(_, v)| v.as_str()),
        )
    };
    let update_rule = if antiga.kind == CollectionType::View {
        None
    } else {
        regra_do_form(
            p.iter()
                .find(|(k, _)| k == "update_rule_tipo")
                .map(|(_, v)| v.as_str()),
            p.iter()
                .find(|(k, _)| k == "update_rule_expr")
                .map(|(_, v)| v.as_str()),
        )
    };
    let delete_rule = if antiga.kind == CollectionType::View {
        None
    } else {
        regra_do_form(
            p.iter()
                .find(|(k, _)| k == "delete_rule_tipo")
                .map(|(_, v)| v.as_str()),
            p.iter()
                .find(|(k, _)| k == "delete_rule_expr")
                .map(|(_, v)| v.as_str()),
        )
    };

    let mut fields = antiga.fields.clone();
    if antiga.kind != CollectionType::View {
        let novos_campos = campos_do_form(&p);
        for novo_f in novos_campos {
            if !fields
                .iter()
                .any(|f| f.name.eq_ignore_ascii_case(&novo_f.name))
            {
                fields.push(novo_f);
            }
        }
    }

    let mut nova = antiga.clone();
    nova.name = novo_nome;
    nova.view_query = view_query;
    nova.list_rule = list_rule;
    nova.view_rule = view_rule;
    nova.create_rule = create_rule;
    nova.update_rule = update_rule;
    nova.delete_rule = delete_rule;
    nova.fields = fields;

    if antiga.kind == CollectionType::Auth {
        let mut otp = antiga.otp_config();
        otp.enabled = p
            .iter()
            .any(|(k, v)| k == "otp_enabled" && (v == "true" || v == "on" || v == "1"));
        if let Some((_, v)) = p.iter().find(|(k, _)| k == "otp_duration")
            && let Ok(dur) = v.parse::<i64>()
        {
            otp.duration = dur.clamp(30, 86400);
        }
        if let Some((_, v)) = p.iter().find(|(k, _)| k == "otp_length")
            && let Ok(len) = v.parse::<usize>()
        {
            otp.length = len.clamp(4, 10);
        }
        nova.otp = Some(otp);

        let mut mfa = antiga.mfa_config();
        mfa.enabled = p
            .iter()
            .any(|(k, v)| k == "mfa_enabled" && (v == "true" || v == "on" || v == "1"));
        if let Some((_, v)) = p.iter().find(|(k, _)| k == "mfa_duration")
            && let Ok(dur) = v.parse::<i64>()
        {
            mfa.duration = dur.clamp(60, 86400);
        }
        if let Some((_, v)) = p.iter().find(|(k, _)| k == "mfa_rule") {
            mfa.rule = v.trim().to_string();
        }
        nova.mfa = Some(mfa);

        let mut oauth2 = antiga.oauth2_config();
        oauth2.enabled = p
            .iter()
            .any(|(k, v)| k == "oauth2_enabled" && (v == "true" || v == "on" || v == "1"));
        nova.oauth2 = Some(oauth2);
    }

    match e.banco.atualizar_colecao(&antiga.name, nova).await {
        Ok(atualizada) => {
            Redirect::to(&format!("/_/collections/{}", atualizada.name)).into_response()
        }
        Err(erro) => html(admin::pagina(
            &format!("Configurações: {}", antiga.name),
            admin::pagina_editar_colecao(&antiga, Some(&erro.to_string())),
        )),
    }
}

async fn colecao_apagar(
    State(e): State<Estado>,
    headers: HeaderMap,
    Path(nome): Path<String>,
) -> Response {
    if sessao(&e, &headers).await.is_none() {
        return entrar();
    }
    match e.banco.apagar_colecao(&nome).await {
        Ok(()) => Redirect::to("/_/").into_response(),
        Err(erro) => pagina_erro(erro),
    }
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
    if let Some(resp) = recusar_view(&c) {
        return resp;
    }
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
    if let Some(resp) = recusar_view(&c) {
        return resp;
    }
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
    if let Some(resp) = recusar_view(&c) {
        return resp;
    }
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
    if let Some(resp) = recusar_view(&c) {
        return resp;
    }
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
    if let Some(resp) = recusar_view(&c) {
        return resp;
    }
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
        // O painel é a sessão do superusuário: o filtro pode usar `@collection`.
        superusuario: true,
        ..Default::default()
    };
    e.banco.listar(c, &q).await
}

fn recusar_view(c: &Collection) -> Option<Response> {
    if c.kind == CollectionType::View {
        Some(pagina_erro(Erro::invalido(
            "The collection is not writable.",
        )))
    } else {
        None
    }
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

fn regra_do_form(tipo: Option<&str>, expr: Option<&str>) -> Option<String> {
    let expr_trim = expr.unwrap_or("").trim();
    match tipo.unwrap_or("super") {
        "public" => Some(String::new()),
        "custom" => {
            if expr_trim.is_empty() {
                None
            } else {
                Some(expr_trim.to_string())
            }
        }
        "super" => {
            if !expr_trim.is_empty() {
                Some(expr_trim.to_string())
            } else {
                None
            }
        }
        _ => None,
    }
}

fn campos_do_form(pares: &[(String, String)]) -> Vec<Field> {
    let names: Vec<&str> = pares
        .iter()
        .filter(|(k, _)| k == "field_name")
        .map(|(_, v)| v.trim())
        .collect();
    let types: Vec<&str> = pares
        .iter()
        .filter(|(k, _)| k == "field_type")
        .map(|(_, v)| v.trim())
        .collect();
    let reqs: Vec<&str> = pares
        .iter()
        .filter(|(k, _)| k == "field_required")
        .map(|(_, v)| v.trim())
        .collect();

    let mut campos = Vec::new();
    for (i, name) in names.into_iter().enumerate() {
        if name.is_empty() {
            continue;
        }
        let tipo_str = types.get(i).copied().unwrap_or("text");
        let kind = match tipo_str {
            "number" => FieldType::Number,
            "bool" => FieldType::Bool,
            "editor" => FieldType::Editor,
            "email" => FieldType::Email,
            "url" => FieldType::Url,
            "date" => FieldType::Date,
            "select" => FieldType::Select,
            "file" => FieldType::File,
            "relation" => FieldType::Relation,
            "json" => FieldType::Json,
            "password" => FieldType::Password,
            "geoPoint" => FieldType::GeoPoint,
            _ => FieldType::Text,
        };
        let required = reqs.get(i).map(|s| *s == "true").unwrap_or(false);
        campos.push(Field {
            id: String::new(),
            name: name.to_string(),
            kind,
            required,
            hidden: false,
            system: false,
            options: Map::new(),
        });
    }
    campos
}

// ── Handlers do Marco 0.5: Logs, Crons, Backups, Configurações ───────────

async fn logs_pagina(
    State(e): State<Estado>,
    headers: HeaderMap,
) -> Result<Html<String>, Redirect> {
    if sessao(&e, &headers).await.is_none() {
        return Err(Redirect::to("/_/login"));
    }
    let lista = e
        .banco
        .logs_listar(1, 100, None)
        .await
        .unwrap_or(forja_core::store::PaginaLogs {
            page: 1,
            per_page: 100,
            total_items: 0,
            total_pages: 0,
            items: Vec::new(),
        });
    let stats = e.banco.logs_stats(None).await.unwrap_or_default();
    Ok(Html(admin::pagina_logs(&lista.items, &stats).into_string()))
}

async fn crons_pagina(
    State(e): State<Estado>,
    headers: HeaderMap,
) -> Result<Html<String>, Redirect> {
    if sessao(&e, &headers).await.is_none() {
        return Err(Redirect::to("/_/login"));
    }
    let itens = e.banco.crons_listar().await.unwrap_or_default();
    Ok(Html(admin::pagina_crons(&itens).into_string()))
}

async fn cron_executar_painel(
    State(e): State<Estado>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Redirect, Redirect> {
    if sessao(&e, &headers).await.is_none() {
        return Err(Redirect::to("/_/login"));
    }
    let _ = e.banco.cron_executar(&id).await;
    Ok(Redirect::to("/_/crons"))
}

async fn backups_pagina(
    State(e): State<Estado>,
    headers: HeaderMap,
) -> Result<Html<String>, Redirect> {
    if sessao(&e, &headers).await.is_none() {
        return Err(Redirect::to("/_/login"));
    }
    let itens = e.banco.backups_listar().await.unwrap_or_default();
    Ok(Html(
        admin::pagina_backups(&itens, None, None).into_string(),
    ))
}

async fn backup_novo_painel(
    State(e): State<Estado>,
    headers: HeaderMap,
) -> Result<Redirect, Redirect> {
    if sessao(&e, &headers).await.is_none() {
        return Err(Redirect::to("/_/login"));
    }
    let _ = e.banco.backup_criar(None).await;
    Ok(Redirect::to("/_/backups"))
}

async fn backup_apagar_painel(
    State(e): State<Estado>,
    headers: HeaderMap,
    Path(key): Path<String>,
) -> Result<Redirect, Redirect> {
    if sessao(&e, &headers).await.is_none() {
        return Err(Redirect::to("/_/login"));
    }
    let _ = e.banco.backup_apagar(&key).await;
    Ok(Redirect::to("/_/backups"))
}

async fn backup_restaurar_painel(
    State(e): State<Estado>,
    headers: HeaderMap,
    Path(key): Path<String>,
) -> Result<Redirect, Redirect> {
    if sessao(&e, &headers).await.is_none() {
        return Err(Redirect::to("/_/login"));
    }
    let _ = e.banco.backup_restaurar(&key).await;
    Ok(Redirect::to("/_/backups"))
}

async fn settings_pagina(
    State(e): State<Estado>,
    headers: HeaderMap,
) -> Result<Html<String>, Redirect> {
    if sessao(&e, &headers).await.is_none() {
        return Err(Redirect::to("/_/login"));
    }
    let cfg = e.banco.configuracoes().await;
    Ok(Html(admin::pagina_settings(&cfg, None, None).into_string()))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
#[allow(dead_code)]
struct FormSettings {
    #[serde(default)]
    app_name: String,
    #[serde(default)]
    app_url: String,
    #[serde(default)]
    sender_name: String,
    #[serde(default)]
    sender_address: String,
    #[serde(default)]
    smtp_enabled: Option<String>,
    #[serde(default)]
    smtp_host: String,
    #[serde(default)]
    smtp_port: Option<u16>,
    #[serde(default)]
    smtp_username: String,
    #[serde(default)]
    smtp_password: String,
    #[serde(default)]
    smtp_auth_method: String,
    #[serde(default)]
    smtp_local_name: String,
    #[serde(default)]
    smtp_tls: Option<String>,
    #[serde(default)]
    s3_enabled: Option<String>,
    #[serde(default)]
    s3_bucket: String,
    #[serde(default)]
    s3_region: String,
    #[serde(default)]
    s3_endpoint: String,
    #[serde(default)]
    s3_access_key: String,
    #[serde(default)]
    s3_secret: String,
    #[serde(default)]
    s3_force_path_style: Option<String>,
    #[serde(default)]
    backups_cron: String,
    #[serde(default)]
    backups_cron_max_keep: Option<i64>,
    #[serde(default)]
    logs_max_days: Option<i64>,
}

async fn settings_salvar_painel(
    State(e): State<Estado>,
    headers: HeaderMap,
    Form(form): Form<FormSettings>,
) -> Result<Html<String>, Redirect> {
    if sessao(&e, &headers).await.is_none() {
        return Err(Redirect::to("/_/login"));
    }
    let mut cfg = e.banco.configuracoes().await;

    if !form.app_name.trim().is_empty() {
        cfg.meta.app_name = form.app_name.trim().to_string();
        cfg.app_name = cfg.meta.app_name.clone();
    }
    if !form.app_url.trim().is_empty() {
        cfg.meta.app_url = form.app_url.trim().to_string();
        cfg.app_url = cfg.meta.app_url.clone();
    }
    if !form.sender_name.trim().is_empty() {
        cfg.meta.sender_name = form.sender_name.trim().to_string();
    }
    if !form.sender_address.trim().is_empty() {
        cfg.meta.sender_address = form.sender_address.trim().to_string();
    }

    cfg.smtp.enabled = form.smtp_enabled.as_deref() == Some("true");
    cfg.smtp.host = form.smtp_host.trim().to_string();
    if let Some(port) = form.smtp_port {
        cfg.smtp.port = port;
    }
    cfg.smtp.username = form.smtp_username.trim().to_string();
    if !form.smtp_password.is_empty() {
        cfg.smtp.password = form.smtp_password;
    }
    cfg.smtp.auth_method = form.smtp_auth_method.trim().to_string();
    cfg.smtp.local_name = form.smtp_local_name.trim().to_string();
    cfg.smtp.tls = form.smtp_tls.as_deref() == Some("true");

    cfg.s3.enabled = form.s3_enabled.as_deref() == Some("true");
    cfg.s3.bucket = form.s3_bucket.trim().to_string();
    cfg.s3.region = form.s3_region.trim().to_string();
    cfg.s3.endpoint = form.s3_endpoint.trim().to_string();
    cfg.s3.access_key = form.s3_access_key.trim().to_string();
    if !form.s3_secret.is_empty() {
        cfg.s3.secret = form.s3_secret;
    }
    cfg.s3.force_path_style = form.s3_force_path_style.as_deref() == Some("true");

    cfg.backups.cron = form.backups_cron.trim().to_string();
    if let Some(k) = form.backups_cron_max_keep {
        cfg.backups.cron_max_keep = k.max(1);
    }
    if let Some(d) = form.logs_max_days {
        cfg.logs.max_days = d.max(1);
    }

    match e.banco.salvar_configuracoes(&cfg).await {
        Ok(salva) => Ok(Html(
            admin::pagina_settings(&salva, None, Some("Configurações salvas com sucesso!"))
                .into_string(),
        )),
        Err(err) => Ok(Html(
            admin::pagina_settings(&cfg, Some(&err.to_string()), None).into_string(),
        )),
    }
}
