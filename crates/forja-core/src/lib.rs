//! Núcleo do Forja: o modelo de dados, os filtros e o armazenamento, sem HTTP.
//!
//! Os nomes seguem o PocketBase (coleções, campos, regras) porque a meta é a
//! API do Forja ser compatível com a dele. Ver `docs/FUNCIONALIDADES.md`.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

pub mod auth;
pub mod barramento;
pub mod correio;
pub mod erro;
pub mod filtro;
pub mod id;
pub mod motor;
mod relacao;
pub mod store;
pub mod valor;

pub use erro::Erro;

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
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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
    #[serde(default)]
    pub created: String,
    #[serde(default)]
    pub updated: String,
}

impl Collection {
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
        matches!(nome, "id" | "created" | "updated") || self.campo(nome).is_some()
    }
}

/// Agora em UTC, no formato do PocketBase (`2026-10-21 14:05:09.123Z`).
pub fn agora() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let d = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    let (s, ms) = (d.as_secs() as i64, d.subsec_millis());
    let (dias, resto) = (s.div_euclid(86_400), s.rem_euclid(86_400));
    let (h, m, seg) = (resto / 3600, resto % 3600 / 60, resto % 60);
    // Data civil a partir de dias desde 1970 (algoritmo de Howard Hinnant).
    let z = dias + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let dia = doy - (153 * mp + 2) / 5 + 1;
    let mes = if mp < 10 { mp + 3 } else { mp - 9 };
    let ano = yoe + era * 400 + if mes <= 2 { 1 } else { 0 };
    format!("{ano:04}-{mes:02}-{dia:02} {h:02}:{m:02}:{seg:02}.{ms:03}Z")
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
    }

    #[test]
    fn versao() {
        assert_eq!(VERSION, "0.0.3");
    }
}
