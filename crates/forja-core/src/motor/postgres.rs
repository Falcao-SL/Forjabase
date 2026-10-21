//! PostgreSQL: qualquer Postgres, inclusive um projeto do Supabase
//! (https://github.com/supabase/supabase). Cada projeto do Forja fica num
//! esquema (`schema`) próprio, então vários projetos dividem um banco.

use super::{Executor, Linha, marcadores_postgres};
use crate::erro::{Erro, Resultado};
use crate::filtro::Dialeto;
use crate::valor::Valor;
use async_trait::async_trait;
use bytes::BytesMut;
use deadpool_postgres::{Config, Pool, Runtime};
use tokio_postgres::types::{IsNull, ToSql, Type, to_sql_checked};
use tokio_postgres_rustls::MakeRustlsConnect;

pub struct Postgres {
    pool: Pool,
    esquema: String,
}

impl Postgres {
    /// `url`: `postgres://usuario:senha@host:5432/banco`. `esquema`: onde
    /// ficam as tabelas deste projeto (criado se não existir).
    pub async fn conectar(url: &str, esquema: &str) -> Resultado<Postgres> {
        if !esquema
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_')
            || esquema.is_empty()
        {
            return Err(Erro::invalido("Invalid Postgres schema name."));
        }
        let mut cfg = Config::new();
        cfg.url = Some(url.to_string());
        let pool = cfg
            .create_pool(Some(Runtime::Tokio1), tls())
            .map_err(|e| Erro::Interno(format!("postgres: {e}")))?;
        let p = Postgres {
            pool,
            esquema: esquema.to_string(),
        };
        let c = p.cliente().await?;
        c.batch_execute(&format!("CREATE SCHEMA IF NOT EXISTS \"{esquema}\""))
            .await
            .map_err(erro)?;
        Ok(p)
    }

    async fn cliente(&self) -> Resultado<deadpool_postgres::Object> {
        let c = self
            .pool
            .get()
            .await
            .map_err(|e| Erro::Interno(format!("postgres: {e}")))?;
        c.batch_execute(&format!("SET search_path TO \"{}\"", self.esquema))
            .await
            .map_err(erro)?;
        Ok(c)
    }
}

/// TLS com as raízes da Mozilla. O Supabase e os Postgres gerenciados exigem
/// TLS; com o `sslmode=prefer` padrão, um Postgres local sem TLS também funciona.
/// `?sslmode=require` na URL recusa conexão sem TLS.
fn tls() -> MakeRustlsConnect {
    let raizes = rustls::RootCertStore::from_iter(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    let cfg = rustls::ClientConfig::builder_with_provider(
        rustls::crypto::ring::default_provider().into(),
    )
    .with_safe_default_protocol_versions()
    .expect("protocolos padrão do rustls")
    .with_root_certificates(raizes)
    .with_no_client_auth();
    MakeRustlsConnect::new(cfg)
}

fn erro(e: tokio_postgres::Error) -> Erro {
    match e.code() {
        Some(c) if *c == tokio_postgres::error::SqlState::UNIQUE_VIOLATION => {
            Erro::Duplicado(e.to_string())
        }
        _ => Erro::Interno(
            e.as_db_error()
                .map(|d| d.to_string())
                .unwrap_or_else(|| e.to_string()),
        ),
    }
}

/// O Postgres confere o tipo de cada parâmetro; `Valor` se adapta à coluna.
#[derive(Debug)]
struct Param(Valor);

impl ToSql for Param {
    fn to_sql(
        &self,
        ty: &Type,
        out: &mut BytesMut,
    ) -> Result<IsNull, Box<dyn std::error::Error + Sync + Send>> {
        match &self.0 {
            Valor::Nulo => Ok(IsNull::Yes),
            Valor::Bool(b) => match *ty {
                Type::BOOL => b.to_sql(ty, out),
                Type::INT2 => (*b as i16).to_sql(ty, out),
                Type::INT4 => (*b as i32).to_sql(ty, out),
                Type::INT8 => (*b as i64).to_sql(ty, out),
                Type::FLOAT8 => (*b as i64 as f64).to_sql(ty, out),
                _ => b.to_string().to_sql(ty, out),
            },
            Valor::Int(i) => match *ty {
                Type::INT2 => (*i as i16).to_sql(ty, out),
                Type::INT4 => (*i as i32).to_sql(ty, out),
                Type::INT8 => i.to_sql(ty, out),
                Type::FLOAT4 => (*i as f32).to_sql(ty, out),
                Type::FLOAT8 => (*i as f64).to_sql(ty, out),
                Type::BOOL => (*i != 0).to_sql(ty, out),
                _ => i.to_string().to_sql(ty, out),
            },
            Valor::Real(x) => match *ty {
                Type::FLOAT4 => (*x as f32).to_sql(ty, out),
                Type::FLOAT8 => x.to_sql(ty, out),
                Type::INT2 => (*x as i16).to_sql(ty, out),
                Type::INT4 => (*x as i32).to_sql(ty, out),
                Type::INT8 => (*x as i64).to_sql(ty, out),
                _ => x.to_string().to_sql(ty, out),
            },
            Valor::Texto(t) => match *ty {
                Type::FLOAT8 => t.parse::<f64>()?.to_sql(ty, out),
                Type::INT8 => t.parse::<i64>()?.to_sql(ty, out),
                Type::BOOL => matches!(t.as_str(), "true" | "1").to_sql(ty, out),
                _ => t.as_str().to_sql(ty, out),
            },
        }
    }

    fn accepts(_: &Type) -> bool {
        true
    }

    to_sql_checked!();
}

fn ler(r: &tokio_postgres::Row, i: usize) -> Valor {
    let t = r.columns()[i].type_().clone();
    let v = match t {
        Type::BOOL => r
            .try_get::<_, Option<bool>>(i)
            .ok()
            .flatten()
            .map(Valor::Bool),
        Type::INT2 => r
            .try_get::<_, Option<i16>>(i)
            .ok()
            .flatten()
            .map(|x| Valor::Int(x as i64)),
        Type::INT4 => r
            .try_get::<_, Option<i32>>(i)
            .ok()
            .flatten()
            .map(|x| Valor::Int(x as i64)),
        Type::INT8 => r
            .try_get::<_, Option<i64>>(i)
            .ok()
            .flatten()
            .map(Valor::Int),
        Type::FLOAT4 => r
            .try_get::<_, Option<f32>>(i)
            .ok()
            .flatten()
            .map(|x| Valor::Real(x as f64)),
        Type::FLOAT8 => r
            .try_get::<_, Option<f64>>(i)
            .ok()
            .flatten()
            .map(Valor::Real),
        _ => r
            .try_get::<_, Option<String>>(i)
            .ok()
            .flatten()
            .map(Valor::Texto),
    };
    v.unwrap_or(Valor::Nulo)
}

fn refs(p: &[Param]) -> Vec<&(dyn ToSql + Sync)> {
    p.iter().map(|x| x as &(dyn ToSql + Sync)).collect()
}

#[async_trait]
impl Executor for Postgres {
    fn dialeto(&self) -> Dialeto {
        Dialeto::Postgres
    }

    fn nome(&self) -> &'static str {
        "postgres"
    }

    async fn executar(&self, sql: &str, params: Vec<Valor>) -> Resultado<u64> {
        let c = self.cliente().await?;
        let p: Vec<Param> = params.into_iter().map(Param).collect();
        c.execute(&marcadores_postgres(sql), &refs(&p))
            .await
            .map_err(erro)
    }

    async fn consultar(&self, sql: &str, params: Vec<Valor>) -> Resultado<Vec<Linha>> {
        let c = self.cliente().await?;
        let p: Vec<Param> = params.into_iter().map(Param).collect();
        let linhas = c
            .query(&marcadores_postgres(sql), &refs(&p))
            .await
            .map_err(erro)?;
        Ok(linhas
            .iter()
            .map(|r| (0..r.len()).map(|i| ler(r, i)).collect())
            .collect())
    }

    async fn lote(&self, comandos: Vec<(String, Vec<Valor>)>) -> Resultado<()> {
        let mut c = self.cliente().await?;
        let tx = c.transaction().await.map_err(erro)?;
        for (sql, params) in comandos {
            if params.is_empty() {
                tx.batch_execute(&sql).await.map_err(erro)?;
            } else {
                let p: Vec<Param> = params.into_iter().map(Param).collect();
                tx.execute(&marcadores_postgres(&sql), &refs(&p))
                    .await
                    .map_err(erro)?;
            }
        }
        tx.commit().await.map_err(erro)
    }
}
