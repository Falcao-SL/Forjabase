//! Núcleo do Forja: o modelo de dados, os filtros e o armazenamento, sem HTTP.
//!
//! Os nomes seguem o PocketBase (coleções, campos, regras) porque a meta é a
//! API do Forja ser compatível com a dele. Ver `docs/FUNCIONALIDADES.md`.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

pub mod arquivos;
pub mod auth;
pub mod barramento;
pub mod correio;
pub mod erro;
pub mod filtro;
pub mod id;
pub mod motor;
pub mod oauth2;
mod relacao;
pub mod store;
pub mod valor;
mod view;

pub use erro::Erro;
pub use oauth2::{
    AuthAlertConfig, AuthMethodsResponse, AuthProviderInfo, MfaConfig, OAuth2Config,
    OAuth2MappedFields, OAuth2PerfilUsuario, OAuth2ProviderConfig, OAuth2TokenResponse, OtpConfig,
    PasswordAuthConfig, TokenConfig,
};
pub use store::{AuthOriginRegistro, ExternalAuthRegistro, MfaRegistro, OtpRegistro};

/// Versão do Forja (a mesma do workspace).
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Tipo de coleção.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum CollectionType {
    /// Registros comuns.
    #[default]
    Base,
    /// Registros que fazem login (e-mail e senha, OTP, OAuth2, MFA).
    Auth,
    /// Somente leitura, definida por uma consulta SQL.
    View,
}

/// Tipo de campo de uma coleção.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FieldType {
    Text,
    Editor,
    Number,
    Bool,
    Email,
    Url,
    Date,
    Autodate,
    Select,
    File,
    Relation,
    Json,
    Password,
    #[serde(rename = "geoPoint")]
    GeoPoint,
}

impl FieldType {
    /// Todos os tipos, na ordem do painel.
    pub const ALL: [FieldType; 14] = [
        FieldType::Text,
        FieldType::Editor,
        FieldType::Number,
        FieldType::Bool,
        FieldType::Email,
        FieldType::Url,
        FieldType::Date,
        FieldType::Autodate,
        FieldType::Select,
        FieldType::File,
        FieldType::Relation,
        FieldType::Json,
        FieldType::Password,
        FieldType::GeoPoint,
    ];

    /// Tipos que já guardam e validam valores nesta versão.
    pub fn suportado(self) -> bool {
        matches!(
            self,
            FieldType::Text
                | FieldType::Editor
                | FieldType::Number
                | FieldType::Bool
                | FieldType::Email
                | FieldType::Url
                | FieldType::Date
                | FieldType::Select
                | FieldType::Relation
                | FieldType::Json
                | FieldType::File
                | FieldType::Password
        )
    }

    /// Campos de sistema de uma coleção `auth`, na ordem em que aparecem.
    /// O PocketBase cria os mesmos cinco em toda coleção de autenticação.
    pub fn campos_auth() -> Vec<Field> {
        let mut email = Field::sistema("email", FieldType::Email);
        email.required = true;
        let email_visibility = Field::sistema("emailVisibility", FieldType::Bool);
        let verified = Field::sistema("verified", FieldType::Bool);
        let mut senha = Field::sistema("password", FieldType::Password);
        senha.required = true;
        senha.hidden = true;
        let mut token_key = Field::sistema("tokenKey", FieldType::Text);
        token_key.hidden = true;
        vec![email, email_visibility, verified, senha, token_key]
    }
}

/// Um campo de coleção. As opções próprias de cada tipo (`min`, `max`,
/// `values`, `maxSelect`…) ficam no mesmo nível, como no PocketBase.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Field {
    #[serde(default)]
    pub id: String,
    pub name: String,
    #[serde(rename = "type")]
    pub kind: FieldType,
    #[serde(default)]
    pub required: bool,
    #[serde(default)]
    pub hidden: bool,
    /// Campo gerenciado pelo sistema (só coleções `auth` têm): o usuário não
    /// escolhe o nome nem o tipo.
    #[serde(default)]
    pub system: bool,
    #[serde(flatten)]
    pub options: Map<String, Value>,
}

impl Field {
    /// Campo de sistema: nome, tipo e opções fixos, `system: true`.
    pub fn sistema(name: &str, kind: FieldType) -> Field {
        Field {
            id: String::new(),
            name: name.into(),
            kind,
            required: false,
            hidden: false,
            system: true,
            options: Map::new(),
        }
    }

    pub fn opcao_f64(&self, nome: &str) -> Option<f64> {
        self.options.get(nome).and_then(Value::as_f64)
    }

    pub fn opcao_bool(&self, nome: &str) -> bool {
        self.options
            .get(nome)
            .and_then(Value::as_bool)
            .unwrap_or(false)
    }

    /// `select`, `relation` e `file` com `maxSelect` maior que 1 guardam uma lista.
    /// Ausente ou até 1 é um valor só (string), como no PocketBase.
    pub fn multiplo(&self) -> bool {
        matches!(
            self.kind,
            FieldType::Select | FieldType::Relation | FieldType::File
        ) && self.opcao_f64("maxSelect").unwrap_or(1.0) > 1.0
    }
}

/// Uma coleção. `None` numa regra = só superusuário; `Some("")` = público.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Collection {
    #[serde(default)]
    pub id: String,
    pub name: String,
    #[serde(rename = "type", default)]
    pub kind: CollectionType,
    #[serde(default)]
    pub system: bool,
    #[serde(default)]
    pub fields: Vec<Field>,
    #[serde(default)]
    pub list_rule: Option<String>,
    #[serde(default)]
    pub view_rule: Option<String>,
    #[serde(default)]
    pub create_rule: Option<String>,
    #[serde(default)]
    pub update_rule: Option<String>,
    #[serde(default)]
    pub delete_rule: Option<String>,
    #[serde(default)]
    pub indexes: Vec<String>,
    /// Consulta `SELECT` de uma coleção `view`. Ausente nos outros tipos.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub view_query: Option<String>,
    #[serde(default)]
    pub created: String,
    #[serde(default)]
    pub updated: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auth_rule: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub manage_rule: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auth_alert: Option<AuthAlertConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub oauth2: Option<OAuth2Config>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub password_auth: Option<PasswordAuthConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mfa: Option<MfaConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub otp: Option<OtpConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auth_token: Option<TokenConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub password_reset_token: Option<TokenConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub email_change_token: Option<TokenConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verification_token: Option<TokenConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file_token: Option<TokenConfig>,
}

impl Collection {
    pub fn oauth2_config(&self) -> OAuth2Config {
        self.oauth2.clone().unwrap_or_default()
    }

    pub fn otp_config(&self) -> OtpConfig {
        self.otp.clone().unwrap_or_default()
    }

    pub fn mfa_config(&self) -> MfaConfig {
        self.mfa.clone().unwrap_or_default()
    }

    pub fn password_auth_config(&self) -> PasswordAuthConfig {
        self.password_auth.clone().unwrap_or_default()
    }

    pub fn auth_alert_config(&self) -> AuthAlertConfig {
        self.auth_alert.clone().unwrap_or_default()
    }

    pub fn campo(&self, nome: &str) -> Option<&Field> {
        self.fields.iter().find(|f| f.name == nome)
    }

    /// Nomes que um filtro ou uma ordenação podem usar. Senha e `tokenKey`
    /// ficam de fora: não são consultáveis nem ordenáveis.
    pub fn coluna_valida(&self, nome: &str) -> bool {
        if self
            .campo(nome)
            .is_some_and(|f| f.kind == FieldType::Password || (f.system && f.name == "tokenKey"))
        {
            return false;
        }
        if nome == "id" || self.campo(nome).is_some() {
            return true;
        }
        // `created` e `updated` são colunas fixas de base e auth. Numa view
        // só existem se a consulta as projetar (aí entram em `fields`).
        self.kind != CollectionType::View && matches!(nome, "created" | "updated")
    }
}

/// Componentes de tempo UTC: (ano, mes, dia, hora, minuto, segundo, milissegundo, weekday)
pub fn tempo_utc() -> (i64, u32, u32, u32, u32, u32, u32, u32) {
    use std::time::{SystemTime, UNIX_EPOCH};
    let d = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    let (s, ms) = (d.as_secs() as i64, d.subsec_millis());
    let (dias, resto) = (s.div_euclid(86_400), s.rem_euclid(86_400));
    let (h, m, seg) = (resto / 3600, resto % 3600 / 60, resto % 60);
    let weekday = ((dias + 4).rem_euclid(7)) as u32;
    let z = dias + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let dia = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let mes = (if mp < 10 { mp + 3 } else { mp - 9 }) as u32;
    let ano = yoe + era * 400 + if mes <= 2 { 1 } else { 0 };
    (ano, mes, dia, h as u32, m as u32, seg as u32, ms, weekday)
}

/// Agora em UTC, no formato do PocketBase (`2026-10-21 14:05:09.123Z`).
pub fn agora() -> String {
    let (ano, mes, dia, h, m, seg, ms, _) = tempo_utc();
    format!("{ano:04}-{mes:02}-{dia:02} {h:02}:{m:02}:{seg:02}.{ms:03}Z")
}

/// Converte segundos UNIX em string UTC no formato PocketBase.
pub fn data_utc_de_epoch_s(s: i64) -> String {
    let (dias, resto) = (s.div_euclid(86_400), s.rem_euclid(86_400));
    let (h, m, seg) = (resto / 3600, resto % 3600 / 60, resto % 60);
    let z = dias + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let dia = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let mes = (if mp < 10 { mp + 3 } else { mp - 9 }) as u32;
    let ano = yoe + era * 400 + if mes <= 2 { 1 } else { 0 };
    format!("{ano:04}-{mes:02}-{dia:02} {h:02}:{m:02}:{seg:02}.000Z")
}

/// Converte uma data UTC no formato PocketBase (`2026-10-21 14:05:09.123Z`) de volta em segundos UNIX.
pub fn epoch_s_de_data_utc(s: &str) -> Option<i64> {
    if s.len() < 19 {
        return None;
    }
    let ano: i64 = s[0..4].parse().ok()?;
    let mes: u32 = s[5..7].parse().ok()?;
    let dia: u32 = s[8..10].parse().ok()?;
    let h: i64 = s[11..13].parse().ok()?;
    let m: i64 = s[14..16].parse().ok()?;
    let seg: i64 = s[17..19].parse().ok()?;

    let y = if mes <= 2 { ano - 1 } else { ano };
    let era = y.div_euclid(400);
    let yoe = y.rem_euclid(400);
    let m_adj = if mes <= 2 { mes + 9 } else { mes - 3 };
    let doy = (153 * m_adj + 2) / 5 + dia - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + (doy as i64);
    let dias = era * 146_097 + doe - 719_468;

    Some(dias * 86_400 + h * 3600 + m * 60 + seg)
}

/// Retorna os segundos atuais desde a época UNIX.
pub fn agora_epoch_s() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nomes_iguais_aos_do_pocketbase() {
        assert_eq!(
            serde_json::to_string(&CollectionType::Auth).unwrap(),
            "\"auth\""
        );
        assert_eq!(
            serde_json::to_string(&FieldType::GeoPoint).unwrap(),
            "\"geoPoint\""
        );
        assert_eq!(
            serde_json::to_string(&FieldType::Autodate).unwrap(),
            "\"autodate\""
        );
        let t: FieldType = serde_json::from_str("\"relation\"").unwrap();
        assert_eq!(t, FieldType::Relation);
    }

    #[test]
    fn campo_com_opcoes_no_mesmo_nivel() {
        let f: Field = serde_json::from_str(
            r#"{"name":"tags","type":"select","values":["a","b"],"maxSelect":2}"#,
        )
        .unwrap();
        assert!(f.multiplo());
        let v = serde_json::to_value(&f).unwrap();
        assert_eq!(v["values"], serde_json::json!(["a", "b"]));
    }

    #[test]
    fn data_no_formato_do_pocketbase() {
        let a = agora();
        assert_eq!(a.len(), 24, "{a}");
        assert!(
            a.ends_with('Z') && &a[4..5] == "-" && &a[10..11] == " ",
            "{a}"
        );

        let agora_s = agora_epoch_s();
        let formatted = data_utc_de_epoch_s(agora_s);
        let parsed = epoch_s_de_data_utc(&formatted).expect("parsed date");
        assert_eq!(agora_s, parsed);
    }

    #[test]
    fn versao() {
        assert_eq!(VERSION, "0.0.3");
    }
}
