//! SQLite pelo rusqlite: o motor padrão, comprovado, um arquivo por projeto.
//! O rusqlite é síncrono; cada chamada roda em `spawn_blocking`.

use super::{Executor, Guardiao, Linha};
use crate::erro::{Erro, Resultado};
use crate::filtro::Dialeto;
use crate::valor::Valor;
use async_trait::async_trait;
use rusqlite::types::Value as Sv;
use rusqlite::{Connection, params_from_iter};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

#[derive(Clone)]
pub struct Sqlite {
    conn: Arc<Mutex<Connection>>,
    /// Uma escrita de lote segura a conexão até o commit. Quem já é o dono
    /// (a mesma tarefa) entra de novo; as outras esperam.
    trava: Arc<tokio::sync::Mutex<()>>,
    dono: Arc<Mutex<Option<DonoTrava>>>,
}

struct DonoTrava {
    id: tokio::task::Id,
    profundidade: usize,
    /// Segura o mutex enquanto a transação está aberta. Não é lido.
    #[allow(dead_code)]
    guarda: Option<tokio::sync::OwnedMutexGuard<()>>,
}

impl Sqlite {
    pub fn abrir(arquivo: &Path) -> Resultado<Sqlite> {
        Sqlite::iniciar(Connection::open(arquivo).map_err(erro)?)
    }

    pub fn em_memoria() -> Resultado<Sqlite> {
        Sqlite::iniciar(Connection::open_in_memory().map_err(erro)?)
    }

    fn iniciar(conn: Connection) -> Resultado<Sqlite> {
        conn.execute_batch(
            "PRAGMA journal_mode = WAL; PRAGMA foreign_keys = ON; PRAGMA busy_timeout = 10000; PRAGMA synchronous = NORMAL;",
        )
        .map_err(erro)?;
        Ok(Sqlite {
            conn: Arc::new(Mutex::new(conn)),
            trava: Arc::new(tokio::sync::Mutex::new(())),
            dono: Arc::new(Mutex::new(None)),
        })
    }

    async fn adquirir(&self) -> GuardaSqlite {
        if let Some(id) = tokio::task::try_id() {
            let mut g = self.dono.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(d) = g.as_mut()
                && d.id == id
            {
                d.profundidade += 1;
                return GuardaSqlite {
                    dono: self.dono.clone(),
                    direta: None,
                    conta: true,
                };
            }
        }
        let guarda = self.trava.clone().lock_owned().await;
        if let Some(id) = tokio::task::try_id() {
            let mut g = self.dono.lock().unwrap_or_else(|e| e.into_inner());
            *g = Some(DonoTrava {
                id,
                profundidade: 1,
                guarda: Some(guarda),
            });
            GuardaSqlite {
                dono: self.dono.clone(),
                direta: None,
                conta: true,
            }
        } else {
            GuardaSqlite {
                dono: self.dono.clone(),
                direta: Some(guarda),
                conta: false,
            }
        }
    }

    async fn com<T: Send + 'static>(
        &self,
        f: impl FnOnce(&mut Connection) -> rusqlite::Result<T> + Send + 'static,
    ) -> Resultado<T> {
        let _g = self.adquirir().await;
        let conn = self.conn.clone();
        tokio::task::spawn_blocking(move || {
            let mut c = conn.lock().unwrap_or_else(|e| e.into_inner());
            f(&mut c)
        })
        .await
        .map_err(|e| Erro::Interno(e.to_string()))?
        .map_err(erro)
    }
}

struct GuardaSqlite {
    dono: Arc<Mutex<Option<DonoTrava>>>,
    direta: Option<tokio::sync::OwnedMutexGuard<()>>,
    /// Esta guarda incrementou `profundidade` e precisa devolver.
    conta: bool,
}

impl Drop for GuardaSqlite {
    fn drop(&mut self) {
        self.direta.take();
        if !self.conta {
            return;
        }
        let mut g = self.dono.lock().unwrap_or_else(|e| e.into_inner());
        let zerar = g.as_mut().is_some_and(|d| {
            d.profundidade = d.profundidade.saturating_sub(1);
            d.profundidade == 0
        });
        if zerar {
            *g = None;
        }
    }
}

struct TxSqlite {
    sqlite: Sqlite,
    _guarda: GuardaSqlite,
    encerrada: AtomicBool,
}

impl TxSqlite {
    fn rollback_sync(&self) {
        let conn = self.sqlite.conn.clone();
        let c = conn.lock().unwrap_or_else(|e| e.into_inner());
        let _ = c.execute_batch("ROLLBACK");
    }
}

impl Drop for TxSqlite {
    fn drop(&mut self) {
        if !self.encerrada.swap(true, Ordering::SeqCst) {
            self.rollback_sync();
        }
    }
}

#[async_trait]
impl Guardiao for TxSqlite {
    async fn commit(&self) -> Resultado<()> {
        self.sqlite.executar("COMMIT", vec![]).await?;
        self.encerrada.store(true, Ordering::SeqCst);
        Ok(())
    }

    async fn rollback(&self) -> Resultado<()> {
        if !self.encerrada.swap(true, Ordering::SeqCst) {
            self.rollback_sync();
        }
        Ok(())
    }
}

fn erro(e: rusqlite::Error) -> Erro {
    match &e {
        rusqlite::Error::SqliteFailure(f, _)
            if f.code == rusqlite::ErrorCode::ConstraintViolation =>
        {
            Erro::Duplicado(e.to_string())
        }
        _ => Erro::Interno(e.to_string()),
    }
}

fn para_sv(v: Valor) -> Sv {
    match v {
        Valor::Nulo => Sv::Null,
        Valor::Int(i) => Sv::Integer(i),
        Valor::Real(x) => Sv::Real(x),
        Valor::Texto(t) => Sv::Text(t),
        Valor::Bool(b) => Sv::Integer(b as i64),
    }
}

fn de_sv(v: Sv) -> Valor {
    match v {
        Sv::Null => Valor::Nulo,
        Sv::Integer(i) => Valor::Int(i),
        Sv::Real(x) => Valor::Real(x),
        Sv::Text(t) => Valor::Texto(t),
        Sv::Blob(b) => Valor::Texto(String::from_utf8_lossy(&b).into_owned()),
    }
}

#[async_trait]
impl Executor for Sqlite {
    fn dialeto(&self) -> Dialeto {
        Dialeto::Sqlite
    }

    fn nome(&self) -> &'static str {
        "sqlite"
    }

    async fn executar(&self, sql: &str, params: Vec<Valor>) -> Resultado<u64> {
        let sql = sql.to_string();
        self.com(move |c| {
            c.execute(&sql, params_from_iter(params.into_iter().map(para_sv)))
                .map(|n| n as u64)
        })
        .await
    }

    async fn consultar(&self, sql: &str, params: Vec<Valor>) -> Resultado<Vec<Linha>> {
        let sql = sql.to_string();
        self.com(move |c| {
            let mut st = c.prepare(&sql)?;
            let n = st.column_count();
            let linhas = st
                .query_map(params_from_iter(params.into_iter().map(para_sv)), |r| {
                    (0..n).map(|i| r.get::<_, Sv>(i).map(de_sv)).collect()
                })?
                .collect::<rusqlite::Result<Vec<Linha>>>()?;
            Ok(linhas)
        })
        .await
    }

    async fn lote(&self, comandos: Vec<(String, Vec<Valor>)>) -> Resultado<()> {
        self.com(move |c| {
            let tx = c.transaction()?;
            for (sql, params) in comandos {
                if params.is_empty() {
                    tx.execute_batch(&sql)?;
                } else {
                    tx.execute(&sql, params_from_iter(params.into_iter().map(para_sv)))?;
                }
            }
            tx.commit()
        })
        .await
    }

    async fn abrir_transacao(&self) -> Resultado<Box<dyn Guardiao>> {
        let guarda = self.adquirir().await;
        if let Err(e) = self.executar("BEGIN IMMEDIATE", vec![]).await {
            drop(guarda);
            return Err(e);
        }
        Ok(Box::new(TxSqlite {
            sqlite: self.clone(),
            _guarda: guarda,
            encerrada: AtomicBool::new(false),
        }))
    }
}
