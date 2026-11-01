//! Servidor HTTP do Forja (axum). As rotas, os corpos e os erros seguem o
//! formato da API do PocketBase, para os SDKs existentes funcionarem.

mod lote;
mod painel;
mod rate_limit;
mod realtime;

use axum::body::Body;
use axum::extract::{
    DefaultBodyLimit, FromRequest, FromRequestParts, Multipart, Path, Query, Request, State,
};
use axum::http::header::{CONTENT_DISPOSITION, CONTENT_LENGTH, CONTENT_RANGE, CONTENT_TYPE, RANGE};
use axum::http::{StatusCode, request::Parts};
use axum::middleware::{self, Next};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use forja_core::arquivos::{self, Recebido};
use forja_core::auth::{
    self, COLECAO_SUPERUSERS, TIPO_ARQUIVO, TIPO_AUTH, TIPO_REDEFINICAO, TIPO_TROCA_EMAIL,
    TIPO_VERIFICACAO, VALIDADE_ARQUIVO, VALIDADE_AUTH, VALIDADE_EMAIL, VALIDADE_SUPERUSER,
};
use forja_core::correio::{Console, Correio, Mensagem};
use forja_core::filtro::Contexto;
use forja_core::oauth2::{self, AuthProviderInfo};
use forja_core::store::{Banco, Consulta, Credenciais, Motor, Superusuario};
use forja_core::{
    AuthOriginRegistro, Collection, CollectionType, Erro, Field, FieldType, agora_epoch_s,
    epoch_s_de_data_utc, id,
};
use serde::Deserialize;
use serde_json::{Map, Value, json};
use std::collections::HashMap;
use std::future::Future;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::Arc;

/// Onboarding do primeiro superusuário.
///
/// Enquanto o projeto não tiver nenhum superusuário, o painel redireciona para
/// `/_/install`, que exige este segredo. Assim, subir numa nuvem não deixa o
/// servidor aberto para qualquer um criar o usuário mestre: só quem enxerga o
/// console (ou recebeu o segredo) conclui a instalação.
#[derive(Debug, Clone)]
pub struct Instalador {
    /// Segredo de uso único, exigido na tela de instalação.
    pub token: String,
    /// E-mails autorizados a concluir o onboarding. Vazio = qualquer e-mail.
    pub emails: Vec<String>,
}

impl Instalador {
    /// Lê `FORJA_INSTALLER_TOKEN`/`FORJA_INSTALLER_EMAILS`; sem eles, gera um
    /// segredo aleatório (impresso no console na subida).
    pub fn do_ambiente() -> Instalador {
        let token = std::env::var("FORJA_INSTALLER_TOKEN")
            .ok()
            .filter(|t| !t.trim().is_empty())
            .unwrap_or_else(|| forja_core::id::aleatorio(40));
        let emails = std::env::var("FORJA_INSTALLER_EMAILS")
            .ok()
            .map(|t| {
                t.split(',')
                    .map(|e| e.trim().to_lowercase())
                    .filter(|e| !e.is_empty())
                    .collect()
            })
            .unwrap_or_default();
        Instalador { token, emails }
    }

    pub fn email_autorizado(&self, email: &str) -> bool {
        self.emails.is_empty() || self.emails.iter().any(|e| e == &email.to_lowercase())
    }
}

/// Estado compartilhado: o banco do projeto, o segredo dos tokens e o correio.
#[derive(Clone)]
pub struct Estado {
    pub banco: Banco,
    pub segredo: Arc<String>,
    pub correio: Arc<dyn Correio>,
    pub instalador: Arc<Instalador>,
    pub realtime: Arc<realtime::Hub>,
    pub rate_limiter: rate_limit::RateLimiter,
}

impl Estado {
    pub async fn novo(banco: Banco) -> Result<Estado, Erro> {
        Estado::com_correio(banco, Arc::new(Console)).await
    }

    /// Com um correio próprio (nos testes, o `Memoria`).
    pub async fn com_correio(banco: Banco, correio: Arc<dyn Correio>) -> Result<Estado, Erro> {
        Estado::com_instalador(banco, correio, Instalador::do_ambiente()).await
    }

    pub async fn com_instalador(
        banco: Banco,
        correio: Arc<dyn Correio>,
        instalador: Instalador,
    ) -> Result<Estado, Erro> {
        let segredo = banco.segredo().await?;
        let realtime = realtime::Hub::novo();
        let escuta = realtime.clone();
        let banco_escuta = banco.clone();
        tokio::spawn(async move {
            realtime::escutar(banco_escuta, escuta).await;
        });
        Ok(Estado {
            banco,
            segredo: Arc::new(segredo),
            correio,
            instalador: Arc::new(instalador),
            realtime,
            rate_limiter: rate_limit::RateLimiter::novo(),
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
#[allow(clippy::large_enum_variant)]
pub enum Identidade {
    Superusuario(Superusuario),
    Registro {
        colecao: Collection,
        registro: Value,
    },
}

pub struct Chamador {
    pub identidade: Option<Identidade>,
    pub method: String,
    pub headers: HashMap<String, String>,
    pub query: HashMap<String, String>,
}

impl Chamador {
    pub fn anonimo() -> Self {
        Self {
            identidade: None,
            method: String::new(),
            headers: HashMap::new(),
            query: HashMap::new(),
        }
    }

    pub fn com_identidade(identidade: Option<Identidade>) -> Self {
        Self {
            identidade,
            method: String::new(),
            headers: HashMap::new(),
            query: HashMap::new(),
        }
    }

    fn superusuario(&self) -> bool {
        matches!(self.identidade, Some(Identidade::Superusuario(_)))
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
        match &self.identidade {
            Some(Identidade::Registro { colecao, registro }) => Some((colecao, registro)),
            _ => None,
        }
    }

    /// O que as macros `@request.*` enxergam.
    fn contexto(&self) -> Contexto {
        self.contexto_com(None)
    }

    fn contexto_com(&self, dados: Option<&Value>) -> Contexto {
        let auth = match self.registro() {
            Some((_, registro)) => registro.as_object().cloned(),
            None => None,
        };
        let method = if self.method.is_empty() {
            None
        } else {
            Some(self.method.clone())
        };
        let query = if self.query.is_empty() {
            None
        } else {
            Some(self.query.clone())
        };
        let headers = if self.headers.is_empty() {
            None
        } else {
            Some(self.headers.clone())
        };
        let data = dados.and_then(|v| v.as_object().cloned());

        Contexto {
            auth,
            context: Some("default".to_string()),
            method,
            data,
            query,
            headers,
        }
    }
}

fn token_invalido() -> ErroApi {
    ErroApi(Erro::NaoAutorizado("Invalid or expired token.".into()))
}

impl FromRequestParts<Estado> for Chamador {
    type Rejection = ErroApi;

    async fn from_request_parts(parts: &mut Parts, e: &Estado) -> Result<Self, Self::Rejection> {
        let method = parts.method.as_str().to_string();
        let mut headers = HashMap::new();
        for (name, val) in &parts.headers {
            if let Ok(v) = val.to_str() {
                headers.insert(name.as_str().to_lowercase(), v.to_string());
            }
        }
        let mut query = HashMap::new();
        if let Some(q) = parts.uri.query() {
            for (k, v) in form_urlencoded::parse(q.as_bytes()) {
                query.insert(k.into_owned(), v.into_owned());
            }
        }

        let Some(bruto) = parts
            .headers
            .get("authorization")
            .and_then(|v| v.to_str().ok())
        else {
            return Ok(Chamador {
                identidade: None,
                method,
                headers,
                query,
            });
        };
        let token = token_do_header(bruto);
        if token.is_empty() {
            return Ok(Chamador {
                identidade: None,
                method,
                headers,
                query,
            });
        }
        let identidade = identificar(e, &token, TIPO_AUTH).await?;
        Ok(Chamador {
            identidade,
            method,
            headers,
            query,
        })
    }
}

fn token_do_header(bruto: &str) -> String {
    bruto
        .strip_prefix("Bearer ")
        .unwrap_or(bruto)
        .trim()
        .to_string()
}

async fn identificar(e: &Estado, token: &str, tipo: &str) -> Res<Option<Identidade>> {
    let claims = auth::validar(&e.segredo, token).map_err(|_| token_invalido())?;
    if claims.tipo != tipo {
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
        return Ok(Some(Identidade::Superusuario(su)));
    }
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
    Ok(Some(Identidade::Registro { colecao, registro }))
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
    match quem.identidade {
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

async fn colecoes_atualizar(
    State(e): State<Estado>,
    quem: Chamador,
    Path(nome): Path<String>,
    Json(c): Json<Collection>,
) -> Res<Json<Collection>> {
    quem.exigir_superusuario()?;
    Ok(Json(e.banco.atualizar_colecao(&nome, c).await?))
}

#[derive(Deserialize)]
struct BodyDryRunView {
    #[serde(default)]
    query: String,
}

async fn colecoes_dry_run_view(
    State(e): State<Estado>,
    quem: Chamador,
    Json(body): Json<BodyDryRunView>,
) -> Res<Json<Value>> {
    quem.exigir_superusuario()?;
    let fields = e.banco.dry_run_view(&body.query).await?;
    Ok(Json(json!({ "fields": fields })))
}

async fn settings_obter(
    State(e): State<Estado>,
    quem: Chamador,
) -> Res<Json<forja_core::store::Configuracoes>> {
    quem.exigir_superusuario()?;
    Ok(Json(e.banco.configuracoes().await))
}

async fn settings_atualizar(
    State(e): State<Estado>,
    quem: Chamador,
    Json(novo): Json<forja_core::store::Configuracoes>,
) -> Res<Json<forja_core::store::Configuracoes>> {
    quem.exigir_superusuario()?;
    let salva = e.banco.salvar_configuracoes(&novo).await?;
    Ok(Json(salva))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct BodyTestS3 {
    #[serde(default)]
    s3: Option<forja_core::store::S3Config>,
}

async fn settings_test_s3(
    State(e): State<Estado>,
    quem: Chamador,
    Json(body): Json<BodyTestS3>,
) -> Res<StatusCode> {
    quem.exigir_superusuario()?;
    let config = if let Some(cfg) = body.s3 {
        cfg
    } else {
        e.banco.configuracoes().await.s3
    };
    let s3 = forja_core::store::S3Storage::new(config);
    s3.testar_conexao().await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct BodyTestEmail {
    email: String,
    template: String,
    #[allow(dead_code)]
    #[serde(default)]
    collection: Option<String>,
}

async fn settings_test_email(
    State(e): State<Estado>,
    quem: Chamador,
    Json(body): Json<BodyTestEmail>,
) -> Res<StatusCode> {
    quem.exigir_superusuario()?;
    if !email_valido(&body.email) {
        return Err(ErroApi(Erro::campo(
            "email",
            "validation_is_email",
            "Must be a valid email address.",
        )));
    }
    let cfg = e.banco.configuracoes().await;
    let tpl = match body.template.as_str() {
        "verification" => &cfg.meta.verification_template,
        "password-reset" => &cfg.meta.reset_password_template,
        "email-change" => &cfg.meta.confirm_email_change_template,
        _ => {
            return Err(ErroApi(Erro::campo(
                "template",
                "validation_in_slice",
                "Invalid email template.",
            )));
        }
    };

    let mut extras = std::collections::HashMap::new();
    extras.insert("email".into(), body.email.clone());
    let (assunto, corpo) = tpl.renderizar(
        &cfg.meta.app_name,
        &cfg.meta.app_url,
        "test_token_1234567890",
        &extras,
    );

    enviar_email(&e, &body.email, &assunto, corpo)
        .await
        .map_err(|err| ErroApi(Erro::invalido(format!("Failed to send email: {err}"))))?;

    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct GerarAppleSecretPayload {
    #[serde(default)]
    client_id: String,
    #[serde(default)]
    team_id: String,
    #[serde(default)]
    key_id: String,
    #[serde(default)]
    private_key: String,
    #[serde(default)]
    duration: Option<u64>,
}

async fn apple_generate_client_secret(
    State(_e): State<Estado>,
    quem: Chamador,
    Json(p): Json<GerarAppleSecretPayload>,
) -> Res<Json<Value>> {
    quem.exigir_superusuario()?;
    let duracao = p.duration.unwrap_or(15_777_000);
    let secret = forja_core::oauth2::gerar_apple_client_secret(
        &p.client_id,
        &p.team_id,
        &p.key_id,
        &p.private_key,
        duracao,
    )?;
    Ok(Json(json!({ "secret": secret })))
}

// ── Logs de Requisição ───────────────────────────────────────────────────

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct QueryLogs {
    #[serde(default = "QueryLogs::page_padrao")]
    page: usize,
    #[serde(default = "QueryLogs::per_page_padrao")]
    per_page: usize,
    #[serde(default)]
    filter: Option<String>,
}

impl QueryLogs {
    fn page_padrao() -> usize {
        1
    }
    fn per_page_padrao() -> usize {
        30
    }
}

async fn logs_listar_api(
    State(e): State<Estado>,
    quem: Chamador,
    Query(q): Query<QueryLogs>,
) -> Res<Json<forja_core::store::PaginaLogs>> {
    quem.exigir_superusuario()?;
    let pagina = e
        .banco
        .logs_listar(q.page, q.per_page, q.filter.as_deref())
        .await?;
    Ok(Json(pagina))
}

async fn logs_obter_api(
    State(e): State<Estado>,
    quem: Chamador,
    Path(id): Path<String>,
) -> Res<Json<forja_core::store::LogItem>> {
    quem.exigir_superusuario()?;
    let item = e.banco.log_obter(&id).await?;
    Ok(Json(item))
}

#[derive(Deserialize)]
struct QueryStats {
    #[serde(default)]
    filter: Option<String>,
}

async fn logs_stats_api(
    State(e): State<Estado>,
    quem: Chamador,
    Query(q): Query<QueryStats>,
) -> Res<Json<Vec<forja_core::store::LogStat>>> {
    quem.exigir_superusuario()?;
    let stats = e.banco.logs_stats(q.filter.as_deref()).await?;
    Ok(Json(stats))
}

// ── Backups ──────────────────────────────────────────────────────────────

async fn backups_listar_api(
    State(e): State<Estado>,
    quem: Chamador,
) -> Res<Json<Vec<forja_core::store::BackupItem>>> {
    quem.exigir_superusuario()?;
    let itens = e.banco.backups_listar().await?;
    Ok(Json(itens))
}

#[derive(Deserialize)]
struct BodyCriarBackup {
    #[serde(default)]
    name: Option<String>,
}

async fn backups_criar_api(
    State(e): State<Estado>,
    quem: Chamador,
    body: Option<Json<BodyCriarBackup>>,
) -> Res<StatusCode> {
    quem.exigir_superusuario()?;
    let nome = body.and_then(|b| b.0.name);
    e.banco.backup_criar(nome.as_deref()).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn backups_baixar_api(
    State(e): State<Estado>,
    quem: Chamador,
    Path(key): Path<String>,
) -> Res<Response> {
    quem.exigir_superusuario()?;
    let dir = e
        .banco
        .backups_pasta()
        .ok_or_else(|| ErroApi(Erro::invalido("No backups folder.")))?;
    let caminho = dir.join(&key);
    if !caminho.exists() {
        return Err(ErroApi(Erro::NaoEncontrado("Backup not found.".into())));
    }
    let bytes = tokio::fs::read(&caminho)
        .await
        .map_err(|err| ErroApi(Erro::Interno(err.to_string())))?;

    let res = Response::builder()
        .header(CONTENT_TYPE, "application/zip")
        .header(
            CONTENT_DISPOSITION,
            format!("attachment; filename=\"{key}\""),
        )
        .body(Body::from(bytes))
        .map_err(|err| ErroApi(Erro::Interno(err.to_string())))?;

    Ok(res)
}

async fn backups_apagar_api(
    State(e): State<Estado>,
    quem: Chamador,
    Path(key): Path<String>,
) -> Res<StatusCode> {
    quem.exigir_superusuario()?;
    e.banco.backup_apagar(&key).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn backups_restaurar_api(
    State(e): State<Estado>,
    quem: Chamador,
    Path(key): Path<String>,
) -> Res<StatusCode> {
    quem.exigir_superusuario()?;
    e.banco.backup_restaurar(&key).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn backups_upload_api(
    State(e): State<Estado>,
    quem: Chamador,
    mut multipart: axum::extract::Multipart,
) -> Res<StatusCode> {
    quem.exigir_superusuario()?;
    let dir = e
        .banco
        .backups_pasta()
        .ok_or_else(|| ErroApi(Erro::invalido("No backups folder.")))?;
    tokio::fs::create_dir_all(&dir)
        .await
        .map_err(|err| ErroApi(Erro::Interno(err.to_string())))?;

    let mut gravou = false;
    if let Some(field) = multipart
        .next_field()
        .await
        .map_err(|err| ErroApi(Erro::invalido(err.to_string())))?
    {
        let nome_arquivo = field.file_name().unwrap_or("backup.zip").to_string();
        if !nome_arquivo.ends_with(".zip")
            || nome_arquivo.contains("..")
            || nome_arquivo.contains('/')
        {
            return Err(ErroApi(Erro::invalido("Invalid backup zip file.")));
        }
        let data = field
            .bytes()
            .await
            .map_err(|err| ErroApi(Erro::invalido(err.to_string())))?;
        tokio::fs::write(dir.join(nome_arquivo), data)
            .await
            .map_err(|err| ErroApi(Erro::Interno(err.to_string())))?;
        gravou = true;
    }
    if !gravou {
        return Err(ErroApi(Erro::invalido("No file provided.")));
    }
    Ok(StatusCode::NO_CONTENT)
}

// ── Crons ────────────────────────────────────────────────────────────────

async fn crons_listar_api(
    State(e): State<Estado>,
    quem: Chamador,
) -> Res<Json<Vec<forja_core::store::CronItem>>> {
    quem.exigir_superusuario()?;
    let itens = e.banco.crons_listar().await?;
    Ok(Json(itens))
}

async fn cron_executar_api(
    State(e): State<Estado>,
    quem: Chamador,
    Path(id): Path<String>,
) -> Res<StatusCode> {
    quem.exigir_superusuario()?;
    e.banco.cron_executar(&id).await?;
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

/// Esconde campos do registro e de tudo que veio em `expand`.
fn apresentar<'a>(
    e: &'a Estado,
    quem: &'a Chamador,
    c: &'a Collection,
    v: Value,
) -> Pin<Box<dyn Future<Output = Value> + Send + 'a>> {
    Box::pin(async move {
        let mut v = esconder(c, quem, v);
        let Some(exp) = v.as_object_mut().and_then(|m| m.remove("expand")) else {
            return v;
        };
        let Value::Object(obj) = exp else {
            if let Some(m) = v.as_object_mut() {
                m.insert("expand".into(), exp);
            }
            return v;
        };
        let mut novo = Map::new();
        for (k, val) in obj {
            novo.insert(k, apresentar_valor(e, quem, val).await);
        }
        if let Some(m) = v.as_object_mut() {
            m.insert("expand".into(), Value::Object(novo));
        }
        v
    })
}

fn apresentar_valor<'a>(
    e: &'a Estado,
    quem: &'a Chamador,
    val: Value,
) -> Pin<Box<dyn Future<Output = Value> + Send + 'a>> {
    Box::pin(async move {
        match val {
            Value::Array(a) => {
                let mut out = Vec::with_capacity(a.len());
                for item in a {
                    out.push(apresentar_qualquer(e, quem, item).await);
                }
                Value::Array(out)
            }
            Value::Object(_) => apresentar_qualquer(e, quem, val).await,
            other => other,
        }
    })
}

fn apresentar_qualquer<'a>(
    e: &'a Estado,
    quem: &'a Chamador,
    v: Value,
) -> Pin<Box<dyn Future<Output = Value> + Send + 'a>> {
    Box::pin(async move {
        let nome = v
            .get("collectionName")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        match e.banco.colecao(&nome).await {
            Ok(c) => apresentar(e, quem, &c, v).await,
            Err(_) => v,
        }
    })
}

pub(crate) fn filtrar_campos(mut reg: Value, fields_param: &str) -> Value {
    let campos: Vec<&str> = fields_param
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect();
    if campos.is_empty() || campos.contains(&"*") {
        return reg;
    }
    let Value::Object(ref mut mapa) = reg else {
        return reg;
    };

    let mut novo = Map::new();
    let mut expand_seletores: Vec<(&str, &str)> = Vec::new();

    for seletor in campos {
        let (nome_puro, modifier) = match seletor.split_once(':') {
            Some((n, m)) => (n.trim(), Some(m.trim())),
            None => (seletor, None),
        };

        if let Some(resto) = nome_puro.strip_prefix("expand.") {
            if let Some((sub_exp, campo_exp)) = resto.split_once('.') {
                expand_seletores.push((sub_exp, campo_exp));
            } else {
                expand_seletores.push((resto, "*"));
            }
            continue;
        }

        if let Some(val) = mapa.get(nome_puro) {
            let mut val_final = val.clone();
            if let Some(mod_str) = modifier
                && let Some(dentro) = mod_str
                    .strip_prefix("excerpt(")
                    .and_then(|s| s.strip_suffix(')'))
            {
                let len = dentro
                    .split(',')
                    .next()
                    .unwrap_or("")
                    .trim()
                    .parse::<usize>()
                    .unwrap_or(100);
                if let Value::String(ref s) = val_final
                    && s.chars().count() > len
                {
                    let cortado: String = s.chars().take(len).collect();
                    val_final = Value::String(format!("{cortado}..."));
                }
            }
            novo.insert(nome_puro.to_string(), val_final);
        }
    }

    if !expand_seletores.is_empty()
        && let Some(Value::Object(exp_mapa)) = mapa.get("expand")
    {
        let mut novo_exp = Map::new();
        for (sub, campo) in expand_seletores {
            if let Some(sub_val) = exp_mapa.get(sub) {
                if campo == "*" {
                    novo_exp.insert(sub.to_string(), sub_val.clone());
                } else if let Value::Object(sub_obj) = sub_val {
                    let mut sub_novo = match novo_exp.remove(sub) {
                        Some(Value::Object(m)) => m,
                        _ => Map::new(),
                    };
                    if let Some(v) = sub_obj.get(campo) {
                        sub_novo.insert(campo.to_string(), v.clone());
                    }
                    novo_exp.insert(sub.to_string(), Value::Object(sub_novo));
                }
            }
        }
        novo.insert("expand".to_string(), Value::Object(novo_exp));
    }

    Value::Object(novo)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ParamsLista {
    page: Option<u32>,
    per_page: Option<u32>,
    sort: Option<String>,
    filter: Option<String>,
    skip_total: Option<bool>,
    expand: Option<String>,
    fields: Option<String>,
}

#[derive(Deserialize, Default)]
struct ParamsExpand {
    expand: Option<String>,
    fields: Option<String>,
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
    let pares: Vec<(String, String)> = [
        p.page.map(|n| ("page".to_string(), n.to_string())),
        p.per_page.map(|n| ("perPage".to_string(), n.to_string())),
        p.sort.map(|s| ("sort".to_string(), s)),
        p.filter.map(|s| ("filter".to_string(), s)),
        p.skip_total
            .map(|b| ("skipTotal".to_string(), b.to_string())),
        p.expand.map(|s| ("expand".to_string(), s)),
        p.fields.map(|s| ("fields".to_string(), s)),
    ]
    .into_iter()
    .flatten()
    .collect();
    Ok(Json(listar_registro(&e, &quem, &c, &pares).await?))
}

async fn listar_registro(
    e: &Estado,
    quem: &Chamador,
    c: &Collection,
    pares: &[(String, String)],
) -> Result<Value, Erro> {
    let regra = regra_para(quem, &c.list_rule).map_err(|err| err.0)?;
    let q = Consulta {
        filtro: par(pares, "filter").map(str::to_string),
        ordem: par(pares, "sort").map(str::to_string),
        pagina: par(pares, "page").and_then(|s| s.parse().ok()).unwrap_or(1),
        por_pagina: par(pares, "perPage")
            .and_then(|s| s.parse().ok())
            .unwrap_or(30),
        pular_total: par(pares, "skipTotal").is_some_and(|s| s == "true"),
        regra,
        contexto: quem.contexto(),
        superusuario: quem.superusuario(),
    };
    let mut lista = e.banco.listar(c, &q).await?;
    if let Some(exp) = par(pares, "expand").filter(|s| !s.trim().is_empty()) {
        e.banco
            .expandir(
                c,
                &mut lista.items,
                exp,
                quem.superusuario(),
                &quem.contexto(),
            )
            .await?;
    }
    let fields_opt = par(pares, "fields").filter(|s| !s.trim().is_empty());
    let mut items = Vec::with_capacity(lista.items.len());
    for v in lista.items {
        let mut apresentada = apresentar(e, quem, c, v).await;
        if let Some(f) = fields_opt {
            apresentada = filtrar_campos(apresentada, f);
        }
        items.push(apresentada);
    }
    lista.items = items;
    Ok(serde_json::to_value(lista).unwrap_or_default())
}

async fn registros_ver(
    State(e): State<Estado>,
    quem: Chamador,
    Path((nome, id)): Path<(String, String)>,
    Query(p): Query<ParamsExpand>,
) -> Res<Json<Value>> {
    let c = colecao_de(&e, nome).await?;
    let mut out = ver_registro(&e, &quem, &c, &id, p.expand).await?;
    if let Some(f) = p.fields.as_deref().filter(|s| !s.trim().is_empty()) {
        out = filtrar_campos(out, f);
    }
    Ok(Json(out))
}

async fn ver_registro(
    e: &Estado,
    quem: &Chamador,
    c: &Collection,
    id: &str,
    expand: Option<String>,
) -> Result<Value, Erro> {
    let regra = regra_para(quem, &c.view_rule).map_err(|err| err.0)?;
    let mut r = e
        .banco
        .registro_com(c, id, regra.as_deref(), &quem.contexto())
        .await?;
    if let Some(exp) = expand.filter(|s| !s.trim().is_empty()) {
        let mut itens = vec![r];
        e.banco
            .expandir(c, &mut itens, &exp, quem.superusuario(), &quem.contexto())
            .await?;
        r = itens.pop().unwrap_or(Value::Null);
    }
    Ok(apresentar(e, quem, c, r).await)
}

async fn registros_criar(
    State(e): State<Estado>,
    quem: Chamador,
    Path(nome): Path<String>,
    Query(p): Query<ParamsExpand>,
    req: Request,
) -> Res<Json<Value>> {
    let c = colecao_de(&e, nome).await?;
    let (dados, arquivos) = ler_corpo(req, &c).await?;
    let mut out = criar_registro(&e, &quem, &c, dados, arquivos, p.expand).await?;
    if let Some(f) = p.fields.as_deref().filter(|s| !s.trim().is_empty()) {
        out = filtrar_campos(out, f);
    }
    Ok(Json(out))
}

async fn criar_registro(
    e: &Estado,
    quem: &Chamador,
    c: &Collection,
    mut dados: Map<String, Value>,
    arquivos: Vec<Recebido>,
    expand: Option<String>,
) -> Result<Value, Erro> {
    if c.kind == CollectionType::View {
        return Err(Erro::invalido("The collection is not writable."));
    }
    if c.kind == CollectionType::Auth && !quem.superusuario() {
        dados.remove("verified");
    }
    let regra = regra_para(quem, &c.create_rule).map_err(|err| err.0)?;
    let dados_ctx = Value::Object(dados.clone());
    let r = e
        .banco
        .criar_com_arquivos(
            c,
            &dados,
            &quem.contexto_com(Some(&dados_ctx)),
            quem.superusuario(),
            arquivos,
        )
        .await?;
    if let Some(regra) = regra {
        let id = r["id"].as_str().unwrap_or_default();
        if e.banco
            .registro_com(c, id, Some(&regra), &quem.contexto_com(Some(&dados_ctx)))
            .await
            .is_err()
        {
            e.banco.apagar(c, id, None).await?;
            return Err(Erro::invalido("Failed to create record."));
        }
    }
    let mut r = r;
    if let Some(exp) = expand.filter(|s| !s.trim().is_empty()) {
        let mut itens = vec![r];
        e.banco
            .expandir(c, &mut itens, &exp, quem.superusuario(), &quem.contexto())
            .await?;
        r = itens.pop().unwrap_or(Value::Null);
    }
    Ok(apresentar(e, quem, c, r).await)
}

async fn registros_atualizar(
    State(e): State<Estado>,
    quem: Chamador,
    Path((nome, id)): Path<(String, String)>,
    Query(p): Query<ParamsExpand>,
    req: Request,
) -> Res<Json<Value>> {
    let c = colecao_de(&e, nome).await?;
    let (dados, arquivos) = ler_corpo(req, &c).await?;
    let mut out = atualizar_registro(&e, &quem, &c, &id, dados, arquivos, p.expand).await?;
    if let Some(f) = p.fields.as_deref().filter(|s| !s.trim().is_empty()) {
        out = filtrar_campos(out, f);
    }
    Ok(Json(out))
}

async fn atualizar_registro(
    e: &Estado,
    quem: &Chamador,
    c: &Collection,
    id: &str,
    mut dados: Map<String, Value>,
    arquivos: Vec<Recebido>,
    expand: Option<String>,
) -> Result<Value, Erro> {
    if c.kind == CollectionType::View {
        return Err(Erro::invalido("The collection is not writable."));
    }
    let regra = regra_para(quem, &c.update_rule).map_err(|err| err.0)?;
    if c.kind == CollectionType::Auth && !quem.superusuario() {
        dados.remove("verified");
        if let Some(novo) = dados.get("email").and_then(Value::as_str) {
            let atual = e.banco.registro(c, id, None).await?;
            let atual = atual
                .get("email")
                .and_then(Value::as_str)
                .unwrap_or_default();
            if !novo.trim().eq_ignore_ascii_case(atual) {
                return Err(Erro::Proibido(
                    "The email can be changed only by superusers or via the email change flow."
                        .into(),
                ));
            }
            dados.remove("email");
        }
    }
    let dados_ctx = Value::Object(dados.clone());
    let mut r = e
        .banco
        .atualizar_com_arquivos(
            c,
            id,
            &dados,
            regra.as_deref(),
            &quem.contexto_com(Some(&dados_ctx)),
            quem.superusuario(),
            arquivos,
        )
        .await?;
    if let Some(exp) = expand.filter(|s| !s.trim().is_empty()) {
        let mut itens = vec![r];
        e.banco
            .expandir(c, &mut itens, &exp, quem.superusuario(), &quem.contexto())
            .await?;
        r = itens.pop().unwrap_or(Value::Null);
    }
    Ok(apresentar(e, quem, c, r).await)
}

async fn registros_apagar(
    State(e): State<Estado>,
    quem: Chamador,
    Path((nome, id)): Path<(String, String)>,
) -> Res<StatusCode> {
    let c = colecao_de(&e, nome).await?;
    apagar_registro(&e, &quem, &c, &id).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn apagar_registro(
    e: &Estado,
    quem: &Chamador,
    c: &Collection,
    id: &str,
) -> Result<(), Erro> {
    if c.kind == CollectionType::View {
        return Err(Erro::invalido("The collection is not writable."));
    }
    let regra = regra_para(quem, &c.delete_rule).map_err(|err| err.0)?;
    e.banco
        .apagar_com(c, id, regra.as_deref(), &quem.contexto())
        .await
}

fn par<'a>(pares: &'a [(String, String)], nome: &str) -> Option<&'a str> {
    pares
        .iter()
        .find(|(k, _)| k == nome)
        .map(|(_, v)| v.as_str())
}

const MAX_CORPO: usize = 32 * 1024 * 1024;

async fn ler_corpo(req: Request, c: &Collection) -> Res<(Map<String, Value>, Vec<Recebido>)> {
    let ct = req
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_ascii_lowercase();
    if ct.starts_with("multipart/form-data") {
        return ler_multipart(req, c).await;
    }
    let bytes = axum::body::to_bytes(req.into_body(), MAX_CORPO)
        .await
        .map_err(|_| ErroApi(Erro::invalido("Invalid request body.")))?;
    if bytes.is_empty() {
        return Ok((Map::new(), vec![]));
    }
    let valor: Value =
        serde_json::from_slice(&bytes).map_err(|_| ErroApi(Erro::invalido("Invalid JSON.")))?;
    let obj = valor
        .as_object()
        .cloned()
        .ok_or_else(|| ErroApi(Erro::invalido("Expected a JSON object.")))?;
    Ok((obj, vec![]))
}

async fn ler_multipart(req: Request, c: &Collection) -> Res<(Map<String, Value>, Vec<Recebido>)> {
    let mut mp = Multipart::from_request(req, &())
        .await
        .map_err(|_| ErroApi(Erro::invalido("Invalid multipart body.")))?;
    let mut textos = Vec::new();
    let mut recebidos = Vec::new();
    while let Some(field) = mp
        .next_field()
        .await
        .map_err(|_| ErroApi(Erro::invalido("Invalid multipart body.")))?
    {
        let nome = field.name().unwrap_or("").to_string();
        if nome.is_empty() {
            continue;
        }
        if let Some(arquivo) = field.file_name().map(str::to_string) {
            let (campo, acrescentar) = nome
                .strip_suffix('+')
                .map(|b| (b.to_string(), true))
                .unwrap_or((nome, false));
            let tipo = field.content_type().unwrap_or("").to_string();
            let bytes = field
                .bytes()
                .await
                .map_err(|_| ErroApi(Erro::invalido("Invalid multipart body.")))?;
            recebidos.push(Recebido {
                campo,
                acrescentar,
                nome_original: arquivo,
                tipo,
                bytes: bytes.to_vec(),
            });
        } else {
            let texto = field
                .text()
                .await
                .map_err(|_| ErroApi(Erro::invalido("Invalid multipart body.")))?;
            textos.push((nome, texto));
        }
    }
    Ok((dados_multipart(c, &textos), recebidos))
}

fn dados_multipart(c: &Collection, pares: &[(String, String)]) -> Map<String, Value> {
    let mut grupos: Vec<(String, Vec<String>)> = Vec::new();
    for (k, v) in pares {
        if let Some((_, g)) = grupos.iter_mut().find(|(n, _)| n == k) {
            g.push(v.clone());
        } else {
            grupos.push((k.clone(), vec![v.clone()]));
        }
    }
    let mut dados = Map::new();
    for (k, valores) in grupos {
        let base = k.strip_suffix(['+', '-']).unwrap_or(&k);
        let campo = c.campo(base);
        let valor = match campo {
            Some(f) if f.kind == FieldType::Bool => {
                json!(valores[0] == "true" || valores[0] == "1")
            }
            Some(f) if f.kind == FieldType::Number => valores[0]
                .parse::<f64>()
                .map(|n| json!(n))
                .unwrap_or_else(|_| json!(&valores[0])),
            Some(f) if f.kind == FieldType::Json => {
                serde_json::from_str(&valores[0]).unwrap_or_else(|_| json!(&valores[0]))
            }
            Some(f)
                if matches!(
                    f.kind,
                    FieldType::Select | FieldType::Relation | FieldType::File
                ) && (f.multiplo() || k.ends_with('-')) =>
            {
                json!(valores)
            }
            _ if k.ends_with('-') && valores.len() > 1 => json!(valores),
            _ => json!(&valores[0]),
        };
        dados.insert(k, valor);
    }
    dados
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

async fn enviar_email(e: &Estado, para: &str, assunto: &str, corpo: String) -> Result<(), Erro> {
    let cfg = e.banco.configuracoes().await;
    let msg = Mensagem::com_remetente(
        para,
        assunto,
        corpo,
        Some(cfg.meta.sender_name),
        Some(cfg.meta.sender_address),
    );
    if cfg.smtp.enabled && !cfg.smtp.host.is_empty() {
        let smtp = forja_core::correio::SmtpCorreio::new(cfg.smtp);
        smtp.enviar(&msg).await?;
    } else {
        e.correio.enviar(&msg).await?;
    }
    Ok(())
}

async fn enviar(e: &Estado, para: &str, assunto: &str, corpo: String) {
    let _ = enviar_email(e, para, assunto, corpo).await;
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
    #[serde(default, rename = "mfaId")]
    mfa_id: Option<String>,
}

fn resposta_mfa(mfa_id: &str) -> Response {
    (
        StatusCode::UNAUTHORIZED,
        [(axum::http::header::CONTENT_TYPE, "application/json")],
        json!({ "mfaId": mfa_id }).to_string(),
    )
        .into_response()
}

fn extrair_ip_e_user_agent(headers: &axum::http::HeaderMap) -> (String, String) {
    let ip = headers
        .get("x-forwarded-for")
        .or_else(|| headers.get("x-real-ip"))
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.split(',').next())
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|| "127.0.0.1".into());
    let user_agent = headers
        .get("user-agent")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();
    (ip, user_agent)
}

async fn processar_auth_origin_e_alerta(
    e: &Estado,
    c: &Collection,
    record_id: &str,
    email: Option<&str>,
    ip: &str,
    user_agent: &str,
) {
    if let Ok(novo) = e
        .banco
        .auth_origin_registrar(&c.id, record_id, ip, user_agent)
        .await
    {
        let alert_cfg = c.auth_alert_config();
        if novo
            && alert_cfg.enabled
            && let Some(destinatario) = email.filter(|em| !em.is_empty())
        {
            let app_name = e.banco.configuracoes().await.meta.app_name;
            let placeholders = [
                ("{APP_NAME}", app_name.as_str()),
                ("{RECORD_ID}", record_id),
            ];
            let tpl = alert_cfg.email_template.as_ref();
            let assunto = if let Some(t) = tpl.filter(|t| !t.subject.is_empty()) {
                let mut s = t.subject.clone();
                for (k, v) in &placeholders {
                    s = s.replace(k, v);
                }
                s
            } else {
                format!("Login from a new device to {app_name}")
            };
            let corpo = if let Some(t) = tpl.filter(|t| !t.body.is_empty()) {
                let mut b = t.body.clone();
                for (k, v) in &placeholders {
                    b = b.replace(k, v);
                }
                b
            } else {
                format!(
                    "Hello,\n\nWe detected a login to your {app_name} account from a new device (IP: {ip}).\n\nIf this was you, you can safely ignore this email.\n\nIf you did not authorize this login, please immediately change your password."
                )
            };
            enviar(e, destinatario, &assunto, corpo).await;
        }
    }
}

async fn auth_com_senha(
    State(e): State<Estado>,
    Path(nome): Path<String>,
    headers: axum::http::HeaderMap,
    Json(l): Json<LoginAuth>,
) -> Res<Response> {
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

    let mfa_cfg = c.mfa_config();
    if mfa_cfg.enabled {
        if let Some(mfa_id) = &l.mfa_id {
            let sessao = e.banco.mfa_obter(&c.id, mfa_id).await?.ok_or_else(|| {
                ErroApi(Erro::invalido(
                    "Failed to authenticate with MFA. Invalid MFA session.",
                ))
            })?;
            let criado_s = epoch_s_de_data_utc(&sessao.created).unwrap_or(0);
            if agora_epoch_s() > criado_s + sessao.duration {
                let _ = e.banco.mfa_apagar(mfa_id).await;
                return Err(ErroApi(Erro::invalido("MFA session expired.")));
            }
            if sessao.record_id != cred.id {
                return Err(ErroApi(Erro::invalido("Invalid MFA session for user.")));
            }
            if sessao.method == "password" {
                return Err(ErroApi(Erro::invalido(
                    "Second MFA factor must be different.",
                )));
            }
            let _ = e.banco.mfa_apagar(mfa_id).await;
        } else {
            let novo_mfa_id = e
                .banco
                .mfa_criar(&c.id, &cred.id, "password", mfa_cfg.duration)
                .await?;
            return Ok(resposta_mfa(&novo_mfa_id));
        }
    }

    let (ip, user_agent) = extrair_ip_e_user_agent(&headers);
    processar_auth_origin_e_alerta(&e, &c, &cred.id, Some(&cred.email), &ip, &user_agent).await;

    let registro = e.banco.registro(&c, &cred.id, None).await?;
    let resp = resposta_registro(&e, &c, &cred, registro)?;
    Ok(resp.into_response())
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
        )
        .await;
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
        )
        .await;
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
    )
    .await;
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

#[derive(Deserialize, Default)]
struct QueryAuthMethods {
    #[serde(default)]
    state: Option<String>,
}

async fn obter_auth_methods(
    State(e): State<Estado>,
    Path(nome): Path<String>,
    Query(q): Query<QueryAuthMethods>,
) -> Res<Json<Value>> {
    let c = colecao_auth(&e, nome).await?;
    let cfg = e.banco.configuracoes().await;
    let redirect_uri = format!(
        "{}/api/oauth2-redirect",
        cfg.meta.app_url.trim_end_matches('/')
    );

    let mfa_cfg = c.mfa_config();
    let otp_cfg = c.otp_config();
    let pwd_cfg = c.password_auth_config();
    let oauth2_cfg = c.oauth2_config();

    let providers: Vec<AuthProviderInfo> = oauth2_cfg
        .providers
        .iter()
        .filter(|p| p.habilitado())
        .map(|p| oauth2::preparar_provedor(p, &redirect_uri, q.state.as_deref()))
        .collect();

    Ok(Json(json!({
        "mfa": {
            "enabled": mfa_cfg.enabled,
            "duration": mfa_cfg.duration
        },
        "otp": {
            "enabled": otp_cfg.enabled,
            "duration": otp_cfg.duration
        },
        "password": {
            "enabled": pwd_cfg.enabled,
            "identityFields": pwd_cfg.identity_fields
        },
        "oauth2": {
            "enabled": oauth2_cfg.enabled,
            "providers": providers
        }
    })))
}

#[derive(Deserialize)]
struct PedidoOtp {
    email: String,
}

async fn pedir_otp(
    State(e): State<Estado>,
    Path(nome): Path<String>,
    Json(p): Json<PedidoOtp>,
) -> Res<Json<Value>> {
    let c = colecao_auth(&e, nome).await?;
    let otp_cfg = c.otp_config();
    if !otp_cfg.enabled {
        return Err(ErroApi(Erro::invalido(
            "Failed to request OTP. OTP authentication is disabled for this collection.",
        )));
    }
    let otp_id = id::novo();
    let code = oauth2::gerar_codigo_otp(otp_cfg.length);
    let hash = auth::hash_senha(&code)?;

    let mut id_retorno = otp_id.clone();

    if let Some(usuario) = e.banco.auth_credenciais(&c, &p.email).await? {
        let criado_id = e
            .banco
            .otp_criar(
                &c.id,
                &usuario.id,
                &usuario.email,
                &hash,
                &usuario.email,
                otp_cfg.duration,
            )
            .await?;
        id_retorno = criado_id;

        let cfg = e.banco.configuracoes().await;
        let mut placeholders = std::collections::HashMap::new();
        placeholders.insert("APP_NAME", cfg.meta.app_name.clone());
        placeholders.insert("APP_URL", cfg.meta.app_url.clone());
        placeholders.insert("OTP", code.clone());
        placeholders.insert("OTP_ID", id_retorno.clone());
        placeholders.insert("DURATION", otp_cfg.duration.to_string());
        placeholders.insert("RECORD:email", usuario.email.clone());

        let (assunto, corpo) = if let Some(tmpl) = &otp_cfg.email_template {
            let mut extras = std::collections::HashMap::new();
            extras.insert("email".into(), usuario.email.clone());
            extras.insert("otpId".into(), id_retorno.clone());
            extras.insert("duration".into(), otp_cfg.duration.to_string());
            let (mut a, mut b) =
                tmpl.renderizar(&cfg.meta.app_name, &cfg.meta.app_url, &code, &extras);
            a = a.replace("{OTP}", &code);
            b = b.replace("{OTP}", &code);
            (a, b)
        } else {
            (
                format!("OTP verification code for {}", cfg.meta.app_name),
                format!(
                    "Your one-time password is: {}\n\nIt will expire in {} seconds.",
                    code, otp_cfg.duration
                ),
            )
        };

        let _ = enviar_email(&e, &usuario.email, &assunto, corpo).await;
    }

    Ok(Json(json!({ "otpId": id_retorno })))
}

#[derive(Deserialize)]
struct AuthComOtp {
    #[serde(rename = "otpId")]
    otp_id: String,
    #[serde(default)]
    password: Option<String>,
    #[serde(default)]
    code: Option<String>,
    #[serde(default, rename = "mfaId")]
    mfa_id: Option<String>,
}

async fn auth_com_otp(
    State(e): State<Estado>,
    Path(nome): Path<String>,
    headers: axum::http::HeaderMap,
    Json(p): Json<AuthComOtp>,
) -> Res<Response> {
    let c = colecao_auth(&e, nome).await?;
    let otp_cfg = c.otp_config();
    if !otp_cfg.enabled {
        return Err(ErroApi(Erro::invalido(
            "Failed to authenticate. OTP authentication is disabled for this collection.",
        )));
    }
    let codigo = p
        .password
        .or(p.code)
        .ok_or_else(|| ErroApi(Erro::invalido("Missing OTP code.")))?;

    let otp = e.banco.otp_obter(&c.id, &p.otp_id).await?.ok_or_else(|| {
        ErroApi(Erro::invalido(
            "Failed to authenticate. Invalid or expired OTP.",
        ))
    })?;

    let criado_s = epoch_s_de_data_utc(&otp.created).unwrap_or(0);
    if agora_epoch_s() > criado_s + otp.duration {
        let _ = e.banco.otp_apagar(&otp.id).await;
        return Err(ErroApi(Erro::invalido(
            "Failed to authenticate. Invalid or expired OTP.",
        )));
    }

    let ok = auth::conferir_senha(&otp.password, &codigo);
    if !ok {
        return Err(ErroApi(Erro::invalido(
            "Failed to authenticate. Invalid or expired OTP.",
        )));
    }

    let _ = e.banco.otp_apagar(&otp.id).await;

    let cred = credenciais_de(&e, &c, &otp.record_id).await?;

    if !cred.verified {
        let _ = e.banco.auth_verificar(&c, &cred.id, None).await;
    }

    let mfa_cfg = c.mfa_config();
    if mfa_cfg.enabled {
        if let Some(mfa_id) = &p.mfa_id {
            let sessao = e.banco.mfa_obter(&c.id, mfa_id).await?.ok_or_else(|| {
                ErroApi(Erro::invalido(
                    "Failed to authenticate with MFA. Invalid MFA session.",
                ))
            })?;
            let criado_s = epoch_s_de_data_utc(&sessao.created).unwrap_or(0);
            if agora_epoch_s() > criado_s + sessao.duration {
                let _ = e.banco.mfa_apagar(mfa_id).await;
                return Err(ErroApi(Erro::invalido("MFA session expired.")));
            }
            if sessao.record_id != cred.id {
                return Err(ErroApi(Erro::invalido("Invalid MFA session for user.")));
            }
            if sessao.method == "otp" {
                return Err(ErroApi(Erro::invalido(
                    "Second MFA factor must be different.",
                )));
            }
            let _ = e.banco.mfa_apagar(mfa_id).await;
        } else {
            let novo_mfa_id = e
                .banco
                .mfa_criar(&c.id, &cred.id, "otp", mfa_cfg.duration)
                .await?;
            return Ok(resposta_mfa(&novo_mfa_id));
        }
    }

    let (ip, user_agent) = extrair_ip_e_user_agent(&headers);
    processar_auth_origin_e_alerta(&e, &c, &cred.id, Some(&cred.email), &ip, &user_agent).await;

    let registro = e.banco.registro(&c, &cred.id, None).await?;
    let resp = resposta_registro(&e, &c, &cred, registro)?;
    Ok(resp.into_response())
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct AuthComOAuth2 {
    provider: String,
    code: String,
    #[serde(default)]
    code_verifier: Option<String>,
    #[serde(default)]
    redirect_url: Option<String>,
    #[serde(default)]
    create_data: Option<Map<String, Value>>,
    #[serde(default)]
    mfa_id: Option<String>,
}

async fn auth_com_oauth2(
    State(e): State<Estado>,
    Path(nome): Path<String>,
    headers: axum::http::HeaderMap,
    Json(p): Json<AuthComOAuth2>,
) -> Res<Response> {
    let c = colecao_auth(&e, nome).await?;
    let oauth2_cfg = c.oauth2_config();
    if !oauth2_cfg.enabled {
        return Err(ErroApi(Erro::invalido(
            "Failed to authenticate. OAuth2 authentication is disabled for this collection.",
        )));
    }

    let provedor = oauth2_cfg
        .providers
        .iter()
        .find(|prov| prov.name.eq_ignore_ascii_case(&p.provider) && prov.habilitado())
        .ok_or_else(|| {
            ErroApi(Erro::invalido(format!(
                "OAuth2 provider '{}' is not configured or disabled.",
                p.provider
            )))
        })?;

    let cfg = e.banco.configuracoes().await;
    let redirect_url = p.redirect_url.filter(|u| !u.is_empty()).unwrap_or_else(|| {
        format!(
            "{}/api/oauth2-redirect",
            cfg.meta.app_url.trim_end_matches('/')
        )
    });

    let token_resp =
        oauth2::trocar_codigo(provedor, &p.code, p.code_verifier.as_deref(), &redirect_url).await?;

    let perfil = oauth2::buscar_perfil(provedor, &token_resp.access_token).await?;

    let mut is_new = false;
    let record_id = if let Some(ext) = e
        .banco
        .external_auth_obter(&c.id, &provedor.name, &perfil.id)
        .await?
    {
        ext.record_id
    } else if !perfil.email.is_empty()
        && let Some(usuario) = e.banco.auth_credenciais(&c, &perfil.email).await?
    {
        let _ = e
            .banco
            .external_auth_criar(&c.id, &usuario.id, &provedor.name, &perfil.id)
            .await?;
        usuario.id
    } else {
        let novo_id = id::novo();
        let mut mapa = p.create_data.unwrap_or_default();
        mapa.insert("id".into(), novo_id.clone().into());
        mapa.insert("email".into(), perfil.email.clone().into());
        mapa.insert("emailVisibility".into(), Value::Bool(true));
        mapa.insert("verified".into(), Value::Bool(true));

        let senha_rand = id::aleatorio(30);
        mapa.insert("password".into(), senha_rand.into());

        if c.campo("name").is_some() && !mapa.contains_key("name") && !perfil.name.is_empty() {
            mapa.insert("name".into(), perfil.name.clone().into());
        }
        if c.campo("avatar").is_some()
            && !mapa.contains_key("avatar")
            && !perfil.avatar_url.is_empty()
        {
            mapa.insert("avatar".into(), perfil.avatar_url.clone().into());
        }

        let _ = e.banco.criar(&c, &mapa).await?;
        let _ = e
            .banco
            .external_auth_criar(&c.id, &novo_id, &provedor.name, &perfil.id)
            .await?;
        is_new = true;
        novo_id
    };

    let cred = credenciais_de(&e, &c, &record_id).await?;

    let mfa_cfg = c.mfa_config();
    if mfa_cfg.enabled {
        if let Some(mfa_id) = &p.mfa_id {
            let sessao = e.banco.mfa_obter(&c.id, mfa_id).await?.ok_or_else(|| {
                ErroApi(Erro::invalido(
                    "Failed to authenticate with MFA. Invalid MFA session.",
                ))
            })?;
            let criado_s = epoch_s_de_data_utc(&sessao.created).unwrap_or(0);
            if agora_epoch_s() > criado_s + sessao.duration {
                let _ = e.banco.mfa_apagar(mfa_id).await;
                return Err(ErroApi(Erro::invalido("MFA session expired.")));
            }
            if sessao.record_id != cred.id {
                return Err(ErroApi(Erro::invalido("Invalid MFA session for user.")));
            }
            if sessao.method == "oauth2" {
                return Err(ErroApi(Erro::invalido(
                    "Second MFA factor must be different.",
                )));
            }
            let _ = e.banco.mfa_apagar(mfa_id).await;
        } else {
            let novo_mfa_id = e
                .banco
                .mfa_criar(&c.id, &cred.id, "oauth2", mfa_cfg.duration)
                .await?;
            return Ok(resposta_mfa(&novo_mfa_id));
        }
    }

    let (ip, user_agent) = extrair_ip_e_user_agent(&headers);
    processar_auth_origin_e_alerta(&e, &c, &record_id, Some(&cred.email), &ip, &user_agent).await;

    let registro = e.banco.registro(&c, &record_id, None).await?;
    let token = auth::emitir_tipo(
        &e.segredo,
        &cred.id,
        &c.id,
        &cred.token_key,
        auth::TIPO_AUTH,
        auth::VALIDADE_AUTH,
        None,
    )?;

    let meta = json!({
        "id": perfil.id,
        "name": perfil.name,
        "username": perfil.username,
        "email": perfil.email,
        "avatarUrl": perfil.avatar_url,
        "accessToken": token_resp.access_token,
        "refreshToken": token_resp.refresh_token,
        "isNew": is_new,
        "rawUser": perfil.raw_user
    });

    Ok(Json(json!({
        "token": token,
        "record": registro,
        "meta": meta
    }))
    .into_response())
}

async fn oauth2_redirect_html() -> impl IntoResponse {
    let html = r#"<!DOCTYPE html>
<html>
<head>
    <meta charset="utf-8">
    <title>OAuth2 redirect</title>
</head>
<body>
    <script>
        const params = new URLSearchParams(window.location.search);
        const data = {
            type: "oauth2Redirect",
            code: params.get("code") || "",
            state: params.get("state") || "",
            error: params.get("error") || "",
            url: window.location.href
        };
        if (window.opener) {
            window.opener.postMessage(data, "*");
            window.close();
        } else {
            document.body.innerText = "Auth completed. You can close this window.";
        }
    </script>
    <p>Auth completed. You can close this window.</p>
</body>
</html>"#;
    Html(html)
}

async fn listar_external_auths(
    State(e): State<Estado>,
    quem: Chamador,
    Path((nome, id)): Path<(String, String)>,
) -> Res<Json<Value>> {
    let c = colecao_auth(&e, nome).await?;
    if !quem.superusuario() {
        let Some((col, reg)) = quem.registro() else {
            return Err(nao_autorizado());
        };
        if col.id != c.id || reg["id"].as_str() != Some(&id) {
            return Err(nao_autorizado());
        }
    }
    let lista = e.banco.external_auth_listar(&c.id, &id).await?;
    Ok(Json(json!(lista)))
}

async fn desvincular_external_auth(
    State(e): State<Estado>,
    quem: Chamador,
    Path((nome, id, provider)): Path<(String, String, String)>,
) -> Res<StatusCode> {
    let c = colecao_auth(&e, nome).await?;
    if !quem.superusuario() {
        let Some((col, reg)) = quem.registro() else {
            return Err(nao_autorizado());
        };
        if col.id != c.id || reg["id"].as_str() != Some(&id) {
            return Err(nao_autorizado());
        }
    }
    e.banco
        .external_auth_apagar_por_provider(&c.id, &id, &provider)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn auth_origins_listar(
    State(e): State<Estado>,
    quem: Chamador,
    Path((colecao_nome, id)): Path<(String, String)>,
) -> Res<Json<Vec<AuthOriginRegistro>>> {
    let c = colecao_auth(&e, colecao_nome).await?;
    checar_dono_ou_superusuario(&quem, &c.id, &id)?;
    let origens = e.banco.auth_origins_listar(&c.id, &id).await?;
    Ok(Json(origens))
}

async fn auth_origins_apagar(
    State(e): State<Estado>,
    quem: Chamador,
    Path((colecao_nome, id, origin_id)): Path<(String, String, String)>,
) -> Res<StatusCode> {
    let c = colecao_auth(&e, colecao_nome).await?;
    checar_dono_ou_superusuario(&quem, &c.id, &id)?;
    e.banco.auth_origin_apagar(&origin_id, &c.id, &id).await?;
    Ok(StatusCode::NO_CONTENT)
}

fn checar_dono_ou_superusuario(
    quem: &Chamador,
    colecao_id_ou_nome: &str,
    record_id: &str,
) -> Res<()> {
    if quem.superusuario() {
        return Ok(());
    }
    if let Some((c, reg)) = quem.registro()
        && (c.id == colecao_id_ou_nome || c.name == colecao_id_ou_nome)
        && reg.get("id").and_then(Value::as_str) == Some(record_id)
    {
        return Ok(());
    }
    Err(ErroApi(Erro::Proibido(
        "You are not allowed to access this resource.".into(),
    )))
}

// ── Arquivos ─────────────────────────────────────────────────────────────

#[derive(Deserialize, Default)]
struct ParamsArquivo {
    download: Option<String>,
    /// Aceito e ignorado: a miniatura ainda não é gerada, então o original sai.
    thumb: Option<String>,
    token: Option<String>,
}

async fn arquivo_token(State(e): State<Estado>, quem: Chamador) -> Res<Json<Value>> {
    let (id, colecao, chave) = match &quem.identidade {
        Some(Identidade::Superusuario(su)) => (
            su.id.clone(),
            COLECAO_SUPERUSERS.to_string(),
            su.token_key.clone(),
        ),
        Some(Identidade::Registro { colecao, registro }) => {
            let id = registro
                .get("id")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            let cred = e
                .banco
                .auth_credenciais_por_id(colecao, &id)
                .await?
                .ok_or_else(token_invalido)?;
            (id, colecao.id.clone(), cred.token_key)
        }
        None => return Err(token_invalido()),
    };
    let token = auth::emitir_tipo(
        &e.segredo,
        &id,
        &colecao,
        &chave,
        TIPO_ARQUIVO,
        VALIDADE_ARQUIVO,
        None,
    )?;
    Ok(Json(json!({ "token": token })))
}

fn arquivo_ausente() -> Response {
    ErroApi(Erro::NaoEncontrado(
        "The requested resource wasn't found.".into(),
    ))
    .into_response()
}

async fn arquivo_baixar(
    State(e): State<Estado>,
    Path((nome, id, arquivo)): Path<(String, String, String)>,
    Query(p): Query<ParamsArquivo>,
    req: Request,
) -> Response {
    let _ = p.thumb;
    let Ok(c) = e.banco.colecao(&nome).await else {
        return arquivo_ausente();
    };
    let Ok(registro) = e.banco.registro(&c, &id, None).await else {
        return arquivo_ausente();
    };
    let Some(protegido) = arquivos::arquivo_do_registro(&c, &registro, &arquivo) else {
        return arquivo_ausente();
    };
    if protegido {
        let Some(token) = p.token.as_deref().filter(|t| !t.is_empty()) else {
            return arquivo_ausente();
        };
        let Ok(identidade) = identificar(&e, token, TIPO_ARQUIVO).await else {
            return arquivo_ausente();
        };
        let quem = Chamador::com_identidade(identidade);
        let Ok(regra) = regra_para(&quem, &c.view_rule) else {
            return arquivo_ausente();
        };
        let visivel = e
            .banco
            .visivel(&c, &id, regra.as_deref(), &quem.contexto())
            .await
            .unwrap_or(false);
        if !visivel {
            return arquivo_ausente();
        }
    }
    let storage = e.banco.storage().await;
    let thumb_param = p.thumb.clone();
    let colecao_id = c.id.clone();
    let id_reg = id.clone();
    let nome_arq = arquivo.clone();

    let bytes = if let Some(t) = thumb_param.as_deref().filter(|s| !s.is_empty())
        && let Some(spec) = arquivos::ThumbSpec::parse(t)
    {
        if let Some(cached) = storage.ler_thumb(&colecao_id, &id_reg, &nome_arq, t).await {
            cached
        } else {
            let Ok(orig_bytes) = storage.ler(&colecao_id, &id_reg, &nome_arq).await else {
                return arquivo_ausente();
            };
            let ext = nome_arq.rsplit_once('.').map(|(_, e)| e).unwrap_or("");
            if let Some(thumb_bytes) = arquivos::gerar_thumb(&orig_bytes, &spec, ext) {
                let _ = storage
                    .gravar_thumb(&colecao_id, &id_reg, &nome_arq, t, &thumb_bytes)
                    .await;
                thumb_bytes
            } else {
                orig_bytes
            }
        }
    } else {
        let Ok(b) = storage.ler(&colecao_id, &id_reg, &nome_arq).await else {
            return arquivo_ausente();
        };
        b
    };
    let faixa = req
        .headers()
        .get(RANGE)
        .and_then(|v| v.to_str().ok())
        .map(str::to_string);
    let (inicio, fim, parcial) = match fatia(bytes.len(), faixa.as_deref()) {
        Ok(v) => v,
        Err(PedidoFaixa::Invalida) => {
            return Response::builder()
                .status(StatusCode::RANGE_NOT_SATISFIABLE)
                .header(CONTENT_RANGE, format!("bytes */{}", bytes.len()))
                .body(axum::body::Body::empty())
                .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response());
        }
    };
    let pedaco = bytes[inicio..fim].to_vec();
    let mut resp = Response::builder()
        .status(if parcial {
            StatusCode::PARTIAL_CONTENT
        } else {
            StatusCode::OK
        })
        .header(CONTENT_TYPE, arquivos::tipo_por_extensao(&arquivo))
        .header(CONTENT_LENGTH, pedaco.len().to_string())
        .header("accept-ranges", "bytes");
    if parcial {
        let ultimo = fim.saturating_sub(1);
        resp = resp.header(
            CONTENT_RANGE,
            format!("bytes {inicio}-{ultimo}/{}", bytes.len()),
        );
    }
    if p.download.as_deref() == Some("1") {
        resp = resp.header(
            CONTENT_DISPOSITION,
            format!("attachment; filename=\"{arquivo}\""),
        );
    }
    let corpo = if req.method() == axum::http::Method::HEAD {
        axum::body::Body::empty()
    } else {
        axum::body::Body::from(pedaco)
    };
    resp.body(corpo)
        .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response())
}

enum PedidoFaixa {
    Invalida,
}

/// `(início, fim exclusivo, parcial)`. Sem cabeçalho, o arquivo inteiro.
fn fatia(len: usize, header: Option<&str>) -> Result<(usize, usize, bool), PedidoFaixa> {
    let Some(h) = header.map(str::trim).filter(|s| !s.is_empty()) else {
        return Ok((0, len, false));
    };
    let Some(spec) = h.strip_prefix("bytes=") else {
        return Err(PedidoFaixa::Invalida);
    };
    let spec = spec.split(',').next().unwrap_or(spec).trim();
    let Some((a, b)) = spec.split_once('-') else {
        return Err(PedidoFaixa::Invalida);
    };
    if len == 0 {
        return Err(PedidoFaixa::Invalida);
    }
    if a.is_empty() {
        let n: usize = b.parse().map_err(|_| PedidoFaixa::Invalida)?;
        if n == 0 {
            return Err(PedidoFaixa::Invalida);
        }
        let inicio = len.saturating_sub(n);
        return Ok((inicio, len, true));
    }
    let inicio: usize = a.parse().map_err(|_| PedidoFaixa::Invalida)?;
    if inicio >= len {
        return Err(PedidoFaixa::Invalida);
    }
    let ultimo = if b.is_empty() {
        len - 1
    } else {
        let fim: usize = b.parse().map_err(|_| PedidoFaixa::Invalida)?;
        fim.min(len - 1)
    };
    if ultimo < inicio {
        return Err(PedidoFaixa::Invalida);
    }
    Ok((inicio, ultimo + 1, true))
}

// ── Rotas ────────────────────────────────────────────────────────────────

/// `GET /api/health`, com o mesmo corpo do PocketBase.
async fn health() -> Json<Value> {
    Json(json!({ "code": 200, "message": "API is healthy.", "data": {} }))
}

async fn log_middleware(State(e): State<Estado>, req: Request, next: Next) -> Response {
    let method = req.method().to_string();
    let uri = req.uri().to_string();
    let inicio = std::time::Instant::now();

    let res = next.run(req).await;

    let duracao = inicio.elapsed().as_secs_f64();
    let status = res.status().as_u16();

    if uri.starts_with("/api/") && !uri.starts_with("/api/logs") {
        let level = if status >= 500 {
            8
        } else if status >= 400 {
            4
        } else {
            0
        };
        let banco = e.banco.clone();
        let msg = format!("{method} {uri} {status}");
        tokio::spawn(async move {
            let data = json!({
                "url": uri,
                "method": method,
                "status": status,
                "execTime": duracao,
            });
            let _ = banco.log_gravar(level, &msg, data).await;
        });
    }

    res
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
            "/api/collections/meta/dry-run-view",
            post(colecoes_dry_run_view),
        )
        .route("/api/collections/dry-run-view", post(colecoes_dry_run_view))
        .route(
            "/api/collections",
            get(colecoes_listar).post(colecoes_criar),
        )
        .route(
            "/api/collections/{colecao}",
            get(colecoes_ver)
                .patch(colecoes_atualizar)
                .delete(colecoes_apagar),
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
        .route(
            "/api/collections/{colecao}/auth-methods",
            get(obter_auth_methods),
        )
        .route("/api/collections/{colecao}/request-otp", post(pedir_otp))
        .route(
            "/api/collections/{colecao}/auth-with-otp",
            post(auth_com_otp),
        )
        .route(
            "/api/collections/{colecao}/auth-with-oauth2",
            post(auth_com_oauth2),
        )
        .route(
            "/api/oauth2-redirect",
            get(oauth2_redirect_html).post(oauth2_redirect_html),
        )
        .route(
            "/api/collections/{colecao}/records/{id}/external-auths",
            get(listar_external_auths),
        )
        .route(
            "/api/collections/{colecao}/records/{id}/external-auths/{provider}",
            delete(desvincular_external_auth),
        )
        .route(
            "/api/collections/{colecao}/records/{id}/auth-origins",
            get(auth_origins_listar),
        )
        .route(
            "/api/collections/{colecao}/records/{id}/auth-origins/{originId}",
            delete(auth_origins_apagar),
        )
        .route("/api/batch", post(lote::http))
        .route(
            "/api/realtime",
            get(realtime::conectar).post(realtime::inscrever),
        )
        .route("/api/files/token", post(arquivo_token))
        .route(
            "/api/files/{colecao}/{id}/{arquivo}",
            get(arquivo_baixar).head(arquivo_baixar),
        )
        .route(
            "/api/settings",
            get(settings_obter).patch(settings_atualizar),
        )
        .route("/api/settings/test/s3", post(settings_test_s3))
        .route("/api/settings/test/email", post(settings_test_email))
        .route(
            "/api/settings/apple/generate-client-secret",
            post(apple_generate_client_secret),
        )
        .route("/api/logs", get(logs_listar_api))
        .route("/api/logs/stats", get(logs_stats_api))
        .route("/api/logs/{id}", get(logs_obter_api))
        .route(
            "/api/backups",
            get(backups_listar_api).post(backups_criar_api),
        )
        .route("/api/backups/upload", post(backups_upload_api))
        .route(
            "/api/backups/{key}",
            get(backups_baixar_api).delete(backups_apagar_api),
        )
        .route("/api/backups/{key}/restore", post(backups_restaurar_api))
        .route("/api/crons", get(crons_listar_api))
        .route("/api/crons/{id}", post(cron_executar_api))
        .merge(painel::rotas())
        .layer(middleware::from_fn_with_state(e.clone(), log_middleware))
        .layer(middleware::from_fn_with_state(
            e.clone(),
            rate_limit::rate_limit_middleware,
        ))
        .layer(DefaultBodyLimit::max(MAX_CORPO))
        .with_state(e)
}

async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        if let Ok(mut sig) =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        {
            sig.recv().await;
        }
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }
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
    axum::serve(listener, router(estado))
        .with_graceful_shutdown(shutdown_signal())
        .await
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

    async fn chamar_com_headers(
        e: &Estado,
        metodo: &str,
        uri: &str,
        headers: &[(&str, &str)],
        corpo: Option<Value>,
    ) -> (StatusCode, Value, axum::http::HeaderMap) {
        let mut req = Request::builder().method(metodo).uri(uri);
        for (k, v) in headers {
            req = req.header(*k, *v);
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
        let resp_headers = resp.headers().clone();
        let bytes = resp.into_body().collect().await.unwrap().to_bytes();
        (
            status,
            serde_json::from_slice(&bytes).unwrap_or(Value::Null),
            resp_headers,
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
            "/_/new-collection",
            "/_/collections/qualquer",
            "/_/collections/qualquer/settings",
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

    #[tokio::test]
    async fn expand_na_listagem_e_no_registro() {
        let e = estado().await;
        let t = login(&e).await;
        let (s, autores) = chamar(
            &e,
            "POST",
            "/api/collections",
            Some(&t),
            Some(json!({
                "name": "autores",
                "fields": [{"name": "nome", "type": "text", "required": true}],
                "listRule": "",
                "viewRule": ""
            })),
        )
        .await;
        assert_eq!(s, StatusCode::OK, "{autores}");
        let (s, autor) = chamar(
            &e,
            "POST",
            "/api/collections/autores/records",
            Some(&t),
            Some(json!({"nome": "Ana"})),
        )
        .await;
        assert_eq!(s, StatusCode::OK, "{autor}");
        let (s, posts) = chamar(
            &e,
            "POST",
            "/api/collections",
            Some(&t),
            Some(json!({
                "name": "posts",
                "fields": [
                    {"name": "titulo", "type": "text", "required": true},
                    {
                        "name": "autor",
                        "type": "relation",
                        "collectionId": autores["id"],
                        "required": true
                    }
                ],
                "listRule": "",
                "viewRule": "",
                "createRule": ""
            })),
        )
        .await;
        assert_eq!(s, StatusCode::OK, "{posts}");
        assert_eq!(posts["fields"][1]["collectionId"], autores["id"]);
        let (s, post) = chamar(
            &e,
            "POST",
            "/api/collections/posts/records?expand=autor",
            None,
            Some(json!({"titulo": "Olá", "autor": autor["id"]})),
        )
        .await;
        assert_eq!(s, StatusCode::OK, "{post}");
        assert_eq!(post["expand"]["autor"]["nome"], json!("Ana"));
        let id = post["id"].as_str().unwrap();
        let (s, visto) = chamar(
            &e,
            "GET",
            &format!("/api/collections/posts/records/{id}?expand=autor"),
            None,
            None,
        )
        .await;
        assert_eq!(s, StatusCode::OK, "{visto}");
        assert_eq!(visto["expand"]["autor"]["id"], autor["id"]);
        let (s, lista) = chamar(
            &e,
            "GET",
            "/api/collections/posts/records?expand=autor",
            None,
            None,
        )
        .await;
        assert_eq!(s, StatusCode::OK, "{lista}");
        assert_eq!(lista["items"][0]["expand"]["autor"]["nome"], json!("Ana"));
        let (s, ruim) = chamar(
            &e,
            "GET",
            &format!("/api/collections/posts/records/{id}?expand=nao_existe"),
            None,
            None,
        )
        .await;
        assert_eq!(s, StatusCode::BAD_REQUEST, "{ruim}");
    }

    #[tokio::test]
    async fn view_e_collection_na_api() {
        let e = estado().await;
        let t = login(&e).await;
        let (s, _) = chamar(
            &e,
            "POST",
            "/api/collections",
            Some(&t),
            Some(json!({
                "name": "posts",
                "fields": [{"name": "titulo", "type": "text"}],
                "listRule": "",
                "viewRule": ""
            })),
        )
        .await;
        assert_eq!(s, StatusCode::OK);
        let (s, post) = chamar(
            &e,
            "POST",
            "/api/collections/posts/records",
            Some(&t),
            Some(json!({"titulo": "Ola"})),
        )
        .await;
        assert_eq!(s, StatusCode::OK, "{post}");

        let (s, vista) = chamar(
            &e,
            "POST",
            "/api/collections",
            Some(&t),
            Some(json!({
                "name": "titulos",
                "type": "view",
                "viewQuery": "SELECT id, titulo FROM posts",
                "listRule": "",
                "viewRule": ""
            })),
        )
        .await;
        assert_eq!(s, StatusCode::OK, "{vista}");
        assert_eq!(vista["viewQuery"], json!("SELECT id, titulo FROM posts"));
        assert!(vista.get("createRule").unwrap().is_null());

        let (s, lista) = chamar(&e, "GET", "/api/collections/titulos/records", None, None).await;
        assert_eq!(s, StatusCode::OK, "{lista}");
        assert_eq!(lista["totalItems"], json!(1));
        assert_eq!(lista["items"][0]["titulo"], json!("Ola"));

        let (s, recusa) = chamar(
            &e,
            "POST",
            "/api/collections/titulos/records",
            Some(&t),
            Some(json!({"titulo": "X"})),
        )
        .await;
        assert_eq!(s, StatusCode::BAD_REQUEST, "{recusa}");
        assert_eq!(recusa["message"], json!("The collection is not writable."));

        let (s, _) = chamar(
            &e,
            "GET",
            "/api/collections/posts/records?filter=@collection.posts.titulo%3D%22Ola%22",
            None,
            None,
        )
        .await;
        assert_eq!(s, StatusCode::FORBIDDEN);
        let (s, super_) = chamar(
            &e,
            "GET",
            "/api/collections/posts/records?filter=@collection.posts.titulo%3D%22Ola%22",
            Some(&t),
            None,
        )
        .await;
        assert_eq!(s, StatusCode::OK, "{super_}");
        assert_eq!(super_["totalItems"], json!(1));
    }

    async fn estado_disco() -> (Estado, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let e = Estado::novo(Banco::sqlite(dir.path()).await.unwrap())
            .await
            .unwrap();
        (e, dir)
    }

    fn parte(nome: &str, valor: &str) -> Vec<u8> {
        format!(
            "--ForjaBoundary\r\nContent-Disposition: form-data; name=\"{nome}\"\r\n\r\n{valor}\r\n"
        )
        .into_bytes()
    }

    fn parte_arquivo(campo: &str, arquivo: &str, tipo: &str, bytes: &[u8]) -> Vec<u8> {
        let mut out = format!(
            "--ForjaBoundary\r\nContent-Disposition: form-data; name=\"{campo}\"; filename=\"{arquivo}\"\r\nContent-Type: {tipo}\r\n\r\n"
        )
        .into_bytes();
        out.extend_from_slice(bytes);
        out.extend_from_slice(b"\r\n");
        out
    }

    async fn enviar_multipart(
        e: &Estado,
        uri: &str,
        token: Option<&str>,
        corpo: Vec<u8>,
    ) -> (StatusCode, Value) {
        let mut req = Request::builder().method("POST").uri(uri).header(
            "content-type",
            "multipart/form-data; boundary=ForjaBoundary",
        );
        if let Some(t) = token {
            req = req.header("authorization", t);
        }
        let resp = router(e.clone())
            .oneshot(req.body(Body::from(corpo)).unwrap())
            .await
            .unwrap();
        let status = resp.status();
        let bytes = resp.into_body().collect().await.unwrap().to_bytes();
        (
            status,
            serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        )
    }

    #[tokio::test]
    async fn arquivo_sobe_baixa_e_recusa_tipo() {
        let (e, _dir) = estado_disco().await;
        let t = login(&e).await;
        let (s, _) = chamar(
            &e,
            "POST",
            "/api/collections",
            Some(&t),
            Some(json!({
                "name": "posts",
                "fields": [
                    {"name": "titulo", "type": "text", "required": true},
                    {"name": "capa", "type": "file", "maxSize": 1000, "mimeTypes": ["image/png"]}
                ],
                "listRule": "",
                "viewRule": "",
                "createRule": ""
            })),
        )
        .await;
        assert_eq!(s, StatusCode::OK);

        let mut ruim = parte("titulo", "Sem figura");
        ruim.extend(parte_arquivo("capa", "nota.txt", "text/plain", b"abc"));
        ruim.extend(b"--ForjaBoundary--\r\n");
        let (s, v) = enviar_multipart(&e, "/api/collections/posts/records", None, ruim).await;
        assert_eq!(s, StatusCode::BAD_REQUEST, "{v}");
        assert_eq!(
            v["data"]["capa"]["code"],
            json!("validation_invalid_mime_type")
        );

        let png = b"png-bytes-12345";
        let mut corpo = parte("titulo", "Com figura");
        corpo.extend(parte_arquivo("capa", "Foto.PNG", "image/png", png));
        corpo.extend(b"--ForjaBoundary--\r\n");
        let (s, v) = enviar_multipart(&e, "/api/collections/posts/records", None, corpo).await;
        assert_eq!(s, StatusCode::OK, "{v}");
        let id = v["id"].as_str().unwrap().to_string();
        let nome = v["capa"].as_str().unwrap().to_string();
        assert!(nome.starts_with("foto_"), "{nome}");
        assert!(nome.ends_with(".png"), "{nome}");

        let resp = router(e.clone())
            .oneshot(
                Request::builder()
                    .uri(format!("/api/files/posts/{id}/{nome}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(
            resp.headers()
                .get("content-type")
                .and_then(|h| h.to_str().ok()),
            Some("image/png")
        );
        let baixado = resp.into_body().collect().await.unwrap().to_bytes();
        assert_eq!(&baixado[..], png);

        let resp = router(e.clone())
            .oneshot(
                Request::builder()
                    .uri(format!(
                        "/api/files/posts/{id}/{nome}?download=1&thumb=100x100"
                    ))
                    .header("range", "bytes=0-2")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::PARTIAL_CONTENT);
        let disp = resp
            .headers()
            .get("content-disposition")
            .and_then(|h| h.to_str().ok())
            .unwrap_or("");
        assert!(disp.contains("attachment"), "{disp}");
        let pedaco = resp.into_body().collect().await.unwrap().to_bytes();
        assert_eq!(&pedaco[..], &png[..3]);

        let resp = router(e.clone())
            .oneshot(
                Request::builder()
                    .uri(format!("/api/files/posts/{id}/{nome}"))
                    .header("range", "bytes=100-200")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::RANGE_NOT_SATISFIABLE);

        let (s, _) = chamar(
            &e,
            "GET",
            &format!("/api/files/posts/{id}/outro.png"),
            None,
            None,
        )
        .await;
        assert_eq!(s, StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn arquivo_protegido_exige_token_e_view_rule() {
        let (e, _dir) = estado_disco().await;
        let t = login(&e).await;
        let (s, _) = chamar(
            &e,
            "POST",
            "/api/collections",
            Some(&t),
            Some(json!({
                "name": "docs",
                "fields": [
                    {"name": "titulo", "type": "text"},
                    {"name": "anexo", "type": "file", "protected": true}
                ],
                "listRule": "",
                "viewRule": null,
                "createRule": ""
            })),
        )
        .await;
        assert_eq!(s, StatusCode::OK);
        let bytes = b"segredo";
        let mut corpo = parte("titulo", "Privado");
        corpo.extend(parte_arquivo("anexo", "segredo.txt", "text/plain", bytes));
        corpo.extend(b"--ForjaBoundary--\r\n");
        let (s, v) = enviar_multipart(&e, "/api/collections/docs/records", Some(&t), corpo).await;
        assert_eq!(s, StatusCode::OK, "{v}");
        let id = v["id"].as_str().unwrap();
        let nome = v["anexo"].as_str().unwrap();
        let uri = format!("/api/files/docs/{id}/{nome}");

        let (s, _) = chamar(&e, "GET", &uri, None, None).await;
        assert_eq!(s, StatusCode::NOT_FOUND);
        let (s, _) = chamar(&e, "GET", &format!("{uri}?token={t}"), None, None).await;
        assert_eq!(s, StatusCode::NOT_FOUND);

        let (s, _) = chamar(&e, "POST", "/api/files/token", None, None).await;
        assert_eq!(s, StatusCode::UNAUTHORIZED);
        let (s, tok) = chamar(&e, "POST", "/api/files/token", Some(&t), None).await;
        assert_eq!(s, StatusCode::OK, "{tok}");
        let file_token = tok["token"].as_str().unwrap();
        let resp = router(e.clone())
            .oneshot(
                Request::builder()
                    .uri(format!("{uri}?token={file_token}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let baixado = resp.into_body().collect().await.unwrap().to_bytes();
        assert_eq!(&baixado[..], bytes);
    }

    #[tokio::test]
    async fn lote_confirma_junto_e_desfaz_junto() {
        let e = estado().await;
        let t = login(&e).await;
        let (s, _) = chamar(
            &e,
            "POST",
            "/api/collections",
            Some(&t),
            Some(json!({
                "name": "posts",
                "fields": [{"name": "titulo", "type": "text", "required": true}],
                "listRule": "",
                "viewRule": "",
                "createRule": ""
            })),
        )
        .await;
        assert_eq!(s, StatusCode::OK);

        let mut rx = e.banco.assinar();
        let (s, v) = chamar(
            &e,
            "POST",
            "/api/batch",
            Some(&t),
            Some(json!({
                "requests": [
                    {"method": "POST", "url": "/api/collections/posts/records", "body": {"titulo": "A"}},
                    {"method": "POST", "url": "/api/collections/posts/records", "body": {}},
                    {"method": "GET", "url": "/api/collections/posts/records"}
                ]
            })),
        )
        .await;
        assert_eq!(s, StatusCode::BAD_REQUEST, "{v}");
        assert!(rx.try_recv().is_err(), "lote falho publicou evento");
        let (s, lista) = chamar(&e, "GET", "/api/collections/posts/records", None, None).await;
        assert_eq!(s, StatusCode::OK, "{lista}");
        assert_eq!(lista["totalItems"], json!(0));

        let (s, v) = chamar(
            &e,
            "POST",
            "/api/batch",
            Some(&t),
            Some(json!({
                "requests": [
                    {"method": "POST", "url": "/api/collections/posts/records", "body": {"titulo": "A"}},
                    {"method": "POST", "url": "/api/collections/posts/records", "body": {"titulo": "B"}},
                    {"method": "GET", "url": "/api/collections/posts/records?sort=-created"}
                ]
            })),
        )
        .await;
        assert_eq!(s, StatusCode::OK, "{v}");
        let itens = v.as_array().unwrap();
        assert_eq!(itens.len(), 3);
        assert_eq!(itens[0]["status"], json!(200));
        assert_eq!(itens[2]["body"]["totalItems"], json!(2));
        assert_eq!(
            rx.try_recv().unwrap().acao,
            forja_core::barramento::Acao::Criar
        );
        assert_eq!(
            rx.try_recv().unwrap().acao,
            forja_core::barramento::Acao::Criar
        );

        let (s, _) = chamar(
            &e,
            "POST",
            "/api/batch",
            Some(&t),
            Some(json!({"requests": []})),
        )
        .await;
        assert_eq!(s, StatusCode::BAD_REQUEST);
    }

    async fn quadro_sse(body: &mut axum::body::Body) -> Option<String> {
        let esperado = tokio::time::timeout(std::time::Duration::from_secs(2), body.frame()).await;
        let Ok(Some(frame)) = esperado else {
            return None;
        };
        let Ok(frame) = frame else {
            return None;
        };
        let Ok(data) = frame.into_data() else {
            return None;
        };
        Some(String::from_utf8(data.to_vec()).unwrap_or_default())
    }

    #[tokio::test]
    async fn sse_respeita_a_regra_da_colecao() {
        let e = estado().await;
        let t = login(&e).await;
        let (s, _) = chamar(
            &e,
            "POST",
            "/api/collections",
            Some(&t),
            Some(json!({
                "name": "posts",
                "fields": [{"name": "titulo", "type": "text"}],
                "createRule": ""
            })),
        )
        .await;
        assert_eq!(s, StatusCode::OK);

        let abrir = |e: &Estado| {
            let e = e.clone();
            async move {
                let resp = router(e)
                    .oneshot(
                        Request::builder()
                            .uri("/api/realtime")
                            .body(Body::empty())
                            .unwrap(),
                    )
                    .await
                    .unwrap();
                assert_eq!(resp.status(), StatusCode::OK);
                let mut body = resp.into_body();
                let texto = quadro_sse(&mut body).await.expect("PB_CONNECT");
                assert!(texto.contains("PB_CONNECT"), "{texto}");
                let client_id = texto.split("data:").nth(1).unwrap().trim().to_string();
                let id = serde_json::from_str::<Value>(&client_id).unwrap()["clientId"]
                    .as_str()
                    .unwrap()
                    .to_string();
                (id, body)
            }
        };
        let (id_visita, mut corpo_visita) = abrir(&e).await;
        let (id_su, mut corpo_su) = abrir(&e).await;

        let (s, _) = chamar(
            &e,
            "POST",
            "/api/realtime",
            None,
            Some(json!({"clientId": id_visita, "subscriptions": ["posts"]})),
        )
        .await;
        assert_eq!(s, StatusCode::NO_CONTENT);
        let (s, _) = chamar(
            &e,
            "POST",
            "/api/realtime",
            Some(&t),
            Some(json!({"clientId": id_su, "subscriptions": ["posts"]})),
        )
        .await;
        assert_eq!(s, StatusCode::NO_CONTENT);

        let (s, criado) = chamar(
            &e,
            "POST",
            "/api/collections/posts/records",
            Some(&t),
            Some(json!({"titulo": "Ao vivo"})),
        )
        .await;
        assert_eq!(s, StatusCode::OK, "{criado}");

        let su = quadro_sse(&mut corpo_su)
            .await
            .expect("evento do superusuário");
        assert!(su.contains("\"action\":\"create\""), "{su}");
        assert!(su.contains(criado["id"].as_str().unwrap()), "{su}");
        let visita =
            tokio::time::timeout(std::time::Duration::from_millis(200), corpo_visita.frame()).await;
        assert!(
            visita.is_err(),
            "visitante recebeu evento com listRule fechada"
        );
    }

    #[tokio::test]
    async fn patch_colecao_dry_run_view_e_filtro_algum_item() {
        let e = estado().await;
        let t = login(&e).await;

        // 1. Criar coleção com campo multi-select / json
        let (s, c) = chamar(
            &e,
            "POST",
            "/api/collections",
            Some(&t),
            Some(json!({
                "name": "artigos",
                "type": "base",
                "listRule": "",
                "viewRule": "",
                "fields": [
                    { "name": "titulo", "type": "text" },
                    { "name": "tags", "type": "select", "maxSelect": 5, "values": ["rust", "go", "web"] }
                ]
            })),
        )
        .await;
        assert_eq!(s, StatusCode::OK, "{c}");

        // 2. dry-run-view
        let (s, dry) = chamar(
            &e,
            "POST",
            "/api/collections/meta/dry-run-view",
            Some(&t),
            Some(json!({ "query": "SELECT id, titulo FROM artigos" })),
        )
        .await;
        assert_eq!(s, StatusCode::OK, "{dry}");
        assert!(
            dry["fields"]
                .as_array()
                .unwrap()
                .iter()
                .any(|f| f["name"] == "titulo")
        );

        // 3. PATCH /api/collections/artigos (adicionar campo 'resumo')
        let mut patch_c = c.clone();
        patch_c["fields"].as_array_mut().unwrap().push(json!({
            "name": "resumo",
            "type": "text"
        }));
        let (s, atualizada) = chamar(
            &e,
            "PATCH",
            "/api/collections/artigos",
            Some(&t),
            Some(patch_c),
        )
        .await;
        assert_eq!(s, StatusCode::OK, "{atualizada}");
        assert!(
            atualizada["fields"]
                .as_array()
                .unwrap()
                .iter()
                .any(|f| f["name"] == "resumo")
        );

        // 4. Criar registros com tags
        let (s, _) = chamar(
            &e,
            "POST",
            "/api/collections/artigos/records",
            Some(&t),
            Some(json!({
                "titulo": "Artigo 1",
                "tags": ["rust", "web"],
                "resumo": "Primeiro resumo"
            })),
        )
        .await;
        assert_eq!(s, StatusCode::OK);

        let (s, _) = chamar(
            &e,
            "POST",
            "/api/collections/artigos/records",
            Some(&t),
            Some(json!({
                "titulo": "Artigo 2",
                "tags": ["go"],
                "resumo": "Segundo resumo"
            })),
        )
        .await;
        assert_eq!(s, StatusCode::OK);

        // 5. Filtro com ?= (algum item é 'rust')
        let (s, lista) = chamar(
            &e,
            "GET",
            "/api/collections/artigos/records?filter=tags%20%3F%3D%20%27rust%27",
            None,
            None,
        )
        .await;
        assert_eq!(s, StatusCode::OK, "{lista}");
        let items = lista["items"].as_array().unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0]["titulo"], "Artigo 1");
    }

    #[tokio::test]
    async fn fields_e_excerpt_e_realtime_expand_fields() {
        let e = estado().await;
        let t = login(&e).await;

        let (s, _) = chamar(
            &e,
            "POST",
            "/api/collections",
            Some(&t),
            Some(json!({
                "name": "noticias",
                "type": "base",
                "listRule": "",
                "viewRule": "",
                "fields": [
                    { "name": "titulo", "type": "text" },
                    { "name": "conteudo", "type": "text" }
                ]
            })),
        )
        .await;
        assert_eq!(s, StatusCode::OK);

        let (s, criado) = chamar(
            &e,
            "POST",
            "/api/collections/noticias/records",
            Some(&t),
            Some(json!({
                "titulo": "Post Longo",
                "conteudo": "Este é um conteúdo muito longo para testar o excerpt."
            })),
        )
        .await;
        assert_eq!(s, StatusCode::OK);
        let id = criado["id"].as_str().unwrap();

        // 1. GET lista com fields e excerpt
        let (s, lista) = chamar(
            &e,
            "GET",
            "/api/collections/noticias/records?fields=id,titulo,conteudo:excerpt(10)",
            None,
            None,
        )
        .await;
        assert_eq!(s, StatusCode::OK);
        let item = &lista["items"][0];
        assert_eq!(item["titulo"], "Post Longo");
        assert_eq!(item["conteudo"], "Este é um ...");
        assert!(item.get("created").is_none());

        // 2. GET registro individual com fields
        let (s, reg) = chamar(
            &e,
            "GET",
            &format!("/api/collections/noticias/records/{id}?fields=titulo"),
            None,
            None,
        )
        .await;
        assert_eq!(s, StatusCode::OK);
        assert_eq!(reg["titulo"], "Post Longo");
        assert!(reg.get("id").is_none());
        assert!(reg.get("conteudo").is_none());
    }

    #[tokio::test]
    async fn painel_gerencia_colecoes() {
        let e = estado().await;
        login(&e).await;
        let cookie = entrar_no_painel(&e).await;

        // 1. Tela de nova coleção
        let (s, _, corpo) = pedir(
            &e,
            "GET",
            "/_/new-collection",
            &[("cookie", cookie.as_str())],
            "",
        )
        .await;
        assert_eq!(s, StatusCode::OK);
        assert!(corpo.contains("Nova Coleção"), "{corpo}");

        // 2. Fragmento htmx de linha de campo
        let (s, _, fragmento) = pedir(
            &e,
            "GET",
            "/_/htmx/campo-linha",
            &[("cookie", cookie.as_str())],
            "",
        )
        .await;
        assert_eq!(s, StatusCode::OK);
        assert!(fragmento.contains("linha-campo"), "{fragmento}");

        // 3. Criar coleção 'tarefas' via form
        let (s, h, _) = form_post(
            &e,
            "/_/new-collection",
            Some(&cookie),
            &[
                ("name", "tarefas"),
                ("type", "base"),
                ("list_rule_tipo", "public"),
                ("view_rule_tipo", "super"),
                ("field_name", "titulo"),
                ("field_type", "text"),
                ("field_required", "true"),
                ("field_name", "concluida"),
                ("field_type", "bool"),
                ("field_required", "false"),
            ],
        )
        .await;
        assert_eq!(s, StatusCode::SEE_OTHER);
        assert_eq!(h.get("location").unwrap(), "/_/collections/tarefas");

        // Verifica no banco
        let col = e.banco.colecao("tarefas").await.unwrap();
        assert_eq!(col.name, "tarefas");
        assert_eq!(col.list_rule, Some(String::new()));
        assert_eq!(col.view_rule, None);
        assert!(col.fields.iter().any(|f| f.name == "titulo" && f.required));
        assert!(
            col.fields
                .iter()
                .any(|f| f.name == "concluida" && !f.required)
        );

        // 4. Tela de configurações
        let (s, _, corpo) = pedir(
            &e,
            "GET",
            "/_/collections/tarefas/settings",
            &[("cookie", cookie.as_str())],
            "",
        )
        .await;
        assert_eq!(s, StatusCode::OK);
        assert!(corpo.contains("Configurações: tarefas"), "{corpo}");

        // 5. Salvar configurações: renomear para 'afazeres' e adicionar campo 'prazo'
        let (s, h, _) = form_post(
            &e,
            "/_/collections/tarefas/settings",
            Some(&cookie),
            &[
                ("name", "afazeres"),
                ("list_rule_tipo", "custom"),
                ("list_rule_expr", "@request.auth.id != ''"),
                ("view_rule_tipo", "public"),
                ("field_name", "prazo"),
                ("field_type", "date"),
                ("field_required", "false"),
            ],
        )
        .await;
        assert_eq!(s, StatusCode::SEE_OTHER);
        assert_eq!(h.get("location").unwrap(), "/_/collections/afazeres");

        let col_atualizada = e.banco.colecao("afazeres").await.unwrap();
        assert_eq!(col_atualizada.name, "afazeres");
        assert_eq!(
            col_atualizada.list_rule,
            Some("@request.auth.id != ''".to_string())
        );
        assert_eq!(col_atualizada.view_rule, Some(String::new()));
        assert!(col_atualizada.fields.iter().any(|f| f.name == "prazo"));

        // 6. Excluir coleção
        let (s, h, _) = form_post(
            &e,
            "/_/collections/afazeres/delete-collection",
            Some(&cookie),
            &[],
        )
        .await;
        assert_eq!(s, StatusCode::SEE_OTHER);
        assert_eq!(h.get("location").unwrap(), "/_/");

        assert!(e.banco.colecao("afazeres").await.is_err());
    }

    #[tokio::test]
    async fn api_filtro_macros_de_data_e_modificadores() {
        let e = estado().await;
        let admin = login(&e).await;

        let (s, _) = chamar(
            &e,
            "POST",
            "/api/collections",
            Some(&admin),
            Some(json!({
                "name": "artigos",
                "type": "base",
                "listRule": "",
                "fields": [
                    { "name": "titulo", "type": "text" },
                    { "name": "resumo", "type": "text" }
                ]
            })),
        )
        .await;
        assert_eq!(s, StatusCode::OK);

        // Cria 2 registros
        let (s, _) = chamar(
            &e,
            "POST",
            "/api/collections/artigos/records",
            Some(&admin),
            Some(json!({ "titulo": "Rust No Servidor", "resumo": "Excelente guia." })),
        )
        .await;
        assert_eq!(s, StatusCode::OK);

        let (s, _) = chamar(
            &e,
            "POST",
            "/api/collections/artigos/records",
            Some(&admin),
            Some(json!({ "titulo": "GO EM PRODUÇÃO" })),
        )
        .await;
        assert_eq!(s, StatusCode::OK);

        // 1. Filtro com :lower
        let (s, lista) = chamar(
            &e,
            "GET",
            "/api/collections/artigos/records?filter=titulo:lower%20%3D%20%27rust%20no%20servidor%27",
            None,
            None,
        )
        .await;
        assert_eq!(s, StatusCode::OK);
        assert_eq!(lista["totalItems"], 1);
        assert_eq!(lista["items"][0]["titulo"], "Rust No Servidor");

        // 2. Filtro com :isset
        let (s, lista) = chamar(
            &e,
            "GET",
            "/api/collections/artigos/records?filter=resumo:isset%20%3D%20true",
            None,
            None,
        )
        .await;
        assert_eq!(s, StatusCode::OK);
        assert_eq!(lista["totalItems"], 1);
        assert_eq!(lista["items"][0]["titulo"], "Rust No Servidor");

        let (s, lista) = chamar(
            &e,
            "GET",
            "/api/collections/artigos/records?filter=resumo:isset%20%3D%20false",
            None,
            None,
        )
        .await;
        assert_eq!(s, StatusCode::OK);
        assert_eq!(lista["totalItems"], 1);
        assert_eq!(lista["items"][0]["titulo"], "GO EM PRODUÇÃO");

        // 3. Filtro com macro de data @todayStart
        let (s, lista) = chamar(
            &e,
            "GET",
            "/api/collections/artigos/records?filter=created%20%3E%3D%20%40todayStart",
            None,
            None,
        )
        .await;
        assert_eq!(s, StatusCode::OK);
        assert_eq!(lista["totalItems"], 2);
    }

    #[tokio::test]
    async fn api_settings_e_s3_storage() {
        let e = estado().await;
        let admin = login(&e).await;

        // 1. Visitante sem token não pode ler ou alterar configurações
        let (s, _) = chamar(&e, "GET", "/api/settings", None, None).await;
        assert_eq!(s, StatusCode::FORBIDDEN);

        let (s, _) = chamar(
            &e,
            "PATCH",
            "/api/settings",
            None,
            Some(json!({ "appName": "Hacker" })),
        )
        .await;
        assert_eq!(s, StatusCode::FORBIDDEN);

        // 2. Superusuário lê configurações padrão
        let (s, cfg) = chamar(&e, "GET", "/api/settings", Some(&admin), None).await;
        assert_eq!(s, StatusCode::OK);
        assert_eq!(cfg["appName"], "Acme");
        assert_eq!(cfg["s3"]["enabled"], false);

        // 3. Superusuário atualiza configurações para habilitar S3
        let (s, cfg_atualizada) = chamar(
            &e,
            "PATCH",
            "/api/settings",
            Some(&admin),
            Some(json!({
                "appName": "Forjabase Prod",
                "appUrl": "https://forja.exemplo.com",
                "s3": {
                    "enabled": true,
                    "bucket": "meu-bucket",
                    "region": "sa-east-1",
                    "endpoint": "http://127.0.0.1:9000",
                    "accessKey": "key123",
                    "secret": "secret123",
                    "forcePathStyle": true
                }
            })),
        )
        .await;
        assert_eq!(s, StatusCode::OK);
        assert_eq!(cfg_atualizada["appName"], "Forjabase Prod");
        assert_eq!(cfg_atualizada["s3"]["enabled"], true);
        assert_eq!(cfg_atualizada["s3"]["bucket"], "meu-bucket");

        // Verifica que persistiu no banco
        let salva = e.banco.configuracoes().await;
        assert_eq!(salva.app_name, "Forjabase Prod");
        assert!(salva.s3.enabled);
        assert_eq!(salva.s3.bucket, "meu-bucket");

        // 4. Teste de conexão S3 com validação de campos vazios
        let (s, _) = chamar(
            &e,
            "POST",
            "/api/settings/test/s3",
            Some(&admin),
            Some(json!({
                "s3": {
                    "enabled": true,
                    "bucket": "",
                    "accessKey": "key",
                    "secret": "secret"
                }
            })),
        )
        .await;
        assert_eq!(s, StatusCode::BAD_REQUEST);

        // 5. Teste de conexão S3 com endpoint inacessível devolve erro claro
        let (s, resp) = chamar(
            &e,
            "POST",
            "/api/settings/test/s3",
            Some(&admin),
            Some(json!({
                "s3": {
                    "enabled": true,
                    "bucket": "b",
                    "region": "us-east-1",
                    "endpoint": "http://127.0.0.1:59999",
                    "accessKey": "key",
                    "secret": "secret"
                }
            })),
        )
        .await;
        assert_eq!(s, StatusCode::BAD_REQUEST);
        assert!(
            resp["message"]
                .as_str()
                .unwrap()
                .contains("Failed to connect")
        );
    }

    #[tokio::test]
    async fn api_settings_test_email() {
        let (e, memoria) = estado_com_memoria().await;
        let admin = login(&e).await;

        // 1. Visitante sem token recebe 403
        let (s, _) = chamar(
            &e,
            "POST",
            "/api/settings/test/email",
            None,
            Some(json!({ "email": "test@example.com", "template": "verification" })),
        )
        .await;
        assert_eq!(s, StatusCode::FORBIDDEN);

        // 2. Email inválido
        let (s, _) = chamar(
            &e,
            "POST",
            "/api/settings/test/email",
            Some(&admin),
            Some(json!({ "email": "invalido", "template": "verification" })),
        )
        .await;
        assert_eq!(s, StatusCode::BAD_REQUEST);

        // 3. Template desconhecido
        let (s, _) = chamar(
            &e,
            "POST",
            "/api/settings/test/email",
            Some(&admin),
            Some(json!({ "email": "test@example.com", "template": "desconhecido" })),
        )
        .await;
        assert_eq!(s, StatusCode::BAD_REQUEST);

        // 4. Envio bem-sucedido via Memoria
        let (s, _) = chamar(
            &e,
            "POST",
            "/api/settings/test/email",
            Some(&admin),
            Some(json!({ "email": "destinatario@exemplo.com", "template": "verification" })),
        )
        .await;
        assert_eq!(s, StatusCode::NO_CONTENT);

        let m = memoria.ultima().expect("email deveria ter sido enviado");
        assert_eq!(m.para, "destinatario@exemplo.com");
        assert!(m.assunto.contains("Acme"));
        assert!(m.corpo.contains("test_token_1234567890"));
    }

    #[tokio::test]
    async fn api_logs_e_stats() {
        let e = estado().await;
        let admin = login(&e).await;

        // Gera alguns logs na API
        let _ = chamar(&e, "GET", "/api/health", None, None).await;
        let _ = chamar(&e, "GET", "/api/collections", Some(&admin), None).await;

        // Permite o processamento assíncrono do log_middleware
        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

        // 1. Visitante é barrado
        let (s, _) = chamar(&e, "GET", "/api/logs", None, None).await;
        assert_eq!(s, StatusCode::FORBIDDEN);

        // 2. Superusuário lista logs
        let (s, lista) = chamar(&e, "GET", "/api/logs", Some(&admin), None).await;
        assert_eq!(s, StatusCode::OK);
        assert!(lista["totalItems"].as_u64().unwrap() >= 2);
        let primeiro = &lista["items"][0];
        let id_log = primeiro["id"].as_str().unwrap();

        // 3. Superusuário obtém log por ID
        let (s, item) = chamar(
            &e,
            "GET",
            &format!("/api/logs/{id_log}"),
            Some(&admin),
            None,
        )
        .await;
        assert_eq!(s, StatusCode::OK);
        assert_eq!(item["id"], id_log);
        assert!(item["data"]["status"].as_u64().is_some());

        // 4. Estatísticas horárias
        let (s, stats) = chamar(&e, "GET", "/api/logs/stats", Some(&admin), None).await;
        assert_eq!(s, StatusCode::OK);
        assert!(!stats.as_array().unwrap().is_empty());
    }

    #[tokio::test]
    async fn api_backups_e_crons() {
        let dir = tempfile::tempdir().unwrap();
        let banco = Banco::sqlite(dir.path()).await.unwrap();
        let e = Estado::novo(banco).await.unwrap();
        let admin = login(&e).await;

        // 1. Visitante é barrado
        let (s, _) = chamar(&e, "GET", "/api/backups", None, None).await;
        assert_eq!(s, StatusCode::FORBIDDEN);

        let (s, _) = chamar(&e, "GET", "/api/crons", None, None).await;
        assert_eq!(s, StatusCode::FORBIDDEN);

        // 2. Superusuário cria backup
        let (s, _) = chamar(
            &e,
            "POST",
            "/api/backups",
            Some(&admin),
            Some(json!({ "name": "backup_teste.zip" })),
        )
        .await;
        assert_eq!(s, StatusCode::NO_CONTENT);

        // 3. Superusuário lista backups
        let (s, lista) = chamar(&e, "GET", "/api/backups", Some(&admin), None).await;
        assert_eq!(s, StatusCode::OK);
        assert_eq!(lista[0]["key"], "backup_teste.zip");

        // 4. Baixar backup
        let (s, _) = chamar(
            &e,
            "GET",
            "/api/backups/backup_teste.zip",
            Some(&admin),
            None,
        )
        .await;
        assert_eq!(s, StatusCode::OK);

        // 5. Restaurar backup
        let (s, _) = chamar(
            &e,
            "POST",
            "/api/backups/backup_teste.zip/restore",
            Some(&admin),
            None,
        )
        .await;
        assert_eq!(s, StatusCode::NO_CONTENT);

        // 6. Apagar backup
        let (s, _) = chamar(
            &e,
            "DELETE",
            "/api/backups/backup_teste.zip",
            Some(&admin),
            None,
        )
        .await;
        assert_eq!(s, StatusCode::NO_CONTENT);

        // 7. Crons
        let (s, crons) = chamar(&e, "GET", "/api/crons", Some(&admin), None).await;
        assert_eq!(s, StatusCode::OK);
        assert!(!crons.as_array().unwrap().is_empty());

        let (s, _) = chamar(
            &e,
            "POST",
            "/api/crons/__pbLogsCleanup__",
            Some(&admin),
            None,
        )
        .await;
        assert_eq!(s, StatusCode::NO_CONTENT);
    }

    #[tokio::test]
    async fn painel_marco_0_5_paginas() {
        let e = estado().await;
        login(&e).await;
        let cookie = entrar_no_painel(&e).await;

        // 1. Logs
        let (s, _, corpo) = pedir(&e, "GET", "/_/logs", &[("cookie", cookie.as_str())], "").await;
        assert_eq!(s, StatusCode::OK);
        assert!(corpo.contains("Logs de Requisições"), "{corpo}");

        // 2. Crons
        let (s, _, corpo) = pedir(&e, "GET", "/_/crons", &[("cookie", cookie.as_str())], "").await;
        assert_eq!(s, StatusCode::OK);
        assert!(corpo.contains("Tarefas Agendadas"), "{corpo}");

        // Executar cron pelo painel
        let (s, h, _) = form_post(&e, "/_/crons/__pbLogsCleanup__/run", Some(&cookie), &[]).await;
        assert_eq!(s, StatusCode::SEE_OTHER);
        assert_eq!(h["location"], "/_/crons");

        // 3. Backups
        let (s, _, corpo) =
            pedir(&e, "GET", "/_/backups", &[("cookie", cookie.as_str())], "").await;
        assert_eq!(s, StatusCode::OK);
        assert!(corpo.contains("Backups"), "{corpo}");

        // 4. Configurações
        let (s, _, corpo) =
            pedir(&e, "GET", "/_/settings", &[("cookie", cookie.as_str())], "").await;
        assert_eq!(s, StatusCode::OK);
        assert!(corpo.contains("Configurações da Aplicação"), "{corpo}");

        // Salvar configurações pelo painel
        let (s, _, corpo) = form_post(
            &e,
            "/_/settings",
            Some(&cookie),
            &[
                ("appName", "Forjabase Prod"),
                ("appUrl", "https://forja.exemplo.com"),
                ("senderName", "Equipe Suporte"),
                ("senderAddress", "suporte@exemplo.com"),
                ("smtpEnabled", "true"),
                ("smtpHost", "smtp.sendgrid.net"),
                ("smtpPort", "587"),
            ],
        )
        .await;
        assert_eq!(s, StatusCode::OK);
        assert!(
            corpo.contains("Configurações salvas com sucesso!"),
            "{corpo}"
        );

        // Confere que persistiu no banco
        let cfg = e.banco.configuracoes().await;
        assert_eq!(cfg.app_name, "Forjabase Prod");
        assert_eq!(cfg.meta.sender_name, "Equipe Suporte");
        assert!(cfg.smtp.enabled);
        assert_eq!(cfg.smtp.host, "smtp.sendgrid.net");
    }

    fn colecao_auth_teste(nome: &str) -> Collection {
        Collection {
            name: nome.to_string(),
            kind: CollectionType::Auth,
            ..Default::default()
        }
    }

    #[tokio::test]
    async fn api_auth_methods_e_oauth2_redirect() {
        let mem = forja_core::correio::Memoria::default();
        let e = Estado::com_correio(
            Banco::sqlite_memoria().await.unwrap(),
            Arc::new(mem.clone()),
        )
        .await
        .unwrap();
        let _token = login(&e).await;

        // Cria coleção auth com OAuth2, OTP e MFA configurados
        let mut col = colecao_auth_teste("clientes");
        col.mfa = Some(forja_core::MfaConfig {
            enabled: true,
            duration: 1800,
            rule: String::new(),
        });
        col.otp = Some(forja_core::OtpConfig {
            enabled: true,
            duration: 180,
            length: 6,
            email_template: None,
        });
        col.oauth2 = Some(forja_core::OAuth2Config {
            enabled: true,
            providers: vec![forja_core::OAuth2ProviderConfig {
                name: "google".into(),
                display_name: "Google Login".into(),
                client_id: "google-client-id-123".into(),
                client_secret: "secret".into(),
                auth_url: String::new(),
                token_url: String::new(),
                user_api_url: String::new(),
                pkce: Some(true),
                extra: Map::new(),
            }],
            mapped_fields: None,
        });

        let _col = e.banco.criar_colecao(col).await.unwrap();

        // 1. Testa GET /api/collections/clientes/auth-methods
        let (s, res) = chamar(
            &e,
            "GET",
            "/api/collections/clientes/auth-methods",
            None,
            None,
        )
        .await;
        assert_eq!(s, StatusCode::OK);
        assert_eq!(res["mfa"]["enabled"], true);
        assert_eq!(res["mfa"]["duration"], 1800);
        assert_eq!(res["otp"]["enabled"], true);
        assert_eq!(res["otp"]["duration"], 180);
        assert_eq!(res["password"]["enabled"], true);
        assert_eq!(res["oauth2"]["enabled"], true);

        let providers = res["oauth2"]["providers"]
            .as_array()
            .expect("array de provedores");
        assert_eq!(providers.len(), 1);
        assert_eq!(providers[0]["name"], "google");
        assert_eq!(providers[0]["displayName"], "Google Login");
        assert_eq!(providers[0]["codeChallengeMethod"], "S256");
        assert!(providers[0]["state"].as_str().unwrap().len() >= 30);
        assert!(providers[0]["codeVerifier"].as_str().unwrap().len() >= 43);
        let auth_url = providers[0]["authUrl"].as_str().unwrap();
        assert!(auth_url.contains("client_id=google-client-id-123"));
        assert!(auth_url.contains("code_challenge="));

        // 2. Testa GET /api/oauth2-redirect
        let (s, _, corpo) = pedir(
            &e,
            "GET",
            "/api/oauth2-redirect?code=xyz&state=123",
            &[],
            "",
        )
        .await;
        assert_eq!(s, StatusCode::OK);
        assert!(corpo.contains("oauth2Redirect"), "{corpo}");
        assert!(corpo.contains("window.opener"), "{corpo}");
    }

    #[tokio::test]
    async fn api_otp_request_e_auth() {
        let mem = forja_core::correio::Memoria::default();
        let e = Estado::com_correio(
            Banco::sqlite_memoria().await.unwrap(),
            Arc::new(mem.clone()),
        )
        .await
        .unwrap();

        let mut col = colecao_auth_teste("membros");
        col.otp = Some(forja_core::OtpConfig {
            enabled: true,
            duration: 180,
            length: 6,
            email_template: None,
        });
        col.mfa = Some(forja_core::MfaConfig {
            enabled: false,
            duration: 1800,
            rule: String::new(),
        });
        let col = e.banco.criar_colecao(col).await.unwrap();

        // Cria usuário de teste com verified = false
        let user = e
            .banco
            .criar(
                &col,
                json!({
                    "email": "ana@exemplo.com",
                    "password": "senha-secreta-456",
                    "verified": false,
                })
                .as_object()
                .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(user["verified"], false);

        // 1. Pede OTP para e-mail inexistente (deve responder 200 com otpId sem enviar e-mail)
        let (s, res_fake) = chamar(
            &e,
            "POST",
            "/api/collections/membros/request-otp",
            None,
            Some(json!({ "email": "fantasma@exemplo.com" })),
        )
        .await;
        assert_eq!(s, StatusCode::OK);
        assert!(res_fake["otpId"].as_str().is_some());
        assert!(mem.ultima().is_none());

        // 2. Pede OTP para usuário existente
        let (s, res) = chamar(
            &e,
            "POST",
            "/api/collections/membros/request-otp",
            None,
            Some(json!({ "email": "ana@exemplo.com" })),
        )
        .await;
        assert_eq!(s, StatusCode::OK);
        let otp_id = res["otpId"].as_str().unwrap().to_string();

        let email_enviado = mem.ultima().expect("e-mail disparado");
        assert_eq!(email_enviado.para, "ana@exemplo.com");
        let codigo = email_enviado
            .corpo
            .split("Your one-time password is: ")
            .nth(1)
            .unwrap()
            .split_whitespace()
            .next()
            .unwrap();
        assert_eq!(codigo.len(), 6);

        // 3. Tenta autenticar com código errado
        let (s_err, _) = chamar(
            &e,
            "POST",
            "/api/collections/membros/auth-with-otp",
            None,
            Some(json!({ "otpId": otp_id, "password": "000000" })),
        )
        .await;
        assert_eq!(s_err, StatusCode::BAD_REQUEST);

        // 4. Autentica com o código correto
        let (s_ok, res_auth) = chamar(
            &e,
            "POST",
            "/api/collections/membros/auth-with-otp",
            None,
            Some(json!({ "otpId": otp_id, "password": codigo })),
        )
        .await;
        assert_eq!(s_ok, StatusCode::OK, "{res_auth}");
        assert!(res_auth["token"].as_str().is_some());
        assert_eq!(res_auth["record"]["email"], "ana@exemplo.com");
        assert_eq!(
            res_auth["record"]["verified"], true,
            "OTP valida e-mail automaticamente"
        );

        // 5. Tenta reutilizar o mesmo OTP (uso único)
        let (s_reuse, _) = chamar(
            &e,
            "POST",
            "/api/collections/membros/auth-with-otp",
            None,
            Some(json!({ "otpId": otp_id, "password": codigo })),
        )
        .await;
        assert_eq!(s_reuse, StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn api_mfa_flow() {
        let mem = forja_core::correio::Memoria::default();
        let e = Estado::com_correio(
            Banco::sqlite_memoria().await.unwrap(),
            Arc::new(mem.clone()),
        )
        .await
        .unwrap();

        let mut col = colecao_auth_teste("cofre");
        col.mfa = Some(forja_core::MfaConfig {
            enabled: true,
            duration: 1800,
            rule: String::new(),
        });
        col.otp = Some(forja_core::OtpConfig {
            enabled: true,
            duration: 180,
            length: 6,
            email_template: None,
        });
        let col = e.banco.criar_colecao(col).await.unwrap();

        let _user = e
            .banco
            .criar(
                &col,
                json!({
                    "email": "beto@exemplo.com",
                    "password": "senha-muito-segura-123",
                })
                .as_object()
                .unwrap(),
            )
            .await
            .unwrap();

        // 1. Tenta login por senha: com MFA ativo, retorna 401 com mfaId
        let (s, res_mfa) = chamar(
            &e,
            "POST",
            "/api/collections/cofre/auth-with-password",
            None,
            Some(json!({
                "identity": "beto@exemplo.com",
                "password": "senha-muito-segura-123"
            })),
        )
        .await;
        assert_eq!(s, StatusCode::UNAUTHORIZED);
        let mfa_id = res_mfa["mfaId"]
            .as_str()
            .expect("mfaId retornado no 401")
            .to_string();

        // 2. Pede segundo fator (OTP)
        let (s, res_otp) = chamar(
            &e,
            "POST",
            "/api/collections/cofre/request-otp",
            None,
            Some(json!({ "email": "beto@exemplo.com" })),
        )
        .await;
        assert_eq!(s, StatusCode::OK);
        let otp_id = res_otp["otpId"].as_str().unwrap().to_string();

        let email_msg = mem.ultima().expect("e-mail com OTP disparado");
        let codigo = email_msg
            .corpo
            .split("Your one-time password is: ")
            .nth(1)
            .unwrap()
            .split_whitespace()
            .next()
            .unwrap();

        // 3. Tenta reutilizar a mesma senha como segundo fator (deve falhar)
        let (s_same, _) = chamar(
            &e,
            "POST",
            "/api/collections/cofre/auth-with-password",
            None,
            Some(json!({
                "identity": "beto@exemplo.com",
                "password": "senha-muito-segura-123",
                "mfaId": mfa_id,
            })),
        )
        .await;
        assert_eq!(s_same, StatusCode::BAD_REQUEST);

        // 4. Conclui com o segundo fator diferente (OTP + mfaId)
        let (s_ok, res_final) = chamar(
            &e,
            "POST",
            "/api/collections/cofre/auth-with-otp",
            None,
            Some(json!({
                "otpId": otp_id,
                "password": codigo,
                "mfaId": mfa_id,
            })),
        )
        .await;
        assert_eq!(s_ok, StatusCode::OK, "{res_final}");
        assert!(res_final["token"].as_str().is_some());
        assert_eq!(res_final["record"]["email"], "beto@exemplo.com");
    }

    #[tokio::test]
    async fn api_oauth2_auth_e_external_auths() {
        // Mock server OAuth2 local
        let mock_app = axum::Router::new()
            .route(
                "/token",
                post(|| async {
                    axum::Json(json!({
                        "access_token": "token-oauth2-mock-123",
                        "token_type": "Bearer"
                    }))
                }),
            )
            .route(
                "/userinfo",
                get(|| async {
                    axum::Json(json!({
                        "id": 888777,
                        "login": "devrust",
                        "name": "Dev Rust",
                        "email": "devrust@github.com",
                        "avatar_url": "https://avatars.githubusercontent.com/u/888777"
                    }))
                }),
            );

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let _ = axum::serve(listener, mock_app).await;
        });

        let base_mock = format!("http://127.0.0.1:{}", addr.port());

        let mem = forja_core::correio::Memoria::default();
        let e = Estado::com_correio(
            Banco::sqlite_memoria().await.unwrap(),
            Arc::new(mem.clone()),
        )
        .await
        .unwrap();

        let mut col = colecao_auth_teste("desenvolvedores");
        col.oauth2 = Some(forja_core::OAuth2Config {
            enabled: true,
            providers: vec![forja_core::OAuth2ProviderConfig {
                name: "github".into(),
                display_name: "GitHub".into(),
                client_id: "client-gh-1".into(),
                client_secret: "secret-gh-1".into(),
                auth_url: format!("{base_mock}/auth"),
                token_url: format!("{base_mock}/token"),
                user_api_url: format!("{base_mock}/userinfo"),
                pkce: Some(true),
                extra: Map::new(),
            }],
            mapped_fields: None,
        });

        let _col = e.banco.criar_colecao(col).await.unwrap();

        // 1. Faz login com OAuth2 (usuário novo)
        let (s, res) = chamar(
            &e,
            "POST",
            "/api/collections/desenvolvedores/auth-with-oauth2",
            None,
            Some(json!({
                "provider": "github",
                "code": "code-mock-xyz",
                "redirectUrl": "http://127.0.0.1:8090/api/oauth2-redirect",
            })),
        )
        .await;

        assert_eq!(s, StatusCode::OK, "{res}");
        let user_token = res["token"].as_str().unwrap().to_string();
        let user_id = res["record"]["id"].as_str().unwrap().to_string();
        assert_eq!(res["record"]["email"], "devrust@github.com");
        assert_eq!(res["record"]["verified"], true);
        assert_eq!(res["meta"]["isNew"], true);
        assert_eq!(res["meta"]["id"], "888777");
        assert_eq!(res["meta"]["username"], "devrust");
        assert_eq!(res["meta"]["accessToken"], "token-oauth2-mock-123");

        // 2. Lista vínculos externos (external-auths)
        let auth_hdr = format!("Bearer {user_token}");
        let (s, ext_list) = chamar(
            &e,
            "GET",
            &format!("/api/collections/desenvolvedores/records/{user_id}/external-auths"),
            Some(&auth_hdr),
            None,
        )
        .await;
        assert_eq!(s, StatusCode::OK, "{ext_list}");
        let lista = ext_list.as_array().unwrap();
        assert_eq!(lista.len(), 1);
        assert_eq!(lista[0]["provider"], "github");
        assert_eq!(lista[0]["providerId"], "888777");

        // 3. Login novamente com OAuth2 (usuário existente)
        let (s_again, res_again) = chamar(
            &e,
            "POST",
            "/api/collections/desenvolvedores/auth-with-oauth2",
            None,
            Some(json!({
                "provider": "github",
                "code": "code-mock-xyz",
            })),
        )
        .await;
        assert_eq!(s_again, StatusCode::OK, "{res_again}");
        assert_eq!(res_again["meta"]["isNew"], false);
        assert_eq!(res_again["record"]["id"], user_id);

        // 4. Desvincula o provedor externo
        let (s_del, _) = chamar(
            &e,
            "DELETE",
            &format!("/api/collections/desenvolvedores/records/{user_id}/external-auths/github"),
            Some(&auth_hdr),
            None,
        )
        .await;
        assert_eq!(s_del, StatusCode::NO_CONTENT);

        // Confere que a lista agora está vazia
        let (s, ext_list_vazia) = chamar(
            &e,
            "GET",
            &format!("/api/collections/desenvolvedores/records/{user_id}/external-auths"),
            Some(&auth_hdr),
            None,
        )
        .await;
        assert_eq!(s, StatusCode::OK);
        assert_eq!(ext_list_vazia.as_array().unwrap().len(), 0);
    }

    #[tokio::test]
    async fn api_rate_limit_middleware() {
        let e = estado().await;
        let admin = login(&e).await;

        // Ativa o rate limiting nas configurações: max 2 requisições por 60s em /api/health
        let (s, _) = chamar(
            &e,
            "PATCH",
            "/api/settings",
            Some(&admin),
            Some(json!({
                "rateLimits": {
                    "enabled": true,
                    "rules": [
                        {
                            "label": "/api/health",
                            "audience": "",
                            "duration": 60,
                            "maxRequests": 2
                        }
                    ]
                }
            })),
        )
        .await;
        assert_eq!(s, StatusCode::OK);

        // 1ª requisição permitida
        let (s1, _, _) = chamar_com_headers(&e, "GET", "/api/health", &[], None).await;
        assert_eq!(s1, StatusCode::OK);

        // 2ª requisição permitida
        let (s2, _, _) = chamar_com_headers(&e, "GET", "/api/health", &[], None).await;
        assert_eq!(s2, StatusCode::OK);

        // 3ª requisição bloqueada: 429 Too Many Requests com Retry-After
        let (s3, v3, h3) = chamar_com_headers(&e, "GET", "/api/health", &[], None).await;
        assert_eq!(s3, StatusCode::TOO_MANY_REQUESTS);
        assert_eq!(v3["code"], 429);
        assert_eq!(v3["message"], "Too Many Requests.");
        assert!(h3.contains_key("retry-after"));

        // Rota não afetada (/api/settings) continua respondendo 200 com admin
        let (sh, _) = chamar(&e, "GET", "/api/settings", Some(&admin), None).await;
        assert_eq!(sh, StatusCode::OK);
    }

    #[tokio::test]
    async fn api_apple_generate_client_secret() {
        let e = estado().await;
        let admin = login(&e).await;

        let pem = "-----BEGIN PRIVATE KEY-----\n\
MIGHAgEAMBMGByqGSM49AgEGCCqGSM49AwEHBG0wawIBAQQglbUl3Ab4lTleMGWQ\n\
wNmWknZqnbDvkmVzBWKOaVVfu+6hRANCAATUv2JuIgcVNotB/RyDLw7ByDHB9Uhq\n\
QZsW/88VekgRw1uMBJnAOVmLF6rgxw9BFHjwoDinr9grT9ylHAqQb5kj\n\
-----END PRIVATE KEY-----";

        // 1. Visitante anônimo recebe 403 Forbidden
        let (s, _) = chamar(
            &e,
            "POST",
            "/api/settings/apple/generate-client-secret",
            None,
            Some(json!({
                "clientId": "com.exemplo.app",
                "teamId": "ABC12345",
                "keyId": "KEY12345",
                "privateKey": pem
            })),
        )
        .await;
        assert_eq!(s, StatusCode::FORBIDDEN);

        // 2. Superusuário com campos faltantes recebe 400 Bad Request
        let (s_bad, v_bad) = chamar(
            &e,
            "POST",
            "/api/settings/apple/generate-client-secret",
            Some(&admin),
            Some(json!({
                "clientId": "",
                "teamId": "",
                "keyId": "",
                "privateKey": ""
            })),
        )
        .await;
        assert_eq!(s_bad, StatusCode::BAD_REQUEST);
        assert!(v_bad["data"].get("clientId").is_some());

        // 3. Superusuário com chave válida gera o client secret JWT
        let (s_ok, v_ok) = chamar(
            &e,
            "POST",
            "/api/settings/apple/generate-client-secret",
            Some(&admin),
            Some(json!({
                "clientId": "com.exemplo.app",
                "teamId": "ABC12345",
                "keyId": "KEY12345",
                "privateKey": pem,
                "duration": 3600
            })),
        )
        .await;
        assert_eq!(s_ok, StatusCode::OK, "{v_ok}");
        let secret = v_ok["secret"].as_str().unwrap();
        assert!(
            secret.starts_with("eyJ"),
            "secret deve ser um JWT: {secret}"
        );
    }

    #[tokio::test]
    async fn api_auth_origins_e_alerta_novo_dispositivo() {
        let (e, memoria) = estado_com_memoria().await;
        let admin = login(&e).await;

        // 1. Cria uma coleção de auth com authAlert ativado
        let (s_col, c_res) = chamar(
            &e,
            "POST",
            "/api/collections",
            Some(&admin),
            Some(json!({
                "name": "colaboradores",
                "type": "auth",
                "authAlert": {
                    "enabled": true
                }
            })),
        )
        .await;
        assert_eq!(s_col, StatusCode::OK, "{c_res}");

        // 2. Cria um usuário
        let (s_u, u_res) = chamar(
            &e,
            "POST",
            "/api/collections/colaboradores/records",
            Some(&admin),
            Some(json!({
                "email": "colaborador@exemplo.com",
                "password": "senha-segura-123",
                "passwordConfirm": "senha-segura-123"
            })),
        )
        .await;
        assert_eq!(s_u, StatusCode::OK, "{u_res}");
        let user_id = u_res["id"].as_str().unwrap().to_string();

        // Limpa mensagens anteriores
        memoria.0.lock().unwrap().clear();

        // 3. Primeiro login: dispositivo inicial (IP 192.168.1.50, User-Agent 'Terminal Alpha')
        // Não envia email de alerta no primeiro login (origem inicial)
        let (s_l1, v_l1, _) = chamar_com_headers(
            &e,
            "POST",
            "/api/collections/colaboradores/auth-with-password",
            &[
                ("x-real-ip", "192.168.1.50"),
                ("user-agent", "Terminal Alpha"),
            ],
            Some(json!({
                "identity": "colaborador@exemplo.com",
                "password": "senha-segura-123"
            })),
        )
        .await;
        assert_eq!(s_l1, StatusCode::OK, "{v_l1}");
        let user_token = v_l1["token"].as_str().unwrap().to_string();
        assert_eq!(memoria.0.lock().unwrap().len(), 0);

        // 4. Segundo login a partir de um NOVO dispositivo (IP 203.0.113.195, User-Agent 'Terminal Beta')
        // Deve disparar o email de alerta de novo dispositivo
        let (s_l2, v_l2, _) = chamar_com_headers(
            &e,
            "POST",
            "/api/collections/colaboradores/auth-with-password",
            &[
                ("x-real-ip", "203.0.113.195"),
                ("user-agent", "Terminal Beta"),
            ],
            Some(json!({
                "identity": "colaborador@exemplo.com",
                "password": "senha-segura-123"
            })),
        )
        .await;
        assert_eq!(s_l2, StatusCode::OK, "{v_l2}");

        let enviados = memoria.0.lock().unwrap().clone();
        assert_eq!(enviados.len(), 1);
        assert_eq!(enviados[0].para, "colaborador@exemplo.com");
        assert!(enviados[0].assunto.contains("Login from a new device"));

        // 5. Lista as 2 origens do usuário autenticado
        let (s_orig, v_orig) = chamar(
            &e,
            "GET",
            &format!("/api/collections/colaboradores/records/{user_id}/auth-origins"),
            Some(&user_token),
            None,
        )
        .await;
        assert_eq!(s_orig, StatusCode::OK, "{v_orig}");
        let arr = v_orig.as_array().unwrap();
        assert_eq!(arr.len(), 2);
        let origin_id = arr[0]["id"].as_str().unwrap().to_string();

        // 6. Login novamente a partir do segundo dispositivo (já conhecido) -> Não envia alerta repetido
        memoria.0.lock().unwrap().clear();
        let (s_l3, _, _) = chamar_com_headers(
            &e,
            "POST",
            "/api/collections/colaboradores/auth-with-password",
            &[
                ("x-real-ip", "203.0.113.195"),
                ("user-agent", "Terminal Beta"),
            ],
            Some(json!({
                "identity": "colaborador@exemplo.com",
                "password": "senha-segura-123"
            })),
        )
        .await;
        assert_eq!(s_l3, StatusCode::OK);
        assert_eq!(memoria.0.lock().unwrap().len(), 0);

        // 7. Apaga uma das origens de autenticação
        let (s_del, _) = chamar(
            &e,
            "DELETE",
            &format!("/api/collections/colaboradores/records/{user_id}/auth-origins/{origin_id}"),
            Some(&user_token),
            None,
        )
        .await;
        assert_eq!(s_del, StatusCode::NO_CONTENT);

        // Origens agora têm 1 item
        let (s_orig2, v_orig2) = chamar(
            &e,
            "GET",
            &format!("/api/collections/colaboradores/records/{user_id}/auth-origins"),
            Some(&user_token),
            None,
        )
        .await;
        assert_eq!(s_orig2, StatusCode::OK);
        assert_eq!(v_orig2.as_array().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn api_regras_com_macros_data_headers_query_method() {
        let e = estado().await;
        let admin = login(&e).await;

        // Cria coleção com regras usando @request.data, @request.headers e @request.method
        let (s_col, _) = chamar(
            &e,
            "POST",
            "/api/collections",
            Some(&admin),
            Some(json!({
                "name": "pedidos",
                "type": "base",
                "fields": [
                    { "name": "cliente", "type": "text", "required": true },
                    { "name": "total", "type": "number" },
                    { "name": "status", "type": "text" }
                ],
                "createRule": "@request.data.total > 100 && @request.headers.x_origem = 'mobile' && @request.method = 'POST'",
                "updateRule": "@request.data.status != ''"
            })),
        )
        .await;
        assert_eq!(s_col, StatusCode::OK);

        // 1. Tenta criar sem o cabeçalho x-origem: recusado pela regra
        let (s_falha1, _, _) = chamar_com_headers(
            &e,
            "POST",
            "/api/collections/pedidos/records",
            &[],
            Some(json!({
                "cliente": "Empresa A",
                "total": 250
            })),
        )
        .await;
        assert_eq!(s_falha1, StatusCode::BAD_REQUEST);

        // 2. Tenta criar com total <= 100: recusado pela regra
        let (s_falha2, _, _) = chamar_com_headers(
            &e,
            "POST",
            "/api/collections/pedidos/records",
            &[("x-origem", "mobile")],
            Some(json!({
                "cliente": "Empresa A",
                "total": 50
            })),
        )
        .await;
        assert_eq!(s_falha2, StatusCode::BAD_REQUEST);

        // 3. Cria com total > 100 e cabeçalho x-origem: aceito!
        let (s_ok, v_ok, _) = chamar_com_headers(
            &e,
            "POST",
            "/api/collections/pedidos/records",
            &[("x-origem", "mobile")],
            Some(json!({
                "cliente": "Empresa A",
                "total": 250,
                "status": "novo"
            })),
        )
        .await;
        assert_eq!(s_ok, StatusCode::OK, "{v_ok}");
        let rec_id = v_ok["id"].as_str().unwrap().to_string();

        // 4. Atualiza registro com status preenchido: aceito!
        let (s_up_ok, v_up_ok, _) = chamar_com_headers(
            &e,
            "PATCH",
            &format!("/api/collections/pedidos/records/{rec_id}"),
            &[],
            Some(json!({
                "status": "pago"
            })),
        )
        .await;
        assert_eq!(s_up_ok, StatusCode::OK, "{v_up_ok}");
        assert_eq!(v_up_ok["status"], "pago");
    }
}
