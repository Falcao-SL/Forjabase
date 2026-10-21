//! Motores de banco. Cada um só sabe executar SQL; a lógica do Forja
//! (coleções, registros, regras, validação) fica em `store::Banco`, escrita
//! uma vez para todos.
//!
//! O SQL chega sempre com `?` como marcador. O executor do Postgres troca
//! por `$1`, `$2`… antes de enviar.

use crate::erro::Resultado;
use crate::filtro::Dialeto;
use crate::valor::Valor;
use async_trait::async_trait;

#[cfg(feature = "postgres")]
pub mod postgres;
#[cfg(feature = "sqlite")]
pub mod sqlite;
#[cfg(feature = "turso")]
pub mod turso;

/// Uma linha de resultado, na ordem das colunas do `SELECT`.
pub type Linha = Vec<Valor>;

#[async_trait]
pub trait Executor: Send + Sync {
    /// Qual SQL este motor fala.
    fn dialeto(&self) -> Dialeto;

    /// Nome do motor, para logs e para `forja`.
    fn nome(&self) -> &'static str;

    /// Executa um comando e devolve as linhas afetadas.
    async fn executar(&self, sql: &str, params: Vec<Valor>) -> Resultado<u64>;

    /// Executa uma consulta e devolve todas as linhas.
    async fn consultar(&self, sql: &str, params: Vec<Valor>) -> Resultado<Vec<Linha>>;

    /// Executa vários comandos numa transação: tudo ou nada.
    async fn lote(&self, comandos: Vec<(String, Vec<Valor>)>) -> Resultado<()>;
}

/// `?` → `$1`, `$2`… O SQL do Forja nunca tem `?` dentro de texto entre
/// aspas (valores sempre vão como parâmetro), então a troca é segura.
pub fn marcadores_postgres(sql: &str) -> String {
    let mut out = String::with_capacity(sql.len() + 8);
    let mut n = 0;
    let mut em_texto = false;
    for c in sql.chars() {
        match c {
            '\'' => {
                em_texto = !em_texto;
                out.push(c);
            }
            '?' if !em_texto => {
                n += 1;
                out.push('$');
                out.push_str(&n.to_string());
            }
            _ => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    #[test]
    fn marcadores() {
        assert_eq!(
            super::marcadores_postgres("a = ? AND b LIKE ? ESCAPE '\\' AND c = '?'"),
            "a = $1 AND b LIKE $2 ESCAPE '\\' AND c = '?'"
        );
    }
}
