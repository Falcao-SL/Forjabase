//! Gerenciador de múltiplos projetos (multi-tenancy) e importação de `pb_data`.

use axum::Json;
use axum::extract::{Path, Request, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use forja_core::Erro;
use forja_core::correio::Correio;
use forja_core::store::{Banco, Motor};
use serde_json::json;
use std::collections::HashMap;
use std::path::{Path as StdPath, PathBuf};
use std::sync::Arc;
use tokio::sync::RwLock;
use tower::ServiceExt;

use crate::{Estado, router};

/// Gerenciador de múltiplos projetos no Forja.
#[derive(Clone)]
pub struct GerenciadorProjetos {
    pub dir: PathBuf,
    pub motor: Motor,
    pub correio: Arc<dyn Correio>,
    cache: Arc<RwLock<HashMap<String, Estado>>>,
}

impl GerenciadorProjetos {
    pub fn novo(dir: PathBuf, motor: Motor, correio: Arc<dyn Correio>) -> Self {
        Self {
            dir,
            motor,
            correio,
            cache: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub async fn existe(&self, nome: &str) -> bool {
        let nome = nome.trim();
        if nome == "default" {
            return true;
        }
        {
            let lock = self.cache.read().await;
            if lock.contains_key(nome) {
                return true;
            }
        }
        self.dir.join(nome).exists()
    }

    pub async fn obter(&self, nome: &str) -> Result<Estado, Erro> {
        let nome = nome.trim();
        if !nome_valido(nome) {
            return Err(Erro::invalido("Invalid project name."));
        }
        if nome != "default" && !self.dir.join(nome).exists() {
            let lock = self.cache.read().await;
            if !lock.contains_key(nome) {
                return Err(Erro::nao_encontrado(format!("Project '{nome}' not found.")));
            }
        }
        self.abrir_interno(nome).await
    }

    pub async fn criar(&self, nome: &str) -> Result<Estado, Erro> {
        let nome = nome.trim();
        if !nome_valido(nome) {
            return Err(Erro::invalido("Invalid project name."));
        }
        self.abrir_interno(nome).await
    }

    async fn abrir_interno(&self, nome: &str) -> Result<Estado, Erro> {
        {
            let lock = self.cache.read().await;
            if let Some(e) = lock.get(nome) {
                return Ok(e.clone());
            }
        }
        let banco = Banco::abrir(&self.motor, &self.dir, nome).await?;
        let estado = Estado::com_correio(banco, self.correio.clone()).await?;
        let mut lock = self.cache.write().await;
        lock.insert(nome.to_string(), estado.clone());
        Ok(estado)
    }

    pub async fn listar(&self) -> Vec<String> {
        let mut nomes = vec!["default".to_string()];
        if let Ok(entries) = std::fs::read_dir(&self.dir) {
            for entry in entries.flatten() {
                if let Ok(file_type) = entry.file_type()
                    && file_type.is_dir()
                {
                    let name = entry.file_name().to_string_lossy().to_string();
                    if nome_valido(&name) && name != "default" {
                        nomes.push(name);
                    }
                }
            }
        }
        nomes.sort();
        nomes.dedup();
        nomes
    }

    pub async fn apagar(&self, nome: &str) -> Result<(), Erro> {
        if nome == "default" {
            return Err(Erro::invalido("The default project cannot be deleted."));
        }
        {
            let mut lock = self.cache.write().await;
            lock.remove(nome);
        }
        let p = self.dir.join(nome);
        if p.exists() {
            std::fs::remove_dir_all(p).map_err(|e| Erro::invalido(e.to_string()))?;
        }
        Ok(())
    }

    pub async fn importar_pb(&self, pb_data_dir: &StdPath, nome: &str) -> Result<Estado, Erro> {
        let nome = nome.trim();
        if !nome_valido(nome) {
            return Err(Erro::invalido("Invalid project name."));
        }
        let db_origem = pb_data_dir.join("data.db");
        if !db_origem.exists() {
            return Err(Erro::invalido(
                "data.db not found in specified pb_data directory.",
            ));
        }
        let proj_dir = self.dir.join(nome);
        std::fs::create_dir_all(&proj_dir).map_err(|e| Erro::invalido(e.to_string()))?;
        let db_destino = proj_dir.join("data.db");
        std::fs::copy(&db_origem, &db_destino).map_err(|e| Erro::invalido(e.to_string()))?;

        let storage_origem = pb_data_dir.join("storage");
        if storage_origem.exists() {
            let storage_destino = proj_dir.join("storage");
            let _ = copiar_dir_recursivo(&storage_origem, &storage_destino);
        }

        self.abrir_interno(nome).await
    }
}

fn nome_valido(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 100
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

fn copiar_dir_recursivo(src: &StdPath, dst: &StdPath) -> std::io::Result<()> {
    if !dst.exists() {
        std::fs::create_dir_all(dst)?;
    }
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        let target = dst.join(entry.file_name());
        if file_type.is_dir() {
            copiar_dir_recursivo(&entry.path(), &target)?;
        } else {
            std::fs::copy(entry.path(), target)?;
        }
    }
    Ok(())
}

/// Despacha requisição na raiz do projeto `/p/{projeto}`.
pub async fn despachar_projeto_raiz(
    State(projetos): State<Arc<GerenciadorProjetos>>,
    Path(projeto): Path<String>,
    req: Request,
) -> Response {
    despachar_projeto_interno(projetos, projeto, req).await
}

/// Despacha uma requisição para o projeto especificado em `/p/{projeto}/...`.
pub async fn despachar_projeto(
    State(projetos): State<Arc<GerenciadorProjetos>>,
    Path((projeto, _resto)): Path<(String, String)>,
    req: Request,
) -> Response {
    despachar_projeto_interno(projetos, projeto, req).await
}

async fn despachar_projeto_interno(
    projetos: Arc<GerenciadorProjetos>,
    projeto: String,
    mut req: Request,
) -> Response {
    let estado = match projetos.obter(&projeto).await {
        Ok(e) => e,
        Err(_) => {
            return (
                StatusCode::NOT_FOUND,
                Json(json!({
                    "code": 404,
                    "message": format!("Project '{projeto}' not found."),
                    "data": {}
                })),
            )
                .into_response();
        }
    };

    let uri = req.uri().clone();
    let path = uri.path();
    let prefix = format!("/p/{projeto}");
    let novo_path = path.strip_prefix(&prefix).unwrap_or(path);
    let novo_path = if novo_path.is_empty() { "/" } else { novo_path };
    let novo_uri = match uri.query() {
        Some(q) => format!("{novo_path}?{q}"),
        None => novo_path.to_string(),
    };
    if let Ok(u) = novo_uri.parse() {
        *req.uri_mut() = u;
    }

    router(estado).oneshot(req).await.unwrap_or_else(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "code": 500, "message": e.to_string(), "data": {} })),
        )
            .into_response()
    })
}
