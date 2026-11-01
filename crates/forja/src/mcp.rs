//! Servidor MCP (Model Context Protocol) local sobre stdio.
//!
//! Permite que agentes e assistentes de IA (Antigravity, Claude Desktop, Cursor, etc.)
//! inspecionem coleções, consultem registros, leiam esquemas e executem operações
//! com segurança.

use forja_core::store::{Banco, Consulta};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

#[derive(Clone)]
pub struct ServidorMcp {
    banco: Arc<Banco>,
    read_write: bool,
}

#[derive(Debug, Deserialize, Serialize)]
pub(crate) struct JsonRpcRequest {
    jsonrpc: String,
    id: Option<Value>,
    method: String,
    #[serde(default)]
    params: Option<Value>,
}

#[derive(Debug, Deserialize, Serialize)]
pub(crate) struct JsonRpcResponse {
    jsonrpc: String,
    id: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<JsonRpcError>,
}

#[derive(Debug, Deserialize, Serialize)]
pub(crate) struct JsonRpcError {
    code: i64,
    message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    data: Option<Value>,
}

impl ServidorMcp {
    pub fn novo(banco: Banco, read_write: bool) -> Self {
        Self {
            banco: Arc::new(banco),
            read_write,
        }
    }

    /// Executa o loop stdio atendendo mensagens JSON-RPC linha a linha.
    pub async fn rodar_stdio(&self) -> std::io::Result<()> {
        let stdin = tokio::io::stdin();
        let mut reader = BufReader::new(stdin);
        let mut stdout = tokio::io::stdout();
        let mut linha = String::new();

        while reader.read_line(&mut linha).await? > 0 {
            let trimmed = linha.trim();
            if trimmed.is_empty() {
                linha.clear();
                continue;
            }

            if let Ok(req) = serde_json::from_str::<JsonRpcRequest>(trimmed) {
                if let Some(resp) = self.processar_requisicao(req).await {
                    let resp_str = serde_json::to_string(&resp).unwrap_or_default();
                    stdout.write_all(resp_str.as_bytes()).await?;
                    stdout.write_all(b"\n").await?;
                    stdout.flush().await?;
                }
            } else {
                let err_resp = json!({
                    "jsonrpc": "2.0",
                    "id": null,
                    "error": {
                        "code": -32700,
                        "message": "Parse error: invalid JSON."
                    }
                });
                let err_str = serde_json::to_string(&err_resp).unwrap_or_default();
                stdout.write_all(err_str.as_bytes()).await?;
                stdout.write_all(b"\n").await?;
                stdout.flush().await?;
            }
            linha.clear();
        }

        Ok(())
    }

    /// Processa uma única requisição JSON-RPC e gera a resposta correspondente.
    pub async fn processar_requisicao(&self, req: JsonRpcRequest) -> Option<JsonRpcResponse> {
        let id = req.id?; // Notificações sem id não esperam resposta

        match req.method.as_str() {
            "initialize" => {
                let resultado = json!({
                    "protocolVersion": "2024-11-05",
                    "capabilities": {
                        "tools": {},
                        "resources": {}
                    },
                    "serverInfo": {
                        "name": "forjabase-mcp",
                        "version": forja_core::VERSION
                    }
                });
                Some(JsonRpcResponse {
                    jsonrpc: "2.0".into(),
                    id,
                    result: Some(resultado),
                    error: None,
                })
            }
            "ping" => Some(JsonRpcResponse {
                jsonrpc: "2.0".into(),
                id,
                result: Some(json!({})),
                error: None,
            }),
            "tools/list" => {
                let tools = json!({
                    "tools": [
                        {
                            "name": "collections_list",
                            "description": "List all collections in the current Forjabase project with their types and basic metadata.",
                            "inputSchema": {
                                "type": "object",
                                "properties": {}
                            }
                        },
                        {
                            "name": "collections_get",
                            "description": "Get detailed schema and field definitions of a specific collection by name or ID.",
                            "inputSchema": {
                                "type": "object",
                                "properties": {
                                    "collection": {
                                        "type": "string",
                                        "description": "The collection name or ID"
                                    }
                                },
                                "required": ["collection"]
                            }
                        },
                        {
                            "name": "records_list",
                            "description": "Query and list records in a collection with optional filter, sort, page, and perPage.",
                            "inputSchema": {
                                "type": "object",
                                "properties": {
                                    "collection": { "type": "string", "description": "Collection name or ID" },
                                    "filter": { "type": "string", "description": "PocketBase filter expression, e.g. status = 'active' && created > '2026-01-01'" },
                                    "sort": { "type": "string", "description": "Fields to sort by, e.g. -created or title" },
                                    "page": { "type": "integer", "description": "Page number (1-based, default: 1)" },
                                    "perPage": { "type": "integer", "description": "Number of records per page (default: 30, max: 100)" }
                                },
                                "required": ["collection"]
                            }
                        },
                        {
                            "name": "records_get",
                            "description": "Get a single record by collection name and record ID.",
                            "inputSchema": {
                                "type": "object",
                                "properties": {
                                    "collection": { "type": "string", "description": "Collection name or ID" },
                                    "id": { "type": "string", "description": "Record ID" }
                                },
                                "required": ["collection", "id"]
                            }
                        },
                        {
                            "name": "records_create",
                            "description": "Create a new record in a collection (requires --read-write mode).",
                            "inputSchema": {
                                "type": "object",
                                "properties": {
                                    "collection": { "type": "string", "description": "Collection name or ID" },
                                    "data": { "type": "object", "description": "JSON object with field values" }
                                },
                                "required": ["collection", "data"]
                            }
                        },
                        {
                            "name": "records_update",
                            "description": "Update an existing record in a collection (requires --read-write mode).",
                            "inputSchema": {
                                "type": "object",
                                "properties": {
                                    "collection": { "type": "string", "description": "Collection name or ID" },
                                    "id": { "type": "string", "description": "Record ID" },
                                    "data": { "type": "object", "description": "JSON object with fields to update" }
                                },
                                "required": ["collection", "id", "data"]
                            }
                        },
                        {
                            "name": "records_delete",
                            "description": "Delete a record by collection name and ID (requires --read-write mode).",
                            "inputSchema": {
                                "type": "object",
                                "properties": {
                                    "collection": { "type": "string", "description": "Collection name or ID" },
                                    "id": { "type": "string", "description": "Record ID to delete" }
                                },
                                "required": ["collection", "id"]
                            }
                        },
                        {
                            "name": "logs_list",
                            "description": "Inspect recent HTTP server request and audit logs.",
                            "inputSchema": {
                                "type": "object",
                                "properties": {
                                    "page": { "type": "integer", "description": "Page number (default: 1)" },
                                    "perPage": { "type": "integer", "description": "Items per page (default: 30)" }
                                }
                            }
                        }
                    ]
                });
                Some(JsonRpcResponse {
                    jsonrpc: "2.0".into(),
                    id,
                    result: Some(tools),
                    error: None,
                })
            }
            "tools/call" => {
                let params = req.params.unwrap_or_default();
                let tool_name = params.get("name").and_then(|v| v.as_str()).unwrap_or("");
                let args = params
                    .get("arguments")
                    .cloned()
                    .unwrap_or_else(|| json!({}));

                let resp_tool = self.chamar_ferramenta(tool_name, args).await;
                Some(JsonRpcResponse {
                    jsonrpc: "2.0".into(),
                    id,
                    result: Some(resp_tool),
                    error: None,
                })
            }
            "resources/list" => {
                let resources = json!({
                    "resources": [
                        {
                            "uri": "forja://schema",
                            "name": "Database Schema",
                            "description": "Complete JSON schema of all collections and their field types in this project.",
                            "mimeType": "application/json"
                        },
                        {
                            "uri": "forja://filter-guide",
                            "name": "Query Filter Syntax Guide",
                            "description": "Documentation for filter expressions, modifiers (:lower, :isset, :length), array operators (?=, ?~), and date macros (@now, @todayStart, etc.).",
                            "mimeType": "text/markdown"
                        }
                    ]
                });
                Some(JsonRpcResponse {
                    jsonrpc: "2.0".into(),
                    id,
                    result: Some(resources),
                    error: None,
                })
            }
            "resources/read" => {
                let params = req.params.unwrap_or_default();
                let uri = params.get("uri").and_then(|v| v.as_str()).unwrap_or("");
                let resp_res = self.ler_recurso(uri).await;
                Some(JsonRpcResponse {
                    jsonrpc: "2.0".into(),
                    id,
                    result: Some(resp_res),
                    error: None,
                })
            }
            _ => Some(JsonRpcResponse {
                jsonrpc: "2.0".into(),
                id,
                result: None,
                error: Some(JsonRpcError {
                    code: -32601,
                    message: format!("Method not found: {}", req.method),
                    data: None,
                }),
            }),
        }
    }

    async fn chamar_ferramenta(&self, nome: &str, args: Value) -> Value {
        match nome {
            "collections_list" => match self.banco.colecoes().await {
                Ok(cols) => {
                    let mut lista = Vec::new();
                    for c in cols {
                        lista.push(json!({
                            "id": c.id,
                            "name": c.name,
                            "type": c.kind,
                            "system": c.system,
                            "fieldsCount": c.fields.len(),
                            "indexesCount": c.indexes.len(),
                        }));
                    }
                    mcp_sucesso(serde_json::to_string_pretty(&lista).unwrap_or_default())
                }
                Err(e) => mcp_erro(e.to_string()),
            },
            "collections_get" => {
                let col_nome = args
                    .get("collection")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                match self.banco.colecao(col_nome).await {
                    Ok(c) => mcp_sucesso(serde_json::to_string_pretty(&c).unwrap_or_default()),
                    Err(e) => mcp_erro(e.to_string()),
                }
            }
            "records_list" => {
                let col_nome = args
                    .get("collection")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                match self.banco.colecao(col_nome).await {
                    Ok(c) => {
                        let page = args.get("page").and_then(|v| v.as_u64()).unwrap_or(1) as u32;
                        let per_page =
                            args.get("perPage").and_then(|v| v.as_u64()).unwrap_or(30) as u32;
                        let filtro = args
                            .get("filter")
                            .and_then(|v| v.as_str())
                            .map(String::from);
                        let ordem = args.get("sort").and_then(|v| v.as_str()).map(String::from);

                        let consulta = Consulta {
                            pagina: page,
                            por_pagina: per_page,
                            filtro,
                            ordem,
                            ..Default::default()
                        };

                        match self.banco.listar(&c, &consulta).await {
                            Ok(lista) => mcp_sucesso(
                                serde_json::to_string_pretty(&lista).unwrap_or_default(),
                            ),
                            Err(e) => mcp_erro(e.to_string()),
                        }
                    }
                    Err(e) => mcp_erro(e.to_string()),
                }
            }
            "records_get" => {
                let col_nome = args
                    .get("collection")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let rec_id = args.get("id").and_then(|v| v.as_str()).unwrap_or("");
                match self.banco.colecao(col_nome).await {
                    Ok(c) => match self.banco.registro(&c, rec_id, None).await {
                        Ok(r) => mcp_sucesso(serde_json::to_string_pretty(&r).unwrap_or_default()),
                        Err(e) => mcp_erro(e.to_string()),
                    },
                    Err(e) => mcp_erro(e.to_string()),
                }
            }
            "records_create" => {
                if !self.read_write {
                    return mcp_erro(
                        "Write operations are disabled in read-only mode. Run 'forja mcp --read-write' to allow modifications.",
                    );
                }
                let col_nome = args
                    .get("collection")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let dados = args.get("data").and_then(|v| v.as_object());
                let Some(obj) = dados else {
                    return mcp_erro("The 'data' argument must be a valid JSON object.");
                };
                match self.banco.colecao(col_nome).await {
                    Ok(c) => match self.banco.criar(&c, obj).await {
                        Ok(r) => mcp_sucesso(serde_json::to_string_pretty(&r).unwrap_or_default()),
                        Err(e) => mcp_erro(e.to_string()),
                    },
                    Err(e) => mcp_erro(e.to_string()),
                }
            }
            "records_update" => {
                if !self.read_write {
                    return mcp_erro(
                        "Write operations are disabled in read-only mode. Run 'forja mcp --read-write' to allow modifications.",
                    );
                }
                let col_nome = args
                    .get("collection")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let rec_id = args.get("id").and_then(|v| v.as_str()).unwrap_or("");
                let dados = args.get("data").and_then(|v| v.as_object());
                let Some(obj) = dados else {
                    return mcp_erro("The 'data' argument must be a valid JSON object.");
                };
                match self.banco.colecao(col_nome).await {
                    Ok(c) => match self.banco.atualizar(&c, rec_id, obj, None).await {
                        Ok(r) => mcp_sucesso(serde_json::to_string_pretty(&r).unwrap_or_default()),
                        Err(e) => mcp_erro(e.to_string()),
                    },
                    Err(e) => mcp_erro(e.to_string()),
                }
            }
            "records_delete" => {
                if !self.read_write {
                    return mcp_erro(
                        "Write operations are disabled in read-only mode. Run 'forja mcp --read-write' to allow modifications.",
                    );
                }
                let col_nome = args
                    .get("collection")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let rec_id = args.get("id").and_then(|v| v.as_str()).unwrap_or("");
                match self.banco.colecao(col_nome).await {
                    Ok(c) => match self.banco.apagar(&c, rec_id, None).await {
                        Ok(()) => mcp_sucesso(format!(
                            "Record '{rec_id}' deleted successfully from '{col_nome}'."
                        )),
                        Err(e) => mcp_erro(e.to_string()),
                    },
                    Err(e) => mcp_erro(e.to_string()),
                }
            }
            "logs_list" => {
                let page = args.get("page").and_then(|v| v.as_u64()).unwrap_or(1) as usize;
                let per_page = args.get("perPage").and_then(|v| v.as_u64()).unwrap_or(30) as usize;
                match self.banco.logs_listar(page, per_page, None).await {
                    Ok(logs) => {
                        mcp_sucesso(serde_json::to_string_pretty(&logs).unwrap_or_default())
                    }
                    Err(e) => mcp_erro(e.to_string()),
                }
            }
            desconhecida => mcp_erro(format!("Unknown tool: {desconhecida}")),
        }
    }

    async fn ler_recurso(&self, uri: &str) -> Value {
        match uri {
            "forja://schema" => match self.banco.colecoes().await {
                Ok(cols) => json!({
                    "contents": [
                        {
                            "uri": uri,
                            "mimeType": "application/json",
                            "text": serde_json::to_string_pretty(&cols).unwrap_or_default()
                        }
                    ]
                }),
                Err(e) => json!({
                    "contents": [
                        {
                            "uri": uri,
                            "mimeType": "text/plain",
                            "text": format!("Error reading schema: {e}")
                        }
                    ]
                }),
            },
            "forja://filter-guide" => {
                let guia = r#"# Forjabase / PocketBase Filter Syntax Guide

### Operators
- `=`, `!=`, `>`, `>=`, `<`, `<=`
- `~` (contains/LIKE), `!~` (not contains)
- `?=` / `?~` (any item matching in arrays or single values)
- `&&` (AND), `||` (OR), `(...)` (parentheses)

### Field Modifiers
- `:lower` -> lower("field") = 'abc'
- `:length` -> length("field") > 5
- `:isset` -> field is not null and not empty

### Date Macros (UTC)
- `@now`, `@yesterday`, `@tomorrow`
- `@todayStart`, `@todayEnd`
- `@monthStart`, `@monthEnd`
- `@yearStart`, `@yearEnd`
"#;
                json!({
                    "contents": [
                        {
                            "uri": uri,
                            "mimeType": "text/markdown",
                            "text": guia
                        }
                    ]
                })
            }
            outro => json!({
                "contents": [
                    {
                        "uri": outro,
                        "mimeType": "text/plain",
                        "text": format!("Resource not found: {outro}")
                    }
                ]
            }),
        }
    }
}

fn mcp_sucesso(conteudo: impl Into<String>) -> Value {
    json!({
        "content": [
            {
                "type": "text",
                "text": conteudo.into()
            }
        ],
        "isError": false
    })
}

fn mcp_erro(mensagem: impl Into<String>) -> Value {
    json!({
        "content": [
            {
                "type": "text",
                "text": mensagem.into()
            }
        ],
        "isError": true
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn mcp_servidor_fluxo_completo() {
        let b = Banco::sqlite_memoria().await.unwrap();
        let col = serde_json::from_value(json!({
            "name": "artigos",
            "fields": [
                { "name": "titulo", "type": "text" }
            ]
        }))
        .unwrap();
        let _c = b.criar_colecao(col).await.unwrap();

        let srv = ServidorMcp::novo(b.clone(), true);

        // 1. initialize
        let resp_init = srv
            .processar_requisicao(JsonRpcRequest {
                jsonrpc: "2.0".into(),
                id: Some(json!(1)),
                method: "initialize".into(),
                params: None,
            })
            .await
            .unwrap();
        assert_eq!(
            resp_init.result.unwrap()["serverInfo"]["name"],
            "forjabase-mcp"
        );

        // 2. tools/list
        let resp_tools = srv
            .processar_requisicao(JsonRpcRequest {
                jsonrpc: "2.0".into(),
                id: Some(json!(2)),
                method: "tools/list".into(),
                params: None,
            })
            .await
            .unwrap();
        let tools = resp_tools.result.unwrap()["tools"]
            .as_array()
            .unwrap()
            .clone();
        assert!(tools.iter().any(|t| t["name"] == "collections_list"));
        assert!(tools.iter().any(|t| t["name"] == "records_create"));

        // 3. tools/call: records_create
        let resp_call_create = srv
            .processar_requisicao(JsonRpcRequest {
                jsonrpc: "2.0".into(),
                id: Some(json!(3)),
                method: "tools/call".into(),
                params: Some(json!({
                    "name": "records_create",
                    "arguments": {
                        "collection": "artigos",
                        "data": { "titulo": "Post Inicial MCP" }
                    }
                })),
            })
            .await
            .unwrap();
        assert_eq!(resp_call_create.result.as_ref().unwrap()["isError"], false);
        let texto = resp_call_create.result.unwrap()["content"][0]["text"]
            .as_str()
            .unwrap()
            .to_string();
        let criado: Value = serde_json::from_str(&texto).unwrap();
        assert_eq!(criado["titulo"], "Post Inicial MCP");
        let rec_id = criado["id"].as_str().unwrap();

        // 4. tools/call: records_list
        let resp_call_list = srv
            .processar_requisicao(JsonRpcRequest {
                jsonrpc: "2.0".into(),
                id: Some(json!(4)),
                method: "tools/call".into(),
                params: Some(json!({
                    "name": "records_list",
                    "arguments": {
                        "collection": "artigos"
                    }
                })),
            })
            .await
            .unwrap();
        let texto_list = resp_call_list.result.unwrap()["content"][0]["text"]
            .as_str()
            .unwrap()
            .to_string();
        assert!(texto_list.contains("Post Inicial MCP"));

        // 5. tools/call: records_get
        let resp_call_get = srv
            .processar_requisicao(JsonRpcRequest {
                jsonrpc: "2.0".into(),
                id: Some(json!(5)),
                method: "tools/call".into(),
                params: Some(json!({
                    "name": "records_get",
                    "arguments": {
                        "collection": "artigos",
                        "id": rec_id
                    }
                })),
            })
            .await
            .unwrap();
        let texto_get = resp_call_get.result.unwrap()["content"][0]["text"]
            .as_str()
            .unwrap()
            .to_string();
        assert!(texto_get.contains("Post Inicial MCP"));

        // 6. resources/read: schema e filter-guide
        let resp_res_schema = srv
            .processar_requisicao(JsonRpcRequest {
                jsonrpc: "2.0".into(),
                id: Some(json!(6)),
                method: "resources/read".into(),
                params: Some(json!({ "uri": "forja://schema" })),
            })
            .await
            .unwrap();
        let schema_text = resp_res_schema.result.unwrap()["contents"][0]["text"]
            .as_str()
            .unwrap()
            .to_string();
        assert!(schema_text.contains("artigos"));

        // 7. Teste de modo somente leitura
        let srv_ro = ServidorMcp::novo(b, false);
        let resp_ro_create = srv_ro
            .processar_requisicao(JsonRpcRequest {
                jsonrpc: "2.0".into(),
                id: Some(json!(7)),
                method: "tools/call".into(),
                params: Some(json!({
                    "name": "records_create",
                    "arguments": {
                        "collection": "artigos",
                        "data": { "titulo": "Tentativa bloqueada" }
                    }
                })),
            })
            .await
            .unwrap();
        assert_eq!(resp_ro_create.result.unwrap()["isError"], true);
    }
}
