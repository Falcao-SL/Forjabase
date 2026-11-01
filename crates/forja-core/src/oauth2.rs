//! Configuração e fluxos de OAuth2, OTP e MFA.
//!
//! Compatível com as rotas e convenções do PocketBase:
//! - PKCE (S256) em provedores OAuth2
//! - Resposta de `auth-methods` com URLs prontas de autorização
//! - OTP por e-mail com tempo de expiração configurável
//! - MFA em duas etapas com id de sessão temporário

use crate::erro::{Erro, Resultado};
use crate::id;
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use rand::RngExt;
use reqwest::header::{ACCEPT, AUTHORIZATION, HeaderMap, HeaderValue, USER_AGENT};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};
use std::collections::HashMap;

/// Gera o par de chaves PKCE (code_verifier, code_challenge) usando S256.
pub fn gerar_pkce() -> (String, String) {
    let verifier = id::aleatorio(64);
    let mut hasher = Sha256::new();
    hasher.update(verifier.as_bytes());
    let hash = hasher.finalize();
    let challenge = URL_SAFE_NO_PAD.encode(hash);
    (verifier, challenge)
}

/// Gera um estado aleatório de 32 caracteres para proteção contra CSRF.
pub fn gerar_state() -> String {
    id::aleatorio(32)
}

/// Gera um código numérico para OTP de uso único.
pub fn gerar_codigo_otp(tamanho: usize) -> String {
    let mut r = rand::rng();
    let digitos: String = (0..tamanho)
        .map(|_| {
            let n = r.random_range(0..10);
            char::from_digit(n, 10).unwrap_or('0')
        })
        .collect();
    digitos
}

#[derive(Debug, Serialize)]
struct AppleClaims {
    iss: String,
    sub: String,
    aud: String,
    iat: u64,
    exp: u64,
}

/// Gera o segredo de cliente para Sign in with Apple (JWT assinado com ES256).
pub fn gerar_apple_client_secret(
    client_id: &str,
    team_id: &str,
    key_id: &str,
    private_key_pem: &str,
    duration_secs: u64,
) -> Resultado<String> {
    let mut erros = serde_json::Map::new();
    if client_id.trim().is_empty() {
        erros.insert(
            "clientId".into(),
            serde_json::json!({
                "code": "validation_required",
                "message": "Missing required value."
            }),
        );
    }
    if team_id.trim().is_empty() {
        erros.insert(
            "teamId".into(),
            serde_json::json!({
                "code": "validation_required",
                "message": "Missing required value."
            }),
        );
    }
    if key_id.trim().is_empty() {
        erros.insert(
            "keyId".into(),
            serde_json::json!({
                "code": "validation_required",
                "message": "Missing required value."
            }),
        );
    }
    if private_key_pem.trim().is_empty() {
        erros.insert(
            "privateKey".into(),
            serde_json::json!({
                "code": "validation_required",
                "message": "Missing required value."
            }),
        );
    }
    if !erros.is_empty() {
        return Err(Erro::Invalido {
            mensagem: "Failed to generate Apple client secret.".into(),
            campos: erros,
        });
    }

    let duracao = duration_secs.clamp(1, 15_777_000); // máx ~6 meses
    let now_s = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    let mut header = jsonwebtoken::Header::new(jsonwebtoken::Algorithm::ES256);
    header.kid = Some(key_id.to_string());

    let claims = AppleClaims {
        iss: team_id.to_string(),
        sub: client_id.to_string(),
        aud: "https://appleid.apple.com".into(),
        iat: now_s,
        exp: now_s + duracao,
    };

    let key = jsonwebtoken::EncodingKey::from_ec_pem(private_key_pem.as_bytes())
        .map_err(|e| Erro::invalido(format!("Invalid Apple private key: {e}")))?;

    jsonwebtoken::encode(&header, &claims, &key)
        .map_err(|e| Erro::invalido(format!("Failed to sign Apple secret: {e}")))
}

/// Configuração de um provedor OAuth2 na coleção de autenticação.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OAuth2ProviderConfig {
    pub name: String,
    #[serde(default)]
    pub display_name: String,
    #[serde(default)]
    pub client_id: String,
    #[serde(default)]
    pub client_secret: String,
    #[serde(default)]
    pub auth_url: String,
    #[serde(default)]
    pub token_url: String,
    #[serde(default)]
    pub user_api_url: String,
    #[serde(default)]
    pub pkce: Option<bool>,
    #[serde(default)]
    pub extra: Map<String, Value>,
}

impl OAuth2ProviderConfig {
    pub fn habilitado(&self) -> bool {
        !self.client_id.trim().is_empty()
    }

    /// Nome de exibição amigável.
    pub fn nome_exibicao(&self) -> String {
        if !self.display_name.is_empty() {
            return self.display_name.clone();
        }
        match self.name.to_ascii_lowercase().as_str() {
            "google" => "Google".into(),
            "github" => "GitHub".into(),
            "gitlab" => "GitLab".into(),
            "discord" => "Discord".into(),
            "microsoft" => "Microsoft".into(),
            "spotify" => "Spotify".into(),
            "apple" => "Apple".into(),
            "oidc" | "oidc2" | "oidc3" => "OpenID Connect".into(),
            _ => self.name.clone(),
        }
    }

    /// URL de autorização padrão conforme o provedor.
    pub fn auth_url_efetiva(&self) -> String {
        if !self.auth_url.is_empty() {
            return self.auth_url.clone();
        }
        match self.name.to_ascii_lowercase().as_str() {
            "google" => "https://accounts.google.com/o/oauth2/v2/auth".into(),
            "github" => "https://github.com/login/oauth/authorize".into(),
            "gitlab" => "https://gitlab.com/oauth/authorize".into(),
            "discord" => "https://discord.com/api/oauth2/authorize".into(),
            "microsoft" => "https://login.microsoftonline.com/common/oauth2/v2.0/authorize".into(),
            "spotify" => "https://accounts.spotify.com/authorize".into(),
            _ => self.auth_url.clone(),
        }
    }

    /// URL de token padrão conforme o provedor.
    pub fn token_url_efetiva(&self) -> String {
        if !self.token_url.is_empty() {
            return self.token_url.clone();
        }
        match self.name.to_ascii_lowercase().as_str() {
            "google" => "https://oauth2.googleapis.com/token".into(),
            "github" => "https://github.com/login/oauth/access_token".into(),
            "gitlab" => "https://gitlab.com/oauth/token".into(),
            "discord" => "https://discord.com/api/oauth2/token".into(),
            "microsoft" => "https://login.microsoftonline.com/common/oauth2/v2.0/token".into(),
            "spotify" => "https://accounts.spotify.com/api/token".into(),
            _ => self.token_url.clone(),
        }
    }

    /// URL de consulta de perfil de usuário padrão conforme o provedor.
    pub fn user_api_url_efetiva(&self) -> String {
        if !self.user_api_url.is_empty() {
            return self.user_api_url.clone();
        }
        match self.name.to_ascii_lowercase().as_str() {
            "google" => "https://www.googleapis.com/oauth2/v2/userinfo".into(),
            "github" => "https://api.github.com/user".into(),
            "gitlab" => "https://gitlab.com/api/v4/user".into(),
            "discord" => "https://discord.com/api/users/@me".into(),
            "microsoft" => "https://graph.microsoft.com/v1.0/me".into(),
            "spotify" => "https://api.spotify.com/v1/me".into(),
            _ => self.user_api_url.clone(),
        }
    }

    /// Escopos padrão solicitados no início do fluxo.
    pub fn escopos_padrao(&self) -> &'static str {
        match self.name.to_ascii_lowercase().as_str() {
            "google" => "openid email profile",
            "github" => "read:user user:email",
            "gitlab" => "read_user openid profile email",
            "discord" => "identify email",
            "microsoft" => "openid profile email User.Read",
            "spotify" => "user-read-private user-read-email",
            _ => "openid email profile",
        }
    }
}

/// Mapeamento de campos retornados pelo provedor OAuth2 para campos da coleção.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OAuth2MappedFields {
    #[serde(default = "OAuth2MappedFields::id_padrao")]
    pub id: String,
    #[serde(default = "OAuth2MappedFields::name_padrao")]
    pub name: String,
    #[serde(default = "OAuth2MappedFields::avatar_padrao")]
    pub avatar: String,
}

impl Default for OAuth2MappedFields {
    fn default() -> Self {
        Self {
            id: Self::id_padrao(),
            name: Self::name_padrao(),
            avatar: Self::avatar_padrao(),
        }
    }
}

impl OAuth2MappedFields {
    fn id_padrao() -> String {
        "id".into()
    }
    fn name_padrao() -> String {
        "name".into()
    }
    fn avatar_padrao() -> String {
        "avatar".into()
    }
}

/// Configuração geral de OAuth2 em uma coleção.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct OAuth2Config {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub providers: Vec<OAuth2ProviderConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mapped_fields: Option<OAuth2MappedFields>,
}

/// Configuração de OTP (One-Time Password) em uma coleção.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OtpConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "OtpConfig::duracao_padrao")]
    pub duration: i64,
    #[serde(default = "OtpConfig::tamanho_padrao")]
    pub length: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub email_template: Option<crate::correio::EmailTemplate>,
}

impl Default for OtpConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            duration: Self::duracao_padrao(),
            length: Self::tamanho_padrao(),
            email_template: None,
        }
    }
}

impl OtpConfig {
    pub fn duracao_padrao() -> i64 {
        180 // 3 minutos
    }

    pub fn tamanho_padrao() -> usize {
        6 // 6 dígitos
    }
}

/// Configuração de MFA (Multi-Factor Authentication) em uma coleção.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MfaConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "MfaConfig::duracao_padrao")]
    pub duration: i64,
    #[serde(default)]
    pub rule: String,
}

impl Default for MfaConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            duration: Self::duracao_padrao(),
            rule: String::new(),
        }
    }
}

impl MfaConfig {
    pub fn duracao_padrao() -> i64 {
        1800 // 30 minutos
    }
}

/// Configuração de autenticação por senha / e-mail.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PasswordAuthConfig {
    #[serde(default = "PasswordAuthConfig::enabled_padrao")]
    pub enabled: bool,
    #[serde(default = "PasswordAuthConfig::identity_fields_padrao")]
    pub identity_fields: Vec<String>,
}

impl Default for PasswordAuthConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            identity_fields: Self::identity_fields_padrao(),
        }
    }
}

impl PasswordAuthConfig {
    pub fn enabled_padrao() -> bool {
        true
    }
    pub fn identity_fields_padrao() -> Vec<String> {
        vec!["email".into()]
    }
}

/// Configuração de duração de tokens.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TokenConfig {
    #[serde(default)]
    pub duration: i64,
}

/// Configuração de alertas de login.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct AuthAlertConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub email_template: Option<crate::correio::EmailTemplate>,
}

/// Informações do provedor OAuth2 retornadas no endpoint público `auth-methods`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthProviderInfo {
    pub name: String,
    pub display_name: String,
    pub state: String,
    pub auth_url: String,
    pub code_verifier: String,
    pub code_challenge: String,
    pub code_challenge_method: String,
}

/// Prepara as informações públicas de um provedor OAuth2 com PKCE e URL de autorização pronta.
pub fn preparar_provedor(
    p: &OAuth2ProviderConfig,
    redirect_uri: &str,
    state_fornecido: Option<&str>,
) -> AuthProviderInfo {
    let (code_verifier, code_challenge) = gerar_pkce();
    let state = state_fornecido
        .map(String::from)
        .unwrap_or_else(gerar_state);

    let auth_base = p.auth_url_efetiva();
    let mut parsed_url = url::Url::parse(&auth_base).unwrap_or_else(|_| {
        url::Url::parse("https://example.com/oauth2/auth").expect("url valida")
    });

    {
        let mut q = parsed_url.query_pairs_mut();
        q.append_pair("client_id", &p.client_id);
        q.append_pair("response_type", "code");
        if !redirect_uri.is_empty() {
            q.append_pair("redirect_uri", redirect_uri);
        }
        q.append_pair("scope", p.escopos_padrao());
        q.append_pair("state", &state);
        q.append_pair("code_challenge", &code_challenge);
        q.append_pair("code_challenge_method", "S256");
    }

    AuthProviderInfo {
        name: p.name.clone(),
        display_name: p.nome_exibicao(),
        state,
        auth_url: parsed_url.to_string(),
        code_verifier,
        code_challenge,
        code_challenge_method: "S256".into(),
    }
}

/// Resumo de MFA no retorno de `auth-methods`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MfaResumo {
    pub enabled: bool,
    pub duration: i64,
}

/// Resumo de OTP no retorno de `auth-methods`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OtpResumo {
    pub enabled: bool,
    pub duration: i64,
}

/// Resumo de PasswordAuth no retorno de `auth-methods`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PasswordResumo {
    pub enabled: bool,
    pub identity_fields: Vec<String>,
}

/// Resumo de OAuth2 no retorno de `auth-methods`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OAuth2Resumo {
    pub enabled: bool,
    pub providers: Vec<AuthProviderInfo>,
}

/// Resposta completa do endpoint `GET /api/collections/{collection}/auth-methods`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuthMethodsResponse {
    pub mfa: MfaResumo,
    pub otp: OtpResumo,
    pub password: PasswordResumo,
    pub oauth2: OAuth2Resumo,
}

/// Resposta do token exchange retornado pelo provedor OAuth2.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OAuth2TokenResponse {
    pub access_token: String,
    #[serde(default)]
    pub refresh_token: Option<String>,
    #[serde(default)]
    pub id_token: Option<String>,
    #[serde(default)]
    pub token_type: Option<String>,
    #[serde(default)]
    pub expires_in: Option<i64>,
}

/// Perfil de usuário normalizado a partir da resposta de API do provedor OAuth2.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OAuth2PerfilUsuario {
    pub id: String,
    pub email: String,
    pub name: String,
    pub username: String,
    pub avatar_url: String,
    pub raw_user: Value,
}

/// Efetua a troca do código de autorização pelo token de acesso.
pub async fn trocar_codigo(
    provedor: &OAuth2ProviderConfig,
    codigo: &str,
    code_verifier: Option<&str>,
    redirect_url: &str,
) -> Resultado<OAuth2TokenResponse> {
    let client = reqwest::Client::builder()
        .build()
        .map_err(|e| Erro::Interno(format!("Falha ao construir cliente HTTP: {e}")))?;

    let token_url = provedor.token_url_efetiva();
    let mut params = HashMap::new();
    params.insert("grant_type", "authorization_code");
    params.insert("client_id", &provedor.client_id);
    params.insert("client_secret", &provedor.client_secret);
    params.insert("code", codigo);
    params.insert("redirect_uri", redirect_url);
    if let Some(cv) = code_verifier {
        params.insert("code_verifier", cv);
    }

    let mut headers = HeaderMap::new();
    headers.insert(ACCEPT, HeaderValue::from_static("application/json"));
    headers.insert(
        USER_AGENT,
        HeaderValue::from_static("Forjabase-Server/0.0.3"),
    );

    let resp = client
        .post(&token_url)
        .headers(headers)
        .form(&params)
        .send()
        .await
        .map_err(|e| Erro::invalido(format!("Falha na requisição de token OAuth2: {e}")))?;

    if !resp.status().is_success() {
        let texto = resp.text().await.unwrap_or_default();
        return Err(Erro::invalido(format!(
            "Provedor OAuth2 recusou a troca de código: {texto}"
        )));
    }

    let texto = resp
        .text()
        .await
        .map_err(|e| Erro::invalido(format!("Falha ao ler corpo do token OAuth2: {e}")))?;

    let valor: Value = serde_json::from_str(&texto)
        .map_err(|e| Erro::invalido(format!("Resposta inválida do token OAuth2: {e}")))?;

    let access_token = valor
        .get("access_token")
        .and_then(Value::as_str)
        .ok_or_else(|| Erro::invalido("Provedor não retornou 'access_token'."))?
        .to_string();

    let refresh_token = valor
        .get("refresh_token")
        .and_then(Value::as_str)
        .map(String::from);
    let id_token = valor
        .get("id_token")
        .and_then(Value::as_str)
        .map(String::from);
    let token_type = valor
        .get("token_type")
        .and_then(Value::as_str)
        .map(String::from);
    let expires_in = valor.get("expires_in").and_then(Value::as_i64);

    Ok(OAuth2TokenResponse {
        access_token,
        refresh_token,
        id_token,
        token_type,
        expires_in,
    })
}

/// Consulta os dados do usuário no provedor usando o token de acesso obtido.
pub async fn buscar_perfil(
    provedor: &OAuth2ProviderConfig,
    access_token: &str,
) -> Resultado<OAuth2PerfilUsuario> {
    let client = reqwest::Client::builder()
        .build()
        .map_err(|e| Erro::Interno(format!("Falha ao construir cliente HTTP: {e}")))?;

    let api_url = provedor.user_api_url_efetiva();
    let auth_header = format!("Bearer {access_token}");

    let mut headers = HeaderMap::new();
    headers.insert(ACCEPT, HeaderValue::from_static("application/json"));
    headers.insert(
        USER_AGENT,
        HeaderValue::from_static("Forjabase-Server/0.0.3"),
    );
    if let Ok(hv) = HeaderValue::from_str(&auth_header) {
        headers.insert(AUTHORIZATION, hv);
    }

    let resp = client
        .get(&api_url)
        .headers(headers)
        .send()
        .await
        .map_err(|e| Erro::invalido(format!("Falha na consulta de perfil OAuth2: {e}")))?;

    if !resp.status().is_success() {
        let texto = resp.text().await.unwrap_or_default();
        return Err(Erro::invalido(format!(
            "Provedor OAuth2 recusou consulta de perfil: {texto}"
        )));
    }

    let texto = resp
        .text()
        .await
        .map_err(|e| Erro::invalido(format!("Falha ao ler corpo do perfil OAuth2: {e}")))?;

    let raw: Value = serde_json::from_str(&texto)
        .map_err(|e| Erro::invalido(format!("Resposta JSON de perfil inválida: {e}")))?;

    Ok(normalizar_perfil(&provedor.name, raw))
}

/// Normaliza os campos do perfil de acordo com as peculiaridades de cada provedor.
pub fn normalizar_perfil(provedor_nome: &str, raw: Value) -> OAuth2PerfilUsuario {
    let str_val = |chaves: &[&str]| -> String {
        for &k in chaves {
            if let Some(v) = raw.get(k) {
                if let Some(s) = v.as_str() {
                    if !s.is_empty() {
                        return s.to_string();
                    }
                } else if let Some(n) = v.as_i64() {
                    return n.to_string();
                }
            }
        }
        String::new()
    };

    match provedor_nome.to_ascii_lowercase().as_str() {
        "google" => {
            let id = str_val(&["sub", "id"]);
            let email = str_val(&["email"]);
            let name = str_val(&["name"]);
            let username = if !email.is_empty() {
                email.split('@').next().unwrap_or("").to_string()
            } else {
                id.clone()
            };
            let avatar_url = str_val(&["picture"]);
            OAuth2PerfilUsuario {
                id,
                email,
                name,
                username,
                avatar_url,
                raw_user: raw,
            }
        }
        "github" => {
            let id = str_val(&["id"]);
            let login = str_val(&["login"]);
            let name = str_val(&["name"]);
            let email = str_val(&["email"]);
            let avatar_url = str_val(&["avatar_url"]);
            let username = if !login.is_empty() {
                login
            } else {
                name.clone()
            };
            OAuth2PerfilUsuario {
                id,
                email,
                name: if name.is_empty() {
                    username.clone()
                } else {
                    name
                },
                username,
                avatar_url,
                raw_user: raw,
            }
        }
        "discord" => {
            let id = str_val(&["id"]);
            let username = str_val(&["username"]);
            let global_name = str_val(&["global_name"]);
            let email = str_val(&["email"]);
            let avatar = str_val(&["avatar"]);
            let avatar_url = if !avatar.is_empty() && !id.is_empty() {
                format!("https://cdn.discordapp.com/avatars/{id}/{avatar}.png")
            } else {
                String::new()
            };
            let name = if !global_name.is_empty() {
                global_name
            } else {
                username.clone()
            };
            OAuth2PerfilUsuario {
                id,
                email,
                name,
                username,
                avatar_url,
                raw_user: raw,
            }
        }
        "gitlab" => {
            let id = str_val(&["id"]);
            let username = str_val(&["username"]);
            let name = str_val(&["name"]);
            let email = str_val(&["email"]);
            let avatar_url = str_val(&["avatar_url"]);
            OAuth2PerfilUsuario {
                id,
                email,
                name,
                username,
                avatar_url,
                raw_user: raw,
            }
        }
        "microsoft" => {
            let id = str_val(&["id"]);
            let display_name = str_val(&["displayName"]);
            let mail = str_val(&["mail", "userPrincipalName"]);
            let username = if !mail.is_empty() {
                mail.split('@').next().unwrap_or("").to_string()
            } else {
                id.clone()
            };
            OAuth2PerfilUsuario {
                id,
                email: mail,
                name: display_name,
                username,
                avatar_url: String::new(),
                raw_user: raw,
            }
        }
        _ => {
            // Padrão genérico OIDC
            let id = str_val(&["sub", "id"]);
            let email = str_val(&["email"]);
            let name = str_val(&["name", "displayName"]);
            let username = str_val(&["preferred_username", "username", "login"]);
            let username = if !username.is_empty() {
                username
            } else if !email.is_empty() {
                email.split('@').next().unwrap_or("").to_string()
            } else {
                id.clone()
            };
            let avatar_url = str_val(&["picture", "avatar_url", "avatar"]);
            OAuth2PerfilUsuario {
                id,
                email,
                name,
                username,
                avatar_url,
                raw_user: raw,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn pkce_geracao_e_desafio() {
        let (verifier, challenge) = gerar_pkce();
        assert_eq!(verifier.len(), 64);
        assert!(!challenge.is_empty());
        assert!(!challenge.contains('+'));
        assert!(!challenge.contains('/'));
        assert!(!challenge.contains('='));

        // Valida que o desafio corresponde exatamente ao SHA256 do verificador
        let mut hasher = Sha256::new();
        hasher.update(verifier.as_bytes());
        let esperado = URL_SAFE_NO_PAD.encode(hasher.finalize());
        assert_eq!(challenge, esperado);
    }

    #[test]
    fn codigo_otp_comprimento() {
        let codigo = gerar_codigo_otp(6);
        assert_eq!(codigo.len(), 6);
        assert!(codigo.chars().all(|c| c.is_ascii_digit()));

        let codigo8 = gerar_codigo_otp(8);
        assert_eq!(codigo8.len(), 8);
        assert!(codigo8.chars().all(|c| c.is_ascii_digit()));
    }

    #[test]
    fn preparacao_provedor_gera_url_e_pkce() {
        let p = OAuth2ProviderConfig {
            name: "google".into(),
            display_name: "Google Login".into(),
            client_id: "google-client-123".into(),
            client_secret: "secret".into(),
            auth_url: String::new(),
            token_url: String::new(),
            user_api_url: String::new(),
            pkce: Some(true),
            extra: Map::new(),
        };

        let info = preparar_provedor(&p, "http://127.0.0.1:8090/api/oauth2-redirect", None);
        assert_eq!(info.name, "google");
        assert_eq!(info.display_name, "Google Login");
        assert!(!info.state.is_empty());
        assert!(!info.code_verifier.is_empty());
        assert_eq!(info.code_challenge_method, "S256");
        assert!(
            info.auth_url
                .starts_with("https://accounts.google.com/o/oauth2/v2/auth?")
        );
        assert!(info.auth_url.contains("client_id=google-client-123"));
        assert!(info.auth_url.contains("code_challenge="));
        assert!(info.auth_url.contains("code_challenge_method=S256"));
    }

    #[test]
    fn normalizacao_perfil_google() {
        let raw = json!({
            "sub": "g-12345",
            "email": "ana@gmail.com",
            "name": "Ana Silva",
            "picture": "https://lh3.googleusercontent.com/avatar.jpg"
        });
        let perfil = normalizar_perfil("google", raw);
        assert_eq!(perfil.id, "g-12345");
        assert_eq!(perfil.email, "ana@gmail.com");
        assert_eq!(perfil.name, "Ana Silva");
        assert_eq!(perfil.username, "ana");
        assert_eq!(
            perfil.avatar_url,
            "https://lh3.googleusercontent.com/avatar.jpg"
        );
    }

    #[test]
    fn normalizacao_perfil_github() {
        let raw = json!({
            "id": 987654,
            "login": "octocat",
            "name": "Monalisa Octocat",
            "email": "octocat@github.com",
            "avatar_url": "https://github.com/images/error/octocat_happy.gif"
        });
        let perfil = normalizar_perfil("github", raw);
        assert_eq!(perfil.id, "987654");
        assert_eq!(perfil.email, "octocat@github.com");
        assert_eq!(perfil.name, "Monalisa Octocat");
        assert_eq!(perfil.username, "octocat");
        assert_eq!(
            perfil.avatar_url,
            "https://github.com/images/error/octocat_happy.gif"
        );
    }

    #[test]
    fn gerador_apple_client_secret_valida_campos_obrigatorios() {
        assert!(gerar_apple_client_secret("", "team", "key", "pk", 100).is_err());
        assert!(gerar_apple_client_secret("client", "", "key", "pk", 100).is_err());
        assert!(gerar_apple_client_secret("client", "team", "", "pk", 100).is_err());
        assert!(gerar_apple_client_secret("client", "team", "key", "", 100).is_err());
    }
}
