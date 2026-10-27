//! Barramento in-process das escritas de registro (ADR-001, marco M1).
//!
//! `criar`, `atualizar` e `apagar` publicam depois do commit. Sem assinante,
//! o `send` do `broadcast` falha e a escrita fica. Com assinante lento, o
//! canal descarta o evento antigo (`Lagged`) e a escrita também fica.

use serde_json::Value;
use tokio::sync::broadcast;

/// Quantos eventos um assinante pode deixar acumular antes de perder os mais
/// antigos. Acima disso a escrita continua; só o assinante fica para trás.
pub const CAPACIDADE: usize = 32;

/// O que aconteceu com o registro.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Acao {
    Criar,
    Atualizar,
    Apagar,
}

/// Um registro acabou de ser gravado. `colecao` é o nome, não o id.
/// `registro` é o estado depois da escrita; no apagar, o último estado.
#[derive(Debug, Clone, PartialEq)]
pub struct Evento {
    pub colecao: String,
    pub acao: Acao,
    pub id: String,
    pub registro: Value,
}

/// Canal compartilhado por um `Banco`. Clonar o banco clona o remetente:
/// os dois publicam no mesmo canal.
#[derive(Clone)]
pub struct Barramento {
    tx: broadcast::Sender<Evento>,
}

impl Barramento {
    pub fn novo() -> Barramento {
        let (tx, _rx) = broadcast::channel(CAPACIDADE);
        Barramento { tx }
    }

    pub fn assinar(&self) -> broadcast::Receiver<Evento> {
        self.tx.subscribe()
    }

    /// Ignora a ausência de assinantes. Nunca desfaz a escrita que já commitou.
    pub fn publicar(&self, evento: Evento) {
        let _ = self.tx.send(evento);
    }
}
