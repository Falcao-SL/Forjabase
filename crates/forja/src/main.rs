//! `forja`: o executável único (servidor + painel embutido, quando pronto).

use clap::{Parser, Subcommand, ValueEnum};
use forja_core::store::{Banco, Motor};
use std::net::SocketAddr;
use std::path::PathBuf;

#[derive(Parser)]
#[command(
    name = "forja",
    version,
    about = "Backend em um arquivo: SQLite, Turso ou Postgres, autenticação, arquivos e tempo real. Em Rust."
)]
struct Cli {
    /// Pasta de dados (cada projeto fica numa subpasta; hoje só `default`).
    #[arg(long, global = true, default_value = "forja_data")]
    dir: PathBuf,
    /// Motor do banco. Os três dão a mesma API.
    #[arg(long, global = true, value_enum, default_value_t = QualMotor::Sqlite, env = "FORJA_ENGINE")]
    engine: QualMotor,
    /// URL do Postgres/Supabase (`postgres://usuario:senha@host:5432/banco`).
    /// Prefira a variável de ambiente: assim a senha não aparece no `ps`.
    #[arg(long, global = true, env = "FORJA_PG_URL", hide_env_values = true)]
    pg_url: Option<String>,
    #[command(subcommand)]
    comando: Comando,
}

#[derive(Clone, Copy, ValueEnum)]
enum QualMotor {
    Sqlite,
    Turso,
    Postgres,
}

impl Cli {
    fn motor(&self) -> Result<Motor, String> {
        Ok(match self.engine {
            QualMotor::Sqlite => Motor::Sqlite,
            QualMotor::Turso => Motor::Turso,
            QualMotor::Postgres => Motor::Postgres {
                url: self
                    .pg_url
                    .clone()
                    .ok_or("o motor postgres precisa de --pg-url ou FORJA_PG_URL")?,
            },
        })
    }
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
    let r = match cli.motor() {
        Err(e) => Err(e),
        Ok(motor) => executar(cli.comando, cli.dir, motor).await,
    };
    match r {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("erro: {e}");
            std::process::ExitCode::FAILURE
        }
    }
}

async fn executar(comando: Comando, dir: PathBuf, motor: Motor) -> Result<(), String> {
    match comando {
        Comando::Serve { http } => forja_server::serve(http, dir, motor)
            .await
            .map_err(|e| e.to_string()),
        Comando::Superuser {
            acao: AcaoSuperuser::Upsert { email, senha },
        } => async {
            let b = Banco::abrir(&motor, &dir, "default").await?;
            b.superuser_salvar(&email, &senha).await
        }
        .await
        .map(|s| println!("Superusuário {} salvo.", s.email))
        .map_err(|e| match e {
            forja_core::Erro::Invalido { campos, .. } => campos
                .values()
                .filter_map(|v| v["message"].as_str())
                .collect::<Vec<_>>()
                .join(" "),
            outro => outro.to_string(),
        }),
    }
}
