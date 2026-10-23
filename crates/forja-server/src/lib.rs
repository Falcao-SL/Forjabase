//! Servidor HTTP do Forja (axum). As rotas, os corpos e os erros seguem o
//! formato da API do PocketBase, para os SDKs existentes funcionarem.

mod painel;

use axum::extract::{FromRequestParts, Path, Query, State};
use axum::http::{StatusCode, request::Parts};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use forja_core::auth::{
    self, COLECAO_SUPERUSERS, TIPO_AUTH, TIPO_REDEFINICAO, TIPO_TROCA_EMAIL, TIPO_VERIFICACAO,
    VALIDADE_AUTH, VALIDADE_EMAIL, VALIDADE_SUPERUSER,
};
use forja_core::correio::{Console, Correio, Mensagem};
use forja_core::filtro::Contexto;
use forja_core::store::{Banco, Consulta, Credenciais, Motor, Superusuario};
use forja_core::{Collection, CollectionType, Erro, Field};
use serde::Deserialize;
use serde_json::{Map, Value, json};
use std::net::SocketAddr;
use std::sync::Arc;

/// Estado compartilhado: o banco do projeto, o segredo dos tokens e o correio.
#[derive(Clone)]
pub struct Estado {
    pub banco: Banco,
    pub segredo: Arc<String>,
    pub correio: Arc<dyn Correio>,
}

impl Estado {
    pub async fn novo(banco: Banco) -> Result<Estado, Erro> {
        Estado::com_correio(banco, Arc::new(Console)).await
    }

    /// Com um correio próprio (nos testes, o `Memoria`).
    pub async fn com_correio(banco: Banco, correio: Arc<dyn Correio>) -> Result<Estado, Erro> {
        let segredo = banco.segredo().await?;
        Ok(Estado {
            banco,
            segredo: Arc::new(segredo),
            correio,
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

// ── Autenticação ─────────────────────────────────────────────────────────

/// Quem está chamando.
pub enum Identidade {
    Superusuario(Superusuario),
    Registro {
        colecao: Collection,
        registro: Value,
    },
}

pub struct Chamador(pub Option<Identidade>);

impl Chamador {
    fn superusuario(&self) -> bool {
        matches!(self.0, Some(Identidade::Superusuario(_)))
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

    /// O registro autenticado, quando é usuário de uma coleção `auth`.
    fn registro(&self) -> Option<(&Collection, &Value)> {
        match &self.0 {
            Some(Identidade::Registro { colecao, registro }) => Some((colecao, registro)),
            _ => None,
        }
    }

    /// O que as macros `@request.auth.*` enxergam.
    fn contexto(&self) -> Contexto {
        match self.registro() {
            Some((_, registro)) => Contexto {
                auth: registro.as_object().cloned(),
            },
            None => Contexto::default(),
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
        if claims.tipo != TIPO_AUTH {
            return Err(token_invalido());
        }
        if claims.colecao == COLECAO_SUPERUSERS {
            let su = e
                .banco
                .superuser(&claims.id)
                .await
                .map_err(|_| token_invalido())?;
            if su.token_key != claims.tk {
                return Err(token_invalido());
            }
            return Ok(Chamador(Some(Identidade::Superusuario(su))));
        }
        // Usuário de uma coleção `auth`: o token vale enquanto a `tokenKey` bater.
        let colecao = e
            .banco
            .colecao(&claims.colecao)
            .await
            .map_err(|_| token_invalido())?;
        if colecao.kind != CollectionType::Auth {
            return Err(token_invalido());
        }
        let credenciais = e
            .banco
            .auth_credenciais_por_id(&colecao, &claims.id)
            .await
            .map_err(|_| token_invalido())?
            .ok_or_else(token_invalido)?;
        if credenciais.token_key != claims.tk {
            return Err(token_invalido());
        }
        let registro = e
            .banco
            .registro(&colecao, &claims.id, None)
            .await
            .map_err(|_| token_invalido())?;
        Ok(Chamador(Some(Identidade::Registro { colecao, registro })))
    }
}

#[derive(Deserialize)]
struct Login {
    identity: String,
    password: String,
}

async fn superuser_login(State(e): State<Estado>, Json(l): Json<Login>) -> Res<Json<Value>> {
    let falha = || ErroApi(Erro::invalido("Failed to authenticate."));
    let su = e
        .banco
        .superuser_por_email(&l.identity)
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
        Some(Identidade::Superusuario(su)) => resposta_auth(&e, su),
        _ => Err(ErroApi(Erro::NaoAutorizado(
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
    let itens = e.banco.colecoes().await?;
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
    Ok(Json(e.banco.criar_colecao(c).await?))
}

async fn colecoes_ver(
    State(e): State<Estado>,
    quem: Chamador,
    Path(nome): Path<String>,
) -> Res<Json<Collection>> {
    quem.exigir_superusuario()?;
    Ok(Json(e.banco.colecao(&nome).await?))
}

async fn colecoes_apagar(
    State(e): State<Estado>,
    quem: Chamador,
    Path(nome): Path<String>,
) -> Res<StatusCode> {
    quem.exigir_superusuario()?;
    e.banco.apagar_colecao(&nome).await?;
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

/// Campos `hidden` só aparecem para superusuários. E-mail de coleção `auth`
/// só aparece para o próprio dono, para superusuários, ou se `emailVisibility`.
fn esconder(c: &Collection, quem: &Chamador, mut v: Value) -> Value {
    if quem.superusuario() {
        return v;
    }
    let dono = match (quem.registro(), v.get("id").and_then(Value::as_str)) {
        (Some((colecao, registro)), Some(id)) => {
            colecao.id == c.id && registro.get("id").and_then(Value::as_str) == Some(id)
        }
        _ => false,
    };
    let email_publico = v
        .get("emailVisibility")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    if let Some(m) = v.as_object_mut() {
        for f in c.fields.iter().filter(|f: &&Field| f.hidden) {
            m.remove(&f.name);
        }
        if c.kind == CollectionType::Auth && !dono && !email_publico {
            m.remove("email");
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
    Ok(e.banco.colecao(&nome).await?)
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
        contexto: quem.contexto(),
    };
    let mut lista = e.banco.listar(&c, &q).await?;
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
    let r = e
        .banco
        .registro_com(&c, &id, regra.as_deref(), &quem.contexto())
        .await?;
    Ok(Json(esconder(&c, &quem, r)))
}

async fn registros_criar(
    State(e): State<Estado>,
    quem: Chamador,
    Path(nome): Path<String>,
    Json(mut dados): Json<Map<String, Value>>,
) -> Res<Json<Value>> {
    let c = colecao_de(&e, nome).await?;
    // `verified` é de sistema: só superusuário marca e-mail como verificado.
    if c.kind == CollectionType::Auth && !quem.superusuario() {
        dados.remove("verified");
    }
    let regra = regra_para(&quem, &c.create_rule)?;
    let r = e.banco.criar(&c, &dados).await?;
    // A regra de criação é conferida no registro criado; se não passar, ele sai.
    if let Some(regra) = regra {
        let id = r["id"].as_str().unwrap_or_default();
        if e.banco
            .registro_com(&c, id, Some(&regra), &quem.contexto())
            .await
            .is_err()
        {
            e.banco.apagar(&c, id, None).await?;
            return Err(ErroApi(Erro::invalido("Failed to create record.")));
        }
    }
    Ok(Json(esconder(&c, &quem, r)))
}

async fn registros_atualizar(
    State(e): State<Estado>,
    quem: Chamador,
    Path((nome, id)): Path<(String, String)>,
    Json(mut dados): Json<Map<String, Value>>,
) -> Res<Json<Value>> {
    let c = colecao_de(&e, nome).await?;
    let regra = regra_para(&quem, &c.update_rule)?;
    // Campos de sistema de coleções `auth`.
    if c.kind == CollectionType::Auth && !quem.superusuario() {
        dados.remove("verified");
        // Trocar o e-mail só pelo fluxo com token; aqui seria troca silenciosa.
        if let Some(novo) = dados.get("email").and_then(Value::as_str) {
            let atual = e.banco.registro(&c, &id, None).await?;
            let atual = atual
                .get("email")
                .and_then(Value::as_str)
                .unwrap_or_default();
            if !novo.trim().eq_ignore_ascii_case(atual) {
                return Err(ErroApi(Erro::Proibido(
                    "The email can be changed only by superusers or via the email change flow."
                        .into(),
                )));
            }
            dados.remove("email");
        }
    }
    let r = e
        .banco
        .atualizar_com(&c, &id, &dados, regra.as_deref(), &quem.contexto())
        .await?;
    Ok(Json(esconder(&c, &quem, r)))
}

async fn registros_apagar(
    State(e): State<Estado>,
    quem: Chamador,
    Path((nome, id)): Path<(String, String)>,
) -> Res<StatusCode> {
    let c = colecao_de(&e, nome).await?;
    let regra = regra_para(&quem, &c.delete_rule)?;
    e.banco
        .apagar_com(&c, &id, regra.as_deref(), &quem.contexto())
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

// ── Autenticação de usuários (coleções auth) ─────────────────────────────

async fn colecao_auth(e: &Estado, nome: String) -> Res<Collection> {
    let c = colecao_de(e, nome).await?;
    if c.kind != CollectionType::Auth {
        return Err(ErroApi(Erro::invalido("Not an auth collection.")));
    }
    Ok(c)
}

fn resposta_registro(
    e: &Estado,
    c: &Collection,
    cred: &Credenciais,
    registro: Value,
) -> Res<Json<Value>> {
    let token = auth::emitir_tipo(
        &e.segredo,
        &cred.id,
        &c.id,
        &cred.token_key,
        TIPO_AUTH,
        VALIDADE_AUTH,
        None,
    )?;
    Ok(Json(json!({ "token": token, "record": registro })))
}

/// Link que vai no e-mail. Sem o endereço público do app nas configurações
/// ainda, é relativo — o cliente junta com a URL base.
fn link(colecao: &str, acao: &str, token: &str) -> String {
    format!("/api/collections/{colecao}/{acao}?token={token}")
}

fn enviar(e: &Estado, para: &str, assunto: &str, corpo: String) {
    e.correio.enviar(&Mensagem {
        para: para.into(),
        assunto: assunto.into(),
        corpo,
    });
}

fn email_valido(s: &str) -> bool {
    let s = s.trim();
    s.split_once('@').is_some_and(|(a, d)| {
        !a.is_empty() && d.contains('.') && !d.starts_with('.') && !d.ends_with('.')
    }) && !s.contains(char::is_whitespace)
        && s.matches('@').count() == 1
}

fn nao_autorizado() -> ErroApi {
    ErroApi(Erro::NaoAutorizado(
        "The request requires valid record authorization token.".into(),
    ))
}

async fn credenciais_de(e: &Estado, c: &Collection, id: &str) -> Res<Credenciais> {
    e.banco
        .auth_credenciais_por_id(c, id)
        .await?
        .ok_or_else(|| {
            ErroApi(Erro::NaoEncontrado(
                "The requested resource wasn't found.".into(),
            ))
        })
}

#[derive(Deserialize)]
struct LoginAuth {
    identity: String,
    password: String,
}

async fn auth_com_senha(
    State(e): State<Estado>,
    Path(nome): Path<String>,
    Json(l): Json<LoginAuth>,
) -> Res<Json<Value>> {
    let c = colecao_auth(&e, nome).await?;
    let falha = || ErroApi(Erro::invalido("Failed to authenticate."));
    let cred = e
        .banco
        .auth_credenciais(&c, &l.identity)
        .await
        .map_err(|_| falha())?
        .ok_or_else(falha)?;
    let (hash, senha) = (cred.senha.clone(), l.password);
    let ok = tokio::task::spawn_blocking(move || auth::conferir_senha(&hash, &senha))
        .await
        .unwrap_or(false);
    if !ok {
        return Err(falha());
    }
    let registro = e.banco.registro(&c, &cred.id, None).await?;
    resposta_registro(&e, &c, &cred, registro)
}

async fn auth_refresh(
    State(e): State<Estado>,
    quem: Chamador,
    Path(nome): Path<String>,
) -> Res<Json<Value>> {
    let c = colecao_auth(&e, nome).await?;
    let Some((colecao, registro)) = quem.registro() else {
        return Err(nao_autorizado());
    };
    if colecao.id != c.id {
        return Err(nao_autorizado());
    }
    let id = registro["id"].as_str().unwrap_or_default().to_string();
    let cred = credenciais_de(&e, &c, &id).await?;
    resposta_registro(&e, &c, &cred, registro.clone())
}

#[derive(Deserialize)]
struct SoEmail {
    email: String,
}

async fn pedir_verificacao(
    State(e): State<Estado>,
    Path(nome): Path<String>,
    Json(p): Json<SoEmail>,
) -> Res<StatusCode> {
    let c = colecao_auth(&e, nome).await?;
    if let Some(cred) = e.banco.auth_credenciais(&c, &p.email).await?
        && !cred.verified
    {
        let token = auth::emitir_tipo(
            &e.segredo,
            &cred.id,
            &c.id,
            &cred.token_key,
            TIPO_VERIFICACAO,
            VALIDADE_EMAIL,
            None,
        )?;
        enviar(
            &e,
            &cred.email,
            "Verifique seu e-mail",
            link(&c.name, "confirm-verification", &token),
        );
    }
    // Sempre 204: não se revela se o e-mail existe.
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
struct ComToken {
    token: String,
}

async fn confirmar_verificacao(
    State(e): State<Estado>,
    Path(nome): Path<String>,
    Json(p): Json<ComToken>,
) -> Res<StatusCode> {
    let c = colecao_auth(&e, nome).await?;
    let claims = auth::validar(&e.segredo, &p.token)?;
    let cred = credenciais_de(&e, &c, &claims.id).await?;
    if claims.tipo != TIPO_VERIFICACAO || claims.colecao != c.id || claims.tk != cred.token_key {
        return Err(ErroApi(Erro::invalido("Invalid verification token.")));
    }
    e.banco.auth_verificar(&c, &cred.id, None).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn pedir_redefinicao(
    State(e): State<Estado>,
    Path(nome): Path<String>,
    Json(p): Json<SoEmail>,
) -> Res<StatusCode> {
    let c = colecao_auth(&e, nome).await?;
    if let Some(cred) = e.banco.auth_credenciais(&c, &p.email).await? {
        let token = auth::emitir_tipo(
            &e.segredo,
            &cred.id,
            &c.id,
            &cred.token_key,
            TIPO_REDEFINICAO,
            VALIDADE_EMAIL,
            None,
        )?;
        enviar(
            &e,
            &cred.email,
            "Redefinição de senha",
            link(&c.name, "confirm-password-reset", &token),
        );
    }
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct NovaSenha {
    token: String,
    password: String,
    password_confirm: Option<String>,
}

async fn confirmar_redefinicao(
    State(e): State<Estado>,
    Path(nome): Path<String>,
    Json(p): Json<NovaSenha>,
) -> Res<StatusCode> {
    let c = colecao_auth(&e, nome).await?;
    let claims = auth::validar(&e.segredo, &p.token)?;
    let cred = credenciais_de(&e, &c, &claims.id).await?;
    if claims.tipo != TIPO_REDEFINICAO || claims.colecao != c.id || claims.tk != cred.token_key {
        return Err(ErroApi(Erro::invalido("Invalid password reset token.")));
    }
    if let Some(confirmacao) = &p.password_confirm
        && confirmacao != &p.password
    {
        return Err(ErroApi(Erro::campo(
            "passwordConfirm",
            "validation_password_mismatch",
            "Passwords do not match.",
        )));
    }
    let mut dados = Map::new();
    dados.insert("password".into(), json!(p.password));
    // A troca de senha também rotaciona a `tokenKey` e invalida os tokens.
    e.banco.atualizar(&c, &cred.id, &dados, None).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct TrocaEmail {
    new_email: String,
}

async fn pedir_troca_email(
    State(e): State<Estado>,
    quem: Chamador,
    Path(nome): Path<String>,
    Json(p): Json<TrocaEmail>,
) -> Res<StatusCode> {
    let c = colecao_auth(&e, nome).await?;
    let Some((colecao, _)) = quem.registro() else {
        return Err(nao_autorizado());
    };
    if colecao.id != c.id {
        return Err(nao_autorizado());
    }
    let novo = p.new_email.trim().to_string();
    if !email_valido(&novo) {
        return Err(ErroApi(Erro::campo(
            "newEmail",
            "validation_is_email",
            "Must be a valid email address.",
        )));
    }
    let id = quem
        .registro()
        .map(|(_, r)| r["id"].as_str().unwrap_or_default())
        .unwrap_or_default();
    let cred = credenciais_de(&e, &c, id).await?;
    if let Some(outro) = e.banco.auth_credenciais(&c, &novo).await?
        && outro.id != cred.id
    {
        return Err(ErroApi(Erro::campo(
            "newEmail",
            "validation_not_unique",
            "The email is invalid or already in use.",
        )));
    }
    let token = auth::emitir_tipo(
        &e.segredo,
        &cred.id,
        &c.id,
        &cred.token_key,
        TIPO_TROCA_EMAIL,
        VALIDADE_EMAIL,
        Some(novo.clone()),
    )?;
    enviar(
        &e,
        &novo,
        "Confirme seu novo e-mail",
        link(&c.name, "confirm-email-change", &token),
    );
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
struct ConfirmarTrocaEmail {
    token: String,
    password: String,
}

async fn confirmar_troca_email(
    State(e): State<Estado>,
    Path(nome): Path<String>,
    Json(p): Json<ConfirmarTrocaEmail>,
) -> Res<StatusCode> {
    let c = colecao_auth(&e, nome).await?;
    let claims = auth::validar(&e.segredo, &p.token)?;
    let cred = credenciais_de(&e, &c, &claims.id).await?;
    if claims.tipo != TIPO_TROCA_EMAIL || claims.colecao != c.id || claims.tk != cred.token_key {
        return Err(ErroApi(Erro::invalido("Invalid email change token.")));
    }
    let (hash, senha) = (cred.senha.clone(), p.password);
    let ok = tokio::task::spawn_blocking(move || auth::conferir_senha(&hash, &senha))
        .await
        .unwrap_or(false);
    if !ok {
        return Err(ErroApi(Erro::invalido("Failed to authenticate.")));
    }
    let novo = claims
        .email
        .ok_or_else(|| ErroApi(Erro::invalido("Invalid email change token.")))?;
    if let Some(outro) = e.banco.auth_credenciais(&c, &novo).await?
        && outro.id != cred.id
    {
        return Err(ErroApi(Erro::campo(
            "email",
            "validation_not_unique",
            "The email is invalid or already in use.",
        )));
    }
    e.banco.auth_verificar(&c, &cred.id, Some(&novo)).await?;
    // Rotaciona a `tokenKey` sem mudar o hash da senha.
    e.banco
        .auth_definir_senha(&c, &cred.id, &cred.senha)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
struct Personificar {
    duration: Option<u64>,
}

async fn personificar(
    State(e): State<Estado>,
    quem: Chamador,
    Path((nome, id)): Path<(String, String)>,
    Json(p): Json<Personificar>,
) -> Res<Json<Value>> {
    quem.exigir_superusuario()?;
    let c = colecao_auth(&e, nome).await?;
    let cred = credenciais_de(&e, &c, &id).await?;
    let validade = p.duration.unwrap_or(3_600).clamp(1, 604_800);
    let token = auth::emitir_tipo(
        &e.segredo,
        &cred.id,
        &c.id,
        &cred.token_key,
        TIPO_AUTH,
        validade,
        None,
    )?;
    let registro = e.banco.registro(&c, &id, None).await?;
    Ok(Json(json!({ "token": token, "record": registro })))
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
        .route(
            "/api/collections/{colecao}/auth-with-password",
            post(auth_com_senha),
        )
        .route(
            "/api/collections/{colecao}/auth-refresh",
            post(auth_refresh),
        )
        .route(
            "/api/collections/{colecao}/request-verification",
            post(pedir_verificacao),
        )
        .route(
            "/api/collections/{colecao}/confirm-verification",
            post(confirmar_verificacao),
        )
        .route(
            "/api/collections/{colecao}/request-password-reset",
            post(pedir_redefinicao),
        )
        .route(
            "/api/collections/{colecao}/confirm-password-reset",
            post(confirmar_redefinicao),
        )
        .route(
            "/api/collections/{colecao}/request-email-change",
            post(pedir_troca_email),
        )
        .route(
            "/api/collections/{colecao}/confirm-email-change",
            post(confirmar_troca_email),
        )
        .route(
            "/api/collections/{colecao}/impersonate/{id}",
            post(personificar),
        )
        .merge(painel::rotas())
        .with_state(e)
}

/// Abre o projeto padrão no motor escolhido e atende até o processo terminar.
pub async fn serve(
    addr: SocketAddr,
    pasta: std::path::PathBuf,
    motor: Motor,
) -> std::io::Result<()> {
    let io = |e: Erro| std::io::Error::other(e.to_string());
    let banco = Banco::abrir(&motor, &pasta, "default").await.map_err(io)?;
    let nome_motor = banco.motor();
    let estado = Estado::novo(banco).await.map_err(io)?;
    if estado.banco.total_superusers().await.unwrap_or(0) == 0 {
        eprintln!(
            "Nenhum superusuário ainda. Crie um com:\n  forja superuser upsert <email> <senha> --dir {}",
            pasta.display()
        );
    }
    let listener = tokio::net::TcpListener::bind(addr).await?;
    eprintln!(
        "Forja {} em http://{addr} (motor {nome_motor}, dados em {})",
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

    async fn estado() -> Estado {
        Estado::novo(Banco::sqlite_memoria().await.unwrap())
            .await
            .unwrap()
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
            .await
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
        let (s, v) = chamar(&estado().await, "GET", "/api/health", None, None).await;
        assert_eq!(s, StatusCode::OK);
        assert_eq!(
            v,
            json!({ "code": 200, "message": "API is healthy.", "data": {} })
        );
    }

    #[tokio::test]
    async fn login_errado_e_token_invalido() {
        let e = estado().await;
        e.banco
            .superuser_salvar("admin@exemplo.com", "senha-forte-123")
            .await
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
        let e = estado().await;
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
            .await
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
        let e = estado().await;
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

    use forja_core::correio::Memoria;

    async fn estado_com_memoria() -> (Estado, Arc<Memoria>) {
        let memoria = Arc::new(Memoria::default());
        let banco = Banco::sqlite_memoria().await.unwrap();
        let e = Estado::com_correio(banco, memoria.clone()).await.unwrap();
        (e, memoria)
    }

    fn token_do_email(memoria: &Memoria) -> String {
        let m = memoria.ultima().expect("nenhum e-mail enviado");
        m.corpo
            .split("token=")
            .nth(1)
            .expect("link sem token")
            .to_string()
    }

    async fn colecao_usuarios(e: &Estado, admin: &str, list_rule: Value) -> Value {
        let (s, v) = chamar(
            e,
            "POST",
            "/api/collections",
            Some(admin),
            Some(json!({
                "name": "usuarios",
                "type": "auth",
                "fields": [{ "name": "nome", "type": "text" }],
                "listRule": list_rule,
                "viewRule": "",
                "createRule": "",
                "updateRule": "id = @request.auth.id",
                "deleteRule": null
            })),
        )
        .await;
        assert_eq!(s, StatusCode::OK, "{v}");
        v
    }

    async fn cadastrar(e: &Estado, email: &str, senha: &str) -> Value {
        let (s, v) = chamar(
            e,
            "POST",
            "/api/collections/usuarios/records",
            None,
            Some(json!({ "email": email, "password": senha, "nome": "Alguém" })),
        )
        .await;
        assert_eq!(s, StatusCode::OK, "{v}");
        v
    }

    #[tokio::test]
    async fn fluxo_completo_de_autenticacao() {
        let (e, memoria) = estado_com_memoria().await;
        let admin = login(&e).await;
        let c = colecao_usuarios(&e, &admin, json!("id = @request.auth.id")).await;
        let sistemas: Vec<_> = c["fields"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|f| f["system"] == json!(true))
            .map(|f| f["name"].as_str().unwrap().to_string())
            .collect();
        assert_eq!(
            sistemas,
            vec![
                "email",
                "emailVisibility",
                "verified",
                "password",
                "tokenKey"
            ]
        );

        let ana = cadastrar(&e, "Ana@Exemplo.com", "senha-bem-longa").await;
        assert!(ana.get("password").is_none(), "{ana}");
        assert!(ana.get("tokenKey").is_none());
        assert_eq!(ana["verified"], json!(false));
        let ana_id = ana["id"].as_str().unwrap().to_string();
        // O visitante que cadastrou não é o dono: o e-mail não volta na resposta.
        assert!(ana.get("email").is_none(), "{ana}");
        let (_, v) = chamar(
            &e,
            "GET",
            &format!("/api/collections/usuarios/records/{ana_id}"),
            Some(&admin),
            None,
        )
        .await;
        assert_eq!(v["email"], json!("Ana@Exemplo.com"));

        // E-mail repetido, sem diferenciar maiúsculas.
        let (s, v) = chamar(
            &e,
            "POST",
            "/api/collections/usuarios/records",
            None,
            Some(json!({ "email": "ANA@exemplo.com", "password": "outra-bem-longa" })),
        )
        .await;
        assert_eq!(s, StatusCode::BAD_REQUEST, "{v}");
        assert_eq!(v["data"]["email"]["code"], json!("validation_not_unique"));

        // Senha curta não passa.
        let (s, v) = chamar(
            &e,
            "POST",
            "/api/collections/usuarios/records",
            None,
            Some(json!({ "email": "curta@exemplo.com", "password": "123" })),
        )
        .await;
        assert_eq!(s, StatusCode::BAD_REQUEST, "{v}");
        assert_eq!(
            v["data"]["password"]["code"],
            json!("validation_min_text_constraint")
        );

        let (s, _) = chamar(
            &e,
            "POST",
            "/api/collections/usuarios/auth-with-password",
            None,
            Some(json!({ "identity": "ana@exemplo.com", "password": "errada" })),
        )
        .await;
        assert_eq!(s, StatusCode::BAD_REQUEST);

        let (s, v) = chamar(
            &e,
            "POST",
            "/api/collections/usuarios/auth-with-password",
            None,
            Some(json!({ "identity": "ana@exemplo.com", "password": "senha-bem-longa" })),
        )
        .await;
        assert_eq!(s, StatusCode::OK, "{v}");
        let token = v["token"].as_str().unwrap().to_string();
        assert_eq!(v["record"]["id"], json!(ana_id));

        // Refresh e a regra `id = @request.auth.id`.
        let auth = format!("Bearer {token}");
        let (s, v) = chamar(
            &e,
            "POST",
            "/api/collections/usuarios/auth-refresh",
            Some(&auth),
            None,
        )
        .await;
        assert_eq!(s, StatusCode::OK, "{v}");
        let (s, v) = chamar(
            &e,
            "GET",
            "/api/collections/usuarios/records",
            Some(&auth),
            None,
        )
        .await;
        assert_eq!(s, StatusCode::OK);
        assert_eq!(v["totalItems"], json!(1), "{v}");

        // Verificação de e-mail.
        let (s, _) = chamar(
            &e,
            "POST",
            "/api/collections/usuarios/request-verification",
            None,
            Some(json!({ "email": "ana@exemplo.com" })),
        )
        .await;
        assert_eq!(s, StatusCode::NO_CONTENT);
        let token_verificacao = token_do_email(&memoria);
        let (s, v) = chamar(
            &e,
            "POST",
            "/api/collections/usuarios/confirm-verification",
            None,
            Some(json!({ "token": token_verificacao })),
        )
        .await;
        assert_eq!(s, StatusCode::NO_CONTENT, "{v}");
        let (_, v) = chamar(
            &e,
            "GET",
            &format!("/api/collections/usuarios/records/{ana_id}"),
            Some(&auth),
            None,
        )
        .await;
        assert_eq!(v["verified"], json!(true));

        // Troca de e-mail.
        memoria.limpar();
        let (s, v) = chamar(
            &e,
            "POST",
            "/api/collections/usuarios/request-email-change",
            Some(&auth),
            Some(json!({ "newEmail": "ana.nova@exemplo.com" })),
        )
        .await;
        assert_eq!(s, StatusCode::NO_CONTENT, "{v}");
        let token_email = token_do_email(&memoria);
        let (s, v) = chamar(
            &e,
            "POST",
            "/api/collections/usuarios/confirm-email-change",
            None,
            Some(json!({ "token": token_email, "password": "senha-bem-longa" })),
        )
        .await;
        assert_eq!(s, StatusCode::NO_CONTENT, "{v}");
        let (s, _) = chamar(
            &e,
            "POST",
            "/api/collections/usuarios/auth-with-password",
            None,
            Some(json!({ "identity": "ana@exemplo.com", "password": "senha-bem-longa" })),
        )
        .await;
        assert_eq!(s, StatusCode::BAD_REQUEST, "o e-mail antigo devia sair");
        let (s, v) = chamar(
            &e,
            "POST",
            "/api/collections/usuarios/auth-with-password",
            None,
            Some(json!({ "identity": "ana.nova@exemplo.com", "password": "senha-bem-longa" })),
        )
        .await;
        assert_eq!(s, StatusCode::OK, "{v}");

        // Redefinição de senha.
        memoria.limpar();
        let (s, _) = chamar(
            &e,
            "POST",
            "/api/collections/usuarios/request-password-reset",
            None,
            Some(json!({ "email": "ana.nova@exemplo.com" })),
        )
        .await;
        assert_eq!(s, StatusCode::NO_CONTENT);
        let token_redefinicao = token_do_email(&memoria);
        let (s, v) = chamar(
            &e,
            "POST",
            "/api/collections/usuarios/confirm-password-reset",
            None,
            Some(json!({
                "token": token_redefinicao,
                "password": "senha-muito-longa",
                "passwordConfirm": "senha-muito-longa"
            })),
        )
        .await;
        assert_eq!(s, StatusCode::NO_CONTENT, "{v}");
        let (s, _) = chamar(
            &e,
            "POST",
            "/api/collections/usuarios/auth-with-password",
            None,
            Some(json!({ "identity": "ana.nova@exemplo.com", "password": "senha-bem-longa" })),
        )
        .await;
        assert_eq!(s, StatusCode::BAD_REQUEST, "a senha antiga devia sair");
        let (s, v) = chamar(
            &e,
            "POST",
            "/api/collections/usuarios/auth-with-password",
            None,
            Some(json!({ "identity": "ana.nova@exemplo.com", "password": "senha-muito-longa" })),
        )
        .await;
        assert_eq!(s, StatusCode::OK, "{v}");

        // O token emitido antes da troca de senha não vale mais.
        let (s, _) = chamar(
            &e,
            "GET",
            "/api/collections/usuarios/records",
            Some(&auth),
            None,
        )
        .await;
        assert_eq!(s, StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn campos_de_sistema_nao_sao_editaveis() {
        let (e, _) = estado_com_memoria().await;
        let admin = login(&e).await;
        colecao_usuarios(&e, &admin, json!("")).await;
        let (s, ana) = chamar(
            &e,
            "POST",
            "/api/collections/usuarios/records",
            None,
            Some(json!({
                "email": "ana@exemplo.com",
                "password": "senha-bem-longa",
                "verified": true
            })),
        )
        .await;
        assert_eq!(s, StatusCode::OK, "{ana}");
        assert_eq!(ana["verified"], json!(false), "{ana}");
        let id = ana["id"].as_str().unwrap().to_string();

        let (_, v) = chamar(
            &e,
            "POST",
            "/api/collections/usuarios/auth-with-password",
            None,
            Some(json!({ "identity": "ana@exemplo.com", "password": "senha-bem-longa" })),
        )
        .await;
        let auth = format!("Bearer {}", v["token"].as_str().unwrap());
        let rota = format!("/api/collections/usuarios/records/{id}");

        // O dono não se marca como verificado...
        let (s, v) = chamar(
            &e,
            "PATCH",
            &rota,
            Some(&auth),
            Some(json!({ "verified": true, "nome": "Ana" })),
        )
        .await;
        assert_eq!(s, StatusCode::OK, "{v}");
        assert_eq!(v["verified"], json!(false));

        // ...nem troca o e-mail sem o fluxo com token.
        let (s, v) = chamar(
            &e,
            "PATCH",
            &rota,
            Some(&auth),
            Some(json!({ "email": "outro@exemplo.com" })),
        )
        .await;
        assert_eq!(s, StatusCode::FORBIDDEN, "{v}");
    }

    #[tokio::test]
    async fn personificar_e_visibilidade_do_email() {
        let (e, _) = estado_com_memoria().await;
        let admin = login(&e).await;
        colecao_usuarios(&e, &admin, json!("")).await;
        let ana = cadastrar(&e, "ana@exemplo.com", "senha-bem-longa").await;
        let ana_id = ana["id"].as_str().unwrap().to_string();
        cadastrar(&e, "bia@exemplo.com", "senha-bem-longa").await;

        // Visitante lista (regra pública) e não vê e-mail.
        let (s, v) = chamar(&e, "GET", "/api/collections/usuarios/records", None, None).await;
        assert_eq!(s, StatusCode::OK, "{v}");
        assert_eq!(v["totalItems"], json!(2));
        assert!(v["items"][0].get("email").is_none(), "{v}");
        assert!(v["items"][0].get("emailVisibility").is_some());
        assert!(v["items"][0].get("verified").is_some());

        // Superusuário pode personificar, e o token resultante é do usuário.
        let (s, v) = chamar(
            &e,
            "POST",
            &format!("/api/collections/usuarios/impersonate/{ana_id}"),
            Some(&admin),
            Some(json!({ "duration": 600 })),
        )
        .await;
        assert_eq!(s, StatusCode::OK, "{v}");
        let token = format!("Bearer {}", v["token"].as_str().unwrap());

        // O dono vê o próprio e-mail, mesmo com `emailVisibility` falso.
        let (s, v) = chamar(
            &e,
            "GET",
            &format!("/api/collections/usuarios/records/{ana_id}"),
            Some(&token),
            None,
        )
        .await;
        assert_eq!(s, StatusCode::OK, "{v}");
        assert_eq!(v["email"], json!("ana@exemplo.com"));

        // Com `emailVisibility` ligado, o visitante vê o e-mail.
        let (s, v) = chamar(
            &e,
            "PATCH",
            &format!("/api/collections/usuarios/records/{ana_id}"),
            Some(&admin),
            Some(json!({ "emailVisibility": true })),
        )
        .await;
        assert_eq!(s, StatusCode::OK, "{v}");
        let (_, v) = chamar(&e, "GET", "/api/collections/usuarios/records", None, None).await;
        let item = v["items"]
            .as_array()
            .unwrap()
            .iter()
            .find(|i| i["id"] == json!(ana_id))
            .unwrap();
        assert_eq!(item["email"], json!("ana@exemplo.com"));

        // Outro usuário não pode se passar por ninguém.
        let (s, _) = chamar(
            &e,
            "POST",
            &format!("/api/collections/usuarios/impersonate/{ana_id}"),
            Some(&token),
            Some(json!({})),
        )
        .await;
        assert_eq!(s, StatusCode::FORBIDDEN);
    }

    // ── Painel ───────────────────────────────────────────────────────────

    fn form(campos: &[(&str, &str)]) -> String {
        campos
            .iter()
            .map(|(k, v)| format!("{}={}", urlenc(k), urlenc(v)))
            .collect::<Vec<_>>()
            .join("&")
    }

    fn urlenc(s: &str) -> String {
        let mut out = String::new();
        for b in s.bytes() {
            match b {
                b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                    out.push(b as char)
                }
                b' ' => out.push('+'),
                _ => out.push_str(&format!("%{b:02X}")),
            }
        }
        out
    }

    async fn pedir(
        e: &Estado,
        metodo: &str,
        uri: &str,
        cabecalhos: &[(&str, &str)],
        corpo: &str,
    ) -> (StatusCode, axum::http::HeaderMap, String) {
        let mut req = Request::builder().method(metodo).uri(uri);
        for (k, v) in cabecalhos {
            req = req.header(*k, *v);
        }
        let req = req.body(Body::from(corpo.to_string())).unwrap();
        let resp = router(e.clone()).oneshot(req).await.unwrap();
        let status = resp.status();
        let headers = resp.headers().clone();
        let bytes = resp.into_body().collect().await.unwrap().to_bytes();
        (
            status,
            headers,
            String::from_utf8_lossy(&bytes).into_owned(),
        )
    }

    async fn form_post(
        e: &Estado,
        uri: &str,
        cookie: Option<&str>,
        campos: &[(&str, &str)],
    ) -> (StatusCode, axum::http::HeaderMap, String) {
        let corpo = form(campos);
        let cab: Vec<(&str, &str)> = match cookie {
            Some(c) => vec![
                ("content-type", "application/x-www-form-urlencoded"),
                ("cookie", c),
            ],
            None => vec![("content-type", "application/x-www-form-urlencoded")],
        };
        pedir(e, "POST", uri, &cab, &corpo).await
    }

    /// Entra no painel e devolve o `Cookie` pronto para as próximas chamadas.
    async fn entrar_no_painel(e: &Estado) -> String {
        let (s, h, corpo) = form_post(
            e,
            "/_/login",
            None,
            &[
                ("identity", "admin@exemplo.com"),
                ("password", "senha-forte-123"),
            ],
        )
        .await;
        assert_eq!(s, StatusCode::SEE_OTHER, "{corpo}");
        let set = h["set-cookie"].to_str().unwrap();
        assert!(set.starts_with("forja_painel="), "{set}");
        assert!(set.contains("HttpOnly") && set.contains("SameSite=Strict"));
        set.split(';').next().unwrap().to_string()
    }

    #[tokio::test]
    async fn painel_exige_login() {
        let e = estado().await;
        login(&e).await;

        // Sem cookie, tudo manda para o login.
        for uri in [
            "/_/",
            "/_/collections/qualquer",
            "/_/collections/qualquer/new",
            "/_/collections/qualquer/tabela",
        ] {
            let (s, h, _) = pedir(&e, "GET", uri, &[], "").await;
            assert_eq!(s, StatusCode::SEE_OTHER, "{uri}");
            assert_eq!(h["location"], "/_/login", "{uri}");
        }

        // Cookie inválido também.
        let (s, h, _) = pedir(&e, "GET", "/_/", &[("cookie", "forja_painel=lixo")], "").await;
        assert_eq!(s, StatusCode::SEE_OTHER);
        assert_eq!(h["location"], "/_/login");

        // A tela de login abre e a senha errada não entra.
        let (s, _, corpo) = pedir(&e, "GET", "/_/login", &[], "").await;
        assert_eq!(s, StatusCode::OK);
        assert!(corpo.contains("Entrar no painel"), "{corpo}");
        let (s, h, corpo) = form_post(
            &e,
            "/_/login",
            None,
            &[("identity", "admin@exemplo.com"), ("password", "errada")],
        )
        .await;
        assert_eq!(s, StatusCode::OK);
        assert!(corpo.contains("E-mail ou senha inválidos."));
        assert!(h.get("set-cookie").is_none());

        // O htmx vem embutido no binário.
        let (s, h, corpo) = pedir(&e, "GET", "/_/htmx.min.js", &[], "").await;
        assert_eq!(s, StatusCode::OK);
        assert!(h["content-type"].to_str().unwrap().contains("javascript"));
        assert!(corpo.starts_with("var htmx"));

        // Depois de entrar, o painel abre.
        let cookie = entrar_no_painel(&e).await;
        let (s, _, corpo) = pedir(&e, "GET", "/_/", &[("cookie", cookie.as_str())], "").await;
        assert_eq!(s, StatusCode::OK);
        assert!(corpo.contains("Coleções"), "{corpo}");
    }

    #[tokio::test]
    async fn painel_lista_e_edita_registros() {
        let e = estado().await;
        let admin = login(&e).await;
        let (s, v) = chamar(
            &e,
            "POST",
            "/api/collections",
            Some(&admin),
            Some(json!({
                "name": "produtos",
                "type": "base",
                "fields": [
                    { "name": "nome", "type": "text", "required": true },
                    { "name": "preco", "type": "number" },
                    { "name": "ativo", "type": "bool" }
                ]
            })),
        )
        .await;
        assert_eq!(s, StatusCode::OK, "{v}");
        let cookie = entrar_no_painel(&e).await;

        // A coleção aparece na home e o formulário tem os campos.
        let (_, _, corpo) = pedir(&e, "GET", "/_/", &[("cookie", cookie.as_str())], "").await;
        assert!(corpo.contains("produtos"), "{corpo}");
        let (s, _, corpo) = pedir(
            &e,
            "GET",
            "/_/collections/produtos/new",
            &[("cookie", cookie.as_str())],
            "",
        )
        .await;
        assert_eq!(s, StatusCode::OK);
        assert!(corpo.contains("name=\"nome\""), "{corpo}");
        assert!(corpo.contains("name=\"ativo\""));

        // Campo obrigatório em branco: o formulário volta com o erro inline.
        let (s, _, corpo) = form_post(
            &e,
            "/_/collections/produtos",
            Some(&cookie),
            &[("ativo", "true")],
        )
        .await;
        assert_eq!(s, StatusCode::OK);
        assert!(corpo.contains("Cannot be blank."), "{corpo}");

        // Cria pelo formulário.
        let (s, h, _) = form_post(
            &e,
            "/_/collections/produtos",
            Some(&cookie),
            &[("nome", "Caneca"), ("preco", "19.5"), ("ativo", "true")],
        )
        .await;
        assert_eq!(s, StatusCode::SEE_OTHER);
        assert_eq!(h["location"], "/_/collections/produtos");

        let (_, _, corpo) = pedir(
            &e,
            "GET",
            "/_/collections/produtos",
            &[("cookie", cookie.as_str())],
            "",
        )
        .await;
        assert!(corpo.contains("Caneca"), "{corpo}");
        assert!(corpo.contains("19.5"));
        assert!(corpo.contains("Sim"));

        // O fragmento do htmx traz só a tabela, com o id do alvo.
        let (s, _, corpo) = pedir(
            &e,
            "GET",
            "/_/collections/produtos/tabela",
            &[("cookie", cookie.as_str())],
            "",
        )
        .await;
        assert_eq!(s, StatusCode::OK);
        assert!(
            corpo.trim_start().starts_with("<div id=\"tabela\">"),
            "{corpo}"
        );
        assert!(!corpo.contains("<html"));

        // Filtra.
        let (_, _, corpo) = pedir(
            &e,
            "GET",
            "/_/collections/produtos/tabela?filter=nome%20~%20%27Nada%27",
            &[("cookie", cookie.as_str())],
            "",
        )
        .await;
        assert!(corpo.contains("Nenhum registro encontrado."), "{corpo}");

        // Descobre o id e edita.
        let (_, v) = chamar(
            &e,
            "GET",
            "/api/collections/produtos/records",
            Some(&admin),
            None,
        )
        .await;
        let id = v["items"][0]["id"].as_str().unwrap().to_string();
        let (s, _, corpo) = pedir(
            &e,
            "GET",
            &format!("/_/collections/produtos/records/{id}"),
            &[("cookie", cookie.as_str())],
            "",
        )
        .await;
        assert_eq!(s, StatusCode::OK);
        assert!(corpo.contains("value=\"Caneca\""), "{corpo}");

        let (s, _, _) = form_post(
            &e,
            &format!("/_/collections/produtos/records/{id}"),
            Some(&cookie),
            &[
                ("nome", "Caneca grande"),
                ("preco", "20"),
                ("ativo", "false"),
            ],
        )
        .await;
        assert_eq!(s, StatusCode::SEE_OTHER);
        let (_, _, corpo) = pedir(
            &e,
            "GET",
            "/_/collections/produtos",
            &[("cookie", cookie.as_str())],
            "",
        )
        .await;
        assert!(corpo.contains("Caneca grande"), "{corpo}");

        // Apagar pelo htmx devolve a tabela atualizada.
        let (s, _, corpo) = pedir(
            &e,
            "POST",
            &format!("/_/collections/produtos/records/{id}/delete"),
            &[("cookie", &cookie), ("hx-request", "true")],
            "",
        )
        .await;
        assert_eq!(s, StatusCode::OK);
        assert!(corpo.contains("Nenhum registro encontrado."), "{corpo}");
        assert!(!corpo.contains("Caneca grande"));

        // Sem htmx, apagar redireciona.
        let (s, _, _) = form_post(
            &e,
            "/_/collections/produtos",
            Some(&cookie),
            &[("nome", "Xícara"), ("ativo", "true")],
        )
        .await;
        assert_eq!(s, StatusCode::SEE_OTHER);
        let (_, v) = chamar(
            &e,
            "GET",
            "/api/collections/produtos/records",
            Some(&admin),
            None,
        )
        .await;
        let id = v["items"][0]["id"].as_str().unwrap().to_string();
        let (s, h, _) = form_post(
            &e,
            &format!("/_/collections/produtos/records/{id}/delete"),
            Some(&cookie),
            &[],
        )
        .await;
        assert_eq!(s, StatusCode::SEE_OTHER);
        assert_eq!(h["location"], "/_/collections/produtos");
    }

    #[tokio::test]
    async fn painel_sai_e_nao_vaza_campos_de_sistema() {
        let e = estado().await;
        login(&e).await;
        let cookie = entrar_no_painel(&e).await;

        // A lista e o formulário não mostram tokenKey.
        let (_, _, corpo) = pedir(&e, "GET", "/_/", &[("cookie", cookie.as_str())], "").await;
        assert!(!corpo.contains("tokenKey"), "{corpo}");

        // Sair apaga o cookie.
        let (s, h, _) = form_post(&e, "/_/logout", Some(&cookie), &[]).await;
        assert_eq!(s, StatusCode::SEE_OTHER);
        assert_eq!(h["location"], "/_/login");
        let set = h["set-cookie"].to_str().unwrap();
        assert!(set.contains("Max-Age=0"), "{set}");
    }
}
