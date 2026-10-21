//! Núcleo do Forja: o modelo de dados, sem HTTP nem banco.
//!
//! Os nomes seguem o PocketBase (coleções, campos, regras) porque a meta é a
//! API do Forja ser compatível com a dele. Ver `docs/FUNCIONALIDADES.md`.

use serde::{Deserialize, Serialize};

/// Versão do Forja (a mesma do workspace).
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Tipo de coleção.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CollectionType {
    /// Registros comuns.
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
    fn versao_inicial() {
        assert_eq!(VERSION, "0.0.1");
    }
}
