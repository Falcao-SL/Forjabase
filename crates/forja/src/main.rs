//! `forja`: o executável único (servidor + painel embutido, quando pronto).

use clap::{Parser, Subcommand};
use forja_core::store::{Banco, Sqlite};
use std::net::SocketAddr;
use std::path::PathBuf;

#[derive(Parser)]
#[command(
    name = "forja",
    version,
    about = "Backend em um arquivo: SQLite, autenticação, arquivos e tempo real. Em Rust."
)]
struct Cli {
    /// Pasta de dados (cada projeto fica numa subpasta; hoje só `default`).
    #[arg(long, global = true, default_value = "forja_data")]
    dir: PathBuf,
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
    /// Gerencia superusuários.
    Superuser {
        #[command(subcommand)]
        acao: AcaoSuperuser,
    },
}

#[derive(Subcommand)]
enum AcaoSuperuser {
    /// Cria o superusuário, ou troca a senha se o e-mail já existir.
    Upsert { email: String, senha: String },
}

#[tokio::main]
async fn main() -> std::process::ExitCode {
    let cli = Cli::parse();
    let r = match cli.comando {
        Comando::Serve { http } => forja_server::serve(http, cli.dir)
            .await
            .map_err(|e| e.to_string()),
        Comando::Superuser {
            acao: AcaoSuperuser::Upsert { email, senha },
        } => Sqlite::abrir(&cli.dir.join("default"))
            .and_then(|b| b.superuser_salvar(&email, &senha))
            .map(|s| println!("Superusuário {} salvo.", s.email))
            .map_err(|e| match e {
                forja_core::Erro::Invalido { campos, .. } => campos
                    .values()
                    .filter_map(|v| v["message"].as_str())
                    .collect::<Vec<_>>()
                    .join(" "),
                outro => outro.to_string(),
            }),
    };
    match r {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("erro: {e}");
            std::process::ExitCode::FAILURE
        }
    }
}
