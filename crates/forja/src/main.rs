//! `forja`: o executável único (servidor + painel embutido, quando pronto).

use clap::{Parser, Subcommand};
use std::net::SocketAddr;

#[derive(Parser)]
#[command(
    name = "forja",
    version,
    about = "Backend em um arquivo: SQLite, autenticação, arquivos e tempo real. Em Rust."
)]
struct Cli {
    #[command(subcommand)]
    comando: Comando,
}

#[derive(Subcommand)]
enum Comando {
    /// Sobe o servidor HTTP.
    Serve {
        /// Endereço e porta (a porta padrão é a mesma do PocketBase).
        #[arg(long, default_value = "127.0.0.1:8090")]
        http: SocketAddr,
    },
}

#[tokio::main]
async fn main() -> std::io::Result<()> {
    match Cli::parse().comando {
        Comando::Serve { http } => forja_server::serve(http).await,
    }
}
