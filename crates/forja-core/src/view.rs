//! Coleções `view`: uma consulta `SELECT` vira uma view do SQLite.
//!
//! O superusuário escreve o SQL. Ele tem de ser um único `SELECT` (ou `WITH`),
//! com uma coluna `id`. Os outros nomes viram campos. Create, update e delete
//! de registro não existem numa view.

use crate::FieldType;
use crate::erro::{Erro, Resultado};

/// Normaliza a consulta de uma view. Recusa vazio, mais de um comando e
/// qualquer coisa que não comece com `SELECT` ou `WITH`.
pub fn normalizar_consulta(q: &str) -> Resultado<String> {
    let q = q.trim();
    if q.is_empty() {
        return Err(erro_consulta(
            "validation_required",
            "Cannot be blank.".into(),
        ));
    }
    let q = q.strip_suffix(';').unwrap_or(q).trim();
    if q.is_empty() || ponto_e_virgula_fora_de_texto(q) {
        return Err(erro_consulta(
            "validation_invalid_view_query",
            "View query must be a single SELECT.".into(),
        ));
    }
    let primeira = q.split_whitespace().next().unwrap_or("");
    if !primeira.eq_ignore_ascii_case("select") && !primeira.eq_ignore_ascii_case("with") {
        return Err(erro_consulta(
            "validation_invalid_view_query",
            "View query must be a SELECT.".into(),
        ));
    }
    Ok(q.to_string())
}

/// A consulta cita `nome` como identificador (não como pedaço de outro nome).
pub fn cita(consulta: &str, nome: &str) -> bool {
    let consulta = consulta.to_ascii_lowercase();
    let nome = nome.to_ascii_lowercase();
    let c = consulta.as_bytes();
    let n = nome.as_bytes();
    if n.is_empty() || n.len() > c.len() {
        return false;
    }
    let mut i = 0;
    while i + n.len() <= c.len() {
        if &c[i..i + n.len()] == n {
            let antes = i == 0 || !ident_byte(c[i - 1]);
            let depois = i + n.len() == c.len() || !ident_byte(c[i + n.len()]);
            if antes && depois {
                return true;
            }
        }
        i += 1;
    }
    false
}

/// Coluna que pode sair numa view: `id`, ou um nome de campo.
/// `created` e `updated` são campos comuns quando a consulta os projeta.
pub fn coluna_aceita(nome: &str) -> bool {
    if nome == "id" {
        return true;
    }
    ident(nome) && !matches!(nome, "collectionId" | "collectionName" | "expand")
}

/// Tipo de campo a partir do tipo declarado pelo motor.
pub fn tipo_inferido(tipo_sql: &str) -> FieldType {
    let t = tipo_sql
        .split('(')
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    match t.as_str() {
        "int" | "integer" | "tinyint" | "smallint" | "mediumint" | "bigint" | "int2" | "int8"
        | "numeric" | "real" | "double" | "float" | "decimal" | "double precision" => {
            FieldType::Number
        }
        "boolean" | "bool" => FieldType::Bool,
        "json" | "jsonb" => FieldType::Json,
        _ => FieldType::Text,
    }
}

pub fn erro_consulta(codigo: &str, mensagem: String) -> Erro {
    use serde_json::json;
    let mut campos = serde_json::Map::new();
    campos.insert(
        "viewQuery".into(),
        json!({ "code": codigo, "message": mensagem }),
    );
    Erro::Invalido {
        mensagem: "Failed to create collection.".into(),
        campos,
    }
}

fn ponto_e_virgula_fora_de_texto(q: &str) -> bool {
    let mut chars = q.chars().peekable();
    let mut aspa: Option<char> = None;
    while let Some(c) = chars.next() {
        if let Some(abre) = aspa {
            if c == abre {
                // `''` e `""` são aspa escapada no SQL.
                if chars.peek() == Some(&abre) {
                    chars.next();
                } else {
                    aspa = None;
                }
            }
            continue;
        }
        match c {
            '\'' | '"' => aspa = Some(c),
            ';' => return true,
            _ => {}
        }
    }
    false
}

fn ident_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

fn ident(n: &str) -> bool {
    let mut cs = n.chars();
    matches!(cs.next(), Some(c) if c.is_ascii_alphabetic() || c == '_')
        && cs.all(|c| c.is_ascii_alphanumeric() || c == '_')
        && !n.is_empty()
        && n.len() <= 100
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aceita_select_e_recusa_o_resto() {
        assert_eq!(
            normalizar_consulta("  SELECT id FROM t ; ").unwrap(),
            "SELECT id FROM t"
        );
        assert!(normalizar_consulta("").is_err());
        assert!(normalizar_consulta("DELETE FROM t").is_err());
        assert!(normalizar_consulta("SELECT id FROM t; DROP TABLE t").is_err());
        assert!(normalizar_consulta("SELECT ';' AS id").is_ok());
    }

    #[test]
    fn cita_o_nome_inteiro() {
        assert!(cita("SELECT id FROM produtos", "produtos"));
        assert!(cita("SELECT id FROM \"Produtos\"", "produtos"));
        assert!(!cita("SELECT id FROM produtos_arquivo", "produtos"));
    }

    #[test]
    fn tipos() {
        assert_eq!(tipo_inferido("NUMERIC"), FieldType::Number);
        assert_eq!(tipo_inferido("DOUBLE PRECISION"), FieldType::Number);
        assert_eq!(tipo_inferido("BOOLEAN"), FieldType::Bool);
        assert_eq!(tipo_inferido("JSON"), FieldType::Json);
        assert_eq!(tipo_inferido(""), FieldType::Text);
        assert_eq!(tipo_inferido("VARCHAR(20)"), FieldType::Text);
    }
}
