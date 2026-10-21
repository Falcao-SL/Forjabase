//! Turso: banco em Rust compatível com o arquivo do SQLite, com escrita
//! concorrente (MVCC), captura de mudanças (CDC), vetores e criptografia.
//! Fala o mesmo SQL do SQLite, então usa o dialeto do SQLite.
//! https://github.com/tursodatabase/turso (MIT)

use super::{Executor, Linha};
use crate::erro::{Erro, Resultado};
use crate::filtro::Dialeto;
use crate::valor::Valor;
use async_trait::async_trait;
use tokio::sync::Mutex;
use turso::{Builder, Connection, Database, Value as Tv};

pub struct Turso {
    // O banco precisa viver enquanto a conexão existir.
    _db: Database,
    conn: Mutex<Connection>,
}

impl Turso {
    /// Abre (ou cria) um arquivo; `":memory:"` para um banco em memória.
    pub async fn abrir(caminho: &str) -> Resultado<Turso> {
        let db = Builder::new_local(caminho).build().await.map_err(erro)?;
        let conn = db.connect().map_err(erro)?;
        conn.busy_timeout(std::time::Duration::from_secs(10))
            .map_err(erro)?;
        Ok(Turso {
            _db: db,
            conn: Mutex::new(conn),
        })
    }
}

fn erro(e: turso::Error) -> Erro {
    let m = e.to_string();
    if m.contains("UNIQUE") || m.to_lowercase().contains("constraint") {
        Erro::Duplicado(m)
    } else {
        Erro::Interno(m)
    }
}

fn para_tv(v: Valor) -> Tv {
    match v {
        Valor::Nulo => Tv::Null,
        Valor::Int(i) => Tv::Integer(i),
        Valor::Real(x) => Tv::Real(x),
        Valor::Texto(t) => Tv::Text(t),
        Valor::Bool(b) => Tv::Integer(b as i64),
    }
}

fn de_tv(v: Tv) -> Valor {
    match v {
        Tv::Null => Valor::Nulo,
        Tv::Integer(i) => Valor::Int(i),
        Tv::Real(x) => Valor::Real(x),
        Tv::Text(t) => Valor::Texto(t),
        Tv::Blob(b) => Valor::Texto(String::from_utf8_lossy(&b).into_owned()),
    }
}

fn params(v: Vec<Valor>) -> turso::params::Params {
    turso::params::Params::Positional(v.into_iter().map(para_tv).collect())
}

#[async_trait]
impl Executor for Turso {
    fn dialeto(&self) -> Dialeto {
        Dialeto::Sqlite
    }

    fn nome(&self) -> &'static str {
        "turso"
    }

    async fn executar(&self, sql: &str, p: Vec<Valor>) -> Resultado<u64> {
        let c = self.conn.lock().await;
        c.execute(sql, params(p)).await.map_err(erro)
    }

    async fn consultar(&self, sql: &str, p: Vec<Valor>) -> Resultado<Vec<Linha>> {
        let c = self.conn.lock().await;
        let mut rows = c.query(sql, params(p)).await.map_err(erro)?;
        let mut out = vec![];
        while let Some(r) = rows.next().await.map_err(erro)? {
            let n = r.column_count();
            let mut linha = Vec::with_capacity(n);
            for i in 0..n {
                linha.push(de_tv(r.get_value(i).map_err(erro)?));
            }
            out.push(linha);
        }
        Ok(out)
    }

    async fn lote(&self, comandos: Vec<(String, Vec<Valor>)>) -> Resultado<()> {
        let c = self.conn.lock().await;
        c.execute("BEGIN", ()).await.map_err(erro)?;
        for (sql, p) in comandos {
            let r = if p.is_empty() {
                c.execute_batch(&sql).await
            } else {
                c.execute(&sql, params(p)).await.map(|_| ())
            };
            if let Err(e) = r {
                let _ = c.execute("ROLLBACK", ()).await;
                return Err(erro(e));
            }
        }
        c.execute("COMMIT", ()).await.map_err(erro)?;
        Ok(())
    }
}
