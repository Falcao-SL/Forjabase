//! SSE no protocolo do PocketBase (`GET` abre, `POST` assina).
//!
//! O evento sai do barramento de escrita, depois do commit. A regra da coleção
//! é reavaliada para cada cliente: `listRule` na coleção inteira, `viewRule`
//! num registro. Sem replay.

use super::{Chamador, ErroApi, Estado, esconder};
use axum::Json;
use axum::extract::{ConnectInfo, FromRequestParts, State};
use axum::http::request::Parts;
use axum::response::{IntoResponse, Response};
use forja_core::barramento::{Acao, Evento};
use forja_core::erro::Erro;
use forja_core::filtro::Contexto;
use forja_core::store::{Banco, Superusuario};
use forja_core::{Collection, CollectionType, id};
use futures_util::Stream;
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::convert::Infallible;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::task::{Context, Poll};
use std::time::Duration;
use tokio::sync::{Mutex, mpsc};

const OCIOSO_MS: u64 = 5 * 60 * 1000;
const MAXIMO: Duration = Duration::from_secs(30 * 60);

/// Quem o cliente é, para a regra e para esconder campos.
#[derive(Clone)]
pub struct SessaoRt {
    pub chave: String,
    pub superusuario: Option<Superusuario>,
    pub colecao: Option<Collection>,
    pub registro: Option<Value>,
}

impl SessaoRt {
    fn anon() -> SessaoRt {
        SessaoRt {
            chave: String::new(),
            superusuario: None,
            colecao: None,
            registro: None,
        }
    }

    pub fn de(quem: &Chamador) -> SessaoRt {
        match &quem.0 {
            Some(super::Identidade::Superusuario(su)) => SessaoRt {
                chave: format!("su:{}", su.id),
                superusuario: Some(su.clone()),
                colecao: None,
                registro: None,
            },
            Some(super::Identidade::Registro { colecao, registro }) => {
                let id = registro
                    .get("id")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                SessaoRt {
                    chave: format!("{}:{id}", colecao.id),
                    superusuario: None,
                    colecao: Some(colecao.clone()),
                    registro: Some(registro.clone()),
                }
            }
            None => SessaoRt::anon(),
        }
    }

    fn chamador(&self) -> Chamador {
        if let Some(su) = &self.superusuario {
            Chamador(Some(super::Identidade::Superusuario(su.clone())))
        } else if let (Some(colecao), Some(registro)) = (&self.colecao, &self.registro) {
            Chamador(Some(super::Identidade::Registro {
                colecao: colecao.clone(),
                registro: registro.clone(),
            }))
        } else {
            Chamador(None)
        }
    }

    fn contexto(&self) -> Contexto {
        Contexto {
            auth: self.registro.as_ref().and_then(Value::as_object).cloned(),
        }
    }
}

struct Inscricao {
    colecao: String,
    /// `None` é a coleção inteira (`nome` ou `nome/*`).
    id: Option<String>,
    filtro: Option<String>,
    bruto: String,
}

struct Cliente {
    tx: mpsc::UnboundedSender<String>,
    sessao: SessaoRt,
    inscricoes: Vec<Inscricao>,
    ip: Option<String>,
    ultimo: Arc<AtomicU64>,
}

pub struct Hub {
    clientes: Mutex<HashMap<String, Cliente>>,
}

impl Hub {
    pub fn novo() -> Arc<Hub> {
        Arc::new(Hub {
            clientes: Mutex::new(HashMap::new()),
        })
    }

    pub async fn distribuir(self: &Arc<Self>, banco: &Banco, ev: Evento) {
        let detalhes: Vec<(String, SessaoRt, Vec<Inscricao>, Arc<AtomicU64>)> = {
            let g = self.clientes.lock().await;
            if g.is_empty() {
                return;
            }
            g.iter()
                .map(|(id, c)| {
                    (
                        id.clone(),
                        c.sessao.clone(),
                        c.inscricoes
                            .iter()
                            .map(|i| Inscricao {
                                colecao: i.colecao.clone(),
                                id: i.id.clone(),
                                filtro: i.filtro.clone(),
                                bruto: i.bruto.clone(),
                            })
                            .collect(),
                        c.ultimo.clone(),
                    )
                })
                .collect()
        };
        let Ok(colecao) = banco.colecao(&ev.colecao).await else {
            return;
        };
        if colecao.kind == CollectionType::View {
            return;
        }
        let mut mortos = Vec::new();
        for (id_cliente, sessao, inscricoes, ultimo) in detalhes {
            let tx = {
                let g = self.clientes.lock().await;
                g.get(&id_cliente).map(|c| c.tx.clone())
            };
            let Some(tx) = tx else { continue };
            for ins in &inscricoes {
                if !ins.colecao.eq_ignore_ascii_case(&ev.colecao) {
                    continue;
                }
                if ins.id.as_ref().is_some_and(|i| i != &ev.id) {
                    continue;
                }
                if !permitido(banco, &colecao, &ev, ins, &sessao).await {
                    continue;
                }
                let bruto = if ev.acao == Acao::Apagar {
                    ev.registro.clone()
                } else {
                    match banco.registro(&colecao, &ev.id, None).await {
                        Ok(v) => v,
                        Err(_) => continue,
                    }
                };
                let registro = esconder(&colecao, &sessao.chamador(), bruto);
                let data = json!({
                    "action": acao_nome(ev.acao),
                    "record": registro,
                });
                let quadro = quadro(&id_cliente, &ins.bruto, &data.to_string());
                if tx.send(quadro).is_err() {
                    mortos.push(id_cliente.clone());
                    break;
                }
                ultimo.store(agora_ms(), Ordering::Relaxed);
            }
        }
        if !mortos.is_empty() {
            let mut g = self.clientes.lock().await;
            for id in mortos {
                g.remove(&id);
            }
        }
    }
}

fn acao_nome(a: Acao) -> &'static str {
    match a {
        Acao::Criar => "create",
        Acao::Atualizar => "update",
        Acao::Apagar => "delete",
    }
}

fn quadro(client_id: &str, evento: &str, data: &str) -> String {
    format!("id:{client_id}\nevent:{evento}\ndata:{data}\n\n")
}

async fn permitido(
    banco: &Banco,
    c: &forja_core::Collection,
    ev: &Evento,
    ins: &Inscricao,
    sessao: &SessaoRt,
) -> bool {
    let regra_campo = if ins.id.is_some() {
        &c.view_rule
    } else {
        &c.list_rule
    };
    let base = match super::regra_para(&sessao.chamador(), regra_campo) {
        Ok(r) => r,
        Err(_) => return false,
    };
    let combinado = combinar(base.as_deref(), ins.filtro.as_deref());
    let ctx = sessao.contexto();
    let ok = if ev.acao == Acao::Apagar {
        banco
            .snapshot_visivel(c, &ev.registro, combinado.as_deref(), &ctx)
            .await
    } else {
        banco.visivel(c, &ev.id, combinado.as_deref(), &ctx).await
    };
    ok.unwrap_or(false)
}

fn combinar(regra: Option<&str>, filtro: Option<&str>) -> Option<String> {
    match (
        regra.filter(|s| !s.trim().is_empty()),
        filtro.filter(|s| !s.trim().is_empty()),
    ) {
        (None, None) => None,
        (Some(a), None) => Some(a.to_string()),
        (None, Some(b)) => Some(b.to_string()),
        (Some(a), Some(b)) => Some(format!("({a}) && ({b})")),
    }
}

pub async fn escutar(banco: Banco, hub: Arc<Hub>) {
    let mut rx = banco.assinar();
    loop {
        match rx.recv().await {
            Ok(ev) => hub.distribuir(&banco, ev).await,
            Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
            Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
        }
    }
}

pub struct Ip(pub Option<String>);

impl FromRequestParts<Estado> for Ip {
    type Rejection = Infallible;

    async fn from_request_parts(parts: &mut Parts, _: &Estado) -> Result<Self, Self::Rejection> {
        let ip = parts
            .extensions
            .get::<ConnectInfo<SocketAddr>>()
            .map(|c| c.0.ip().to_string());
        Ok(Ip(ip))
    }
}

struct CorpoSse {
    rx: mpsc::UnboundedReceiver<String>,
}

impl Stream for CorpoSse {
    type Item = Result<String, std::io::Error>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        match Pin::new(&mut self.get_mut().rx).poll_recv(cx) {
            Poll::Ready(Some(s)) => Poll::Ready(Some(Ok(s))),
            Poll::Ready(None) => Poll::Ready(None),
            Poll::Pending => Poll::Pending,
        }
    }
}

pub async fn conectar(State(e): State<Estado>, ip: Ip) -> Response {
    let client_id = id::aleatorio(40);
    let (tx, rx) = mpsc::unbounded_channel();
    let ultimo = Arc::new(AtomicU64::new(agora_ms()));
    {
        let mut g = e.realtime.clientes.lock().await;
        g.insert(
            client_id.clone(),
            Cliente {
                tx: tx.clone(),
                sessao: SessaoRt::anon(),
                inscricoes: Vec::new(),
                ip: ip.0,
                ultimo: ultimo.clone(),
            },
        );
    }
    let _ = tx.send(quadro(
        &client_id,
        "PB_CONNECT",
        &json!({ "clientId": client_id }).to_string(),
    ));
    let hub = e.realtime.clone();
    let id_tarefa = client_id.clone();
    let ultimo_tarefa = ultimo.clone();
    tokio::spawn(async move {
        let inicio = std::time::Instant::now();
        loop {
            tokio::time::sleep(Duration::from_secs(30)).await;
            if inicio.elapsed() >= MAXIMO {
                break;
            }
            let ocioso = agora_ms().saturating_sub(ultimo_tarefa.load(Ordering::Relaxed));
            if ocioso >= OCIOSO_MS {
                break;
            }
        }
        hub.clientes.lock().await.remove(&id_tarefa);
    });
    let corpo = axum::body::Body::from_stream(CorpoSse { rx });
    Response::builder()
        .header("content-type", "text/event-stream")
        .header("cache-control", "no-store")
        .header("x-accel-buffering", "no")
        .body(corpo)
        .unwrap_or_else(|_| axum::http::StatusCode::INTERNAL_SERVER_ERROR.into_response())
}

#[derive(Deserialize)]
pub(crate) struct Pedido {
    #[serde(rename = "clientId")]
    client_id: String,
    #[serde(default)]
    subscriptions: Vec<String>,
}

pub async fn inscrever(
    State(e): State<Estado>,
    quem: Chamador,
    ip: Ip,
    Json(pedido): Json<Pedido>,
) -> Result<axum::http::StatusCode, ErroApi> {
    let mut inscricoes = Vec::with_capacity(pedido.subscriptions.len());
    for bruto in &pedido.subscriptions {
        inscricoes.push(analisar(bruto).map_err(|m| ErroApi(Erro::invalido(m)))?);
    }
    let mut g = e.realtime.clientes.lock().await;
    let Some(cliente) = g.get_mut(&pedido.client_id) else {
        return Err(ErroApi(Erro::NaoEncontrado(
            "Missing or invalid client id.".into(),
        )));
    };
    if let (Some(nova), Some(atual)) = (&ip.0, &cliente.ip)
        && nova != atual
    {
        return Err(ErroApi(Erro::Proibido(
            "The subscription must come from the same address as the connection.".into(),
        )));
    }
    let nova = SessaoRt::de(&quem);
    if !nova.chave.is_empty()
        && !cliente.sessao.chave.is_empty()
        && nova.chave != cliente.sessao.chave
    {
        return Err(ErroApi(Erro::Proibido(
            "The realtime auth record cannot be changed.".into(),
        )));
    }
    if !nova.chave.is_empty() {
        cliente.sessao = nova;
    }
    cliente.inscricoes = inscricoes;
    cliente.ultimo.store(agora_ms(), Ordering::Relaxed);
    Ok(axum::http::StatusCode::NO_CONTENT)
}

fn analisar(bruto: &str) -> Result<Inscricao, String> {
    if bruto.len() > 2000 {
        return Err("Invalid subscription.".into());
    }
    let (caminho, query) = bruto.split_once('?').unwrap_or((bruto, ""));
    let filtro = filtro_de_opcoes(query)?;
    let mut partes = caminho.split('/');
    let colecao = partes.next().unwrap_or("");
    if !nome_colecao(colecao) {
        return Err("Invalid subscription.".into());
    }
    let id = match partes.next() {
        None | Some("*") => None,
        Some(id) if id::valido(id) => Some(id.to_string()),
        _ => return Err("Invalid subscription.".into()),
    };
    if partes.next().is_some() {
        return Err("Invalid subscription.".into());
    }
    Ok(Inscricao {
        colecao: colecao.to_string(),
        id,
        filtro,
        bruto: bruto.to_string(),
    })
}

fn filtro_de_opcoes(query: &str) -> Result<Option<String>, String> {
    if query.is_empty() {
        return Ok(None);
    }
    let mut filtro = None;
    for (k, v) in form_urlencoded::parse(query.as_bytes()) {
        if k != "options" {
            continue;
        }
        let valor: Value =
            serde_json::from_str(&v).map_err(|_| "Invalid subscription.".to_string())?;
        filtro = valor
            .get("query")
            .and_then(|q| q.get("filter"))
            .and_then(Value::as_str)
            .map(str::to_string)
            .filter(|s| !s.trim().is_empty());
    }
    Ok(filtro)
}

fn nome_colecao(n: &str) -> bool {
    let mut cs = n.chars();
    matches!(cs.next(), Some(c) if c.is_ascii_alphabetic() || c == '_')
        && cs.all(|c| c.is_ascii_alphanumeric() || c == '_')
        && n.len() <= 100
}

fn agora_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}
