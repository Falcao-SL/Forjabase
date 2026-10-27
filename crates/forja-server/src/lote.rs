//! `POST /api/batch`: várias operações de registro no mesmo commit.
//!
//! Se uma falha, o SQLite desfaz as outras e a resposta é o erro dessa
//! operação. O corpo é JSON; arquivo entra pelo multipart do registro, não
//! pelo lote.

use super::{Chamador, ErroApi, Estado, identificar, token_do_header};
use axum::Json;
use axum::http::StatusCode;
use forja_core::auth::TIPO_AUTH;
use forja_core::erro::Erro;
use serde::Deserialize;
use serde_json::{Map, Value, json};

const MAXIMO: usize = 50;

#[derive(Deserialize)]
pub struct Pedido {
    requests: Vec<Item>,
}

#[derive(Deserialize)]
struct Item {
    method: String,
    url: String,
    #[serde(default)]
    headers: Map<String, Value>,
    #[serde(default)]
    body: Value,
}

pub async fn http(
    axum::extract::State(e): axum::extract::State<Estado>,
    quem: Chamador,
    Json(pedido): Json<Pedido>,
) -> Result<Json<Value>, ErroApi> {
    if pedido.requests.is_empty() {
        return Err(ErroApi(Erro::invalido("Requests list can't be empty.")));
    }
    if pedido.requests.len() > MAXIMO {
        return Err(ErroApi(Erro::invalido("Batch requests limit exceeded.")));
    }
    let estado = e.clone();
    let respostas = e
        .banco
        .em_transacao(async move {
            let mut out = Vec::with_capacity(pedido.requests.len());
            for item in pedido.requests {
                let (status, body) = despachar(&estado, &quem, &item).await?;
                out.push(json!({ "status": status, "body": body }));
            }
            Ok(out)
        })
        .await?;
    Ok(Json(Value::Array(respostas)))
}

async fn despachar(e: &Estado, externo: &Chamador, item: &Item) -> Result<(u16, Value), Erro> {
    let quem_local;
    let quem = if let Some(bruto) = header_auth(&item.headers) {
        let token = token_do_header(bruto);
        quem_local = if token.is_empty() {
            Chamador(None)
        } else {
            identificar(e, &token, TIPO_AUTH)
                .await
                .map_err(|err| err.0)?
        };
        &quem_local
    } else {
        externo
    };
    let (partes, query) = caminho(&item.url)?;
    let method = item.method.to_ascii_uppercase();
    let expand = query_get(&query, "expand").map(str::to_string);
    let partes: Vec<&str> = partes.iter().map(String::as_str).collect();
    match (method.as_str(), partes.as_slice()) {
        ("POST", ["api", "collections", nome, "records"]) => {
            let c = e.banco.colecao(nome).await?;
            let dados = objeto(&item.body)?;
            let v = super::criar_registro(e, quem, &c, dados, Vec::new(), expand).await?;
            Ok((StatusCode::OK.as_u16(), v))
        }
        ("PATCH", ["api", "collections", nome, "records", id]) => {
            let c = e.banco.colecao(nome).await?;
            let dados = objeto(&item.body)?;
            let v = super::atualizar_registro(e, quem, &c, id, dados, Vec::new(), expand).await?;
            Ok((StatusCode::OK.as_u16(), v))
        }
        ("DELETE", ["api", "collections", nome, "records", id]) => {
            let c = e.banco.colecao(nome).await?;
            super::apagar_registro(e, quem, &c, id).await?;
            Ok((StatusCode::NO_CONTENT.as_u16(), Value::Null))
        }
        ("GET", ["api", "collections", nome, "records"]) => {
            let c = e.banco.colecao(nome).await?;
            let v = super::listar_registro(e, quem, &c, &query).await?;
            Ok((StatusCode::OK.as_u16(), v))
        }
        ("GET", ["api", "collections", nome, "records", id]) => {
            let c = e.banco.colecao(nome).await?;
            let v = super::ver_registro(e, quem, &c, id, expand).await?;
            Ok((StatusCode::OK.as_u16(), v))
        }
        _ => Err(Erro::invalido("Unsupported batch request.")),
    }
}

fn objeto(v: &Value) -> Result<Map<String, Value>, Erro> {
    match v {
        Value::Null => Ok(Map::new()),
        Value::Object(m) => Ok(m.clone()),
        _ => Err(Erro::invalido("Expected a JSON object.")),
    }
}

fn header_auth(headers: &Map<String, Value>) -> Option<&str> {
    headers.iter().find_map(|(k, v)| {
        k.eq_ignore_ascii_case("authorization")
            .then(|| v.as_str())
            .flatten()
    })
}

type Caminho = (Vec<String>, Vec<(String, String)>);

fn caminho(url: &str) -> Result<Caminho, Erro> {
    if !url.starts_with("/api/") || url.contains("://") || url.starts_with("//") {
        return Err(Erro::invalido("Invalid batch request URL."));
    }
    let (path, q) = url.split_once('?').unwrap_or((url, ""));
    if path.contains("/batch") || path.contains("/realtime") {
        return Err(Erro::invalido("Unsupported batch request."));
    }
    let partes = path
        .trim_matches('/')
        .split('/')
        .map(str::to_string)
        .collect();
    let pares = form_urlencoded::parse(q.as_bytes()).into_owned().collect();
    Ok((partes, pares))
}

fn query_get<'a>(pares: &'a [(String, String)], nome: &str) -> Option<&'a str> {
    pares
        .iter()
        .find(|(k, _)| k == nome)
        .map(|(_, v)| v.as_str())
}
