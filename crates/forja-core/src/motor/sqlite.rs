//! SQLite pelo rusqlite: o motor padrão, comprovado, um arquivo por projeto.
//! O rusqlite é síncrono; cada chamada roda em `spawn_blocking`.

use super::{Executor, Linha};
use crate::erro::{Erro, Resultado};
use crate::filtro::Dialeto;
use crate::valor::Valor;
use async_trait::async_trait;
use rusqlite::types::Value as Sv;
use rusqlite::{Connection, params_from_iter};
use std::path::Path;
use std::sync::{Arc, Mutex};

pub struct Sqlite {
    conn: Arc<Mutex<Connection>>,
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
        })
    }

    async fn com<T: Send + 'static>(
        &self,
        f: impl FnOnce(&mut Connection) -> rusqlite::Result<T> + Send + 'static,
    ) -> Resultado<T> {
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
}
