//! Erros do núcleo. O servidor os traduz para o formato de erro do PocketBase:
//! `{"code": 400, "message": "...", "data": {"campo": {"code": "...", "message": "..."}}}`.

use serde_json::{Map, Value, json};
use std::fmt;

#[derive(Debug, Clone, PartialEq)]
pub enum Erro {
    /// 400, com os erros por campo em `campos`.
    Invalido {
        mensagem: String,
        campos: Map<String, Value>,
    },
    /// 401
    NaoAutorizado(String),
    /// 403
    Proibido(String),
    /// 404
    NaoEncontrado(String),
    /// Violação de unicidade no banco (id ou nome repetido). Quem chama
    /// transforma em erro de validação do campo certo.
    Duplicado(String),
    /// 500 (a mensagem vai para o log, não para o cliente).
    Interno(String),
}

impl Erro {
    pub fn invalido(mensagem: impl Into<String>) -> Erro {
        Erro::Invalido {
            mensagem: mensagem.into(),
            campos: Map::new(),
        }
    }

    /// Erro de validação de um campo, com código e mensagem como no PocketBase.
    pub fn campo(nome: &str, codigo: &str, mensagem: &str) -> Erro {
        let mut campos = Map::new();
        campos.insert(nome.into(), json!({ "code": codigo, "message": mensagem }));
        Erro::Invalido {
            mensagem: "Failed to create record.".into(),
            campos,
        }
    }

    pub fn codigo_http(&self) -> u16 {
        match self {
            Erro::Invalido { .. } => 400,
            Erro::NaoAutorizado(_) => 401,
            Erro::Proibido(_) => 403,
            Erro::NaoEncontrado(_) => 404,
            Erro::Duplicado(_) => 400,
            Erro::Interno(_) => 500,
        }
    }
}

impl fmt::Display for Erro {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Erro::Invalido { mensagem, .. } => write!(f, "{mensagem}"),
            Erro::NaoAutorizado(m)
            | Erro::Proibido(m)
            | Erro::NaoEncontrado(m)
            | Erro::Duplicado(m)
            | Erro::Interno(m) => {
                write!(f, "{m}")
            }
        }
    }
}

impl std::error::Error for Erro {}

pub type Resultado<T> = Result<T, Erro>;
