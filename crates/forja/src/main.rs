//! `forja`: o executável único (servidor + painel embutido, quando pronto).

use clap::{Parser, Subcommand, ValueEnum};
use forja_core::store::{Banco, Motor};
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
    /// Motor do banco. Só `sqlite` tem suporte; os outros dois são
    /// experimentais e precisam de `--features turso` / `--features postgres`.
    #[arg(long, global = true, value_enum, default_value_t = QualMotor::Sqlite, env = "FORJA_ENGINE")]
    engine: QualMotor,
    /// URL do Postgres/Supabase (experimental; exige `--features postgres`).
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
        /// Origens CORS permitidas (separadas por vírgula, padrão "*").
        #[arg(long, default_value = "*")]
        origins: String,
        /// Pasta pública para arquivos estáticos (HTML/CSS/JS).
        #[arg(long, alias = "publicDir", env = "FORJA_PUBLIC_DIR")]
        public_dir: Option<PathBuf>,
        /// Modo de desenvolvimento (logs detalhados e rastreio de erros).
        #[arg(long, default_value_t = false)]
        dev: bool,
    },
    /// Gerencia projetos (multi-tenancy).
    Projects {
        #[command(subcommand)]
        acao: AcaoProjetos,
    },
    /// Gerencia coleções.
    Collections {
        #[command(subcommand)]
        acao: AcaoColecoes,
    },
    /// Gerencia superusuários.
    Superuser {
        #[command(subcommand)]
        acao: AcaoSuperuser,
    },
}

#[derive(Subcommand)]
enum AcaoProjetos {
    /// Lista todos os projetos disponíveis.
    List,
    /// Cria um novo projeto vazio.
    Create { name: String },
    /// Apaga um projeto e seus dados.
    Delete { name: String },
    /// Importa uma pasta pb_data do PocketBase para um projeto do Forja.
    ImportPb {
        /// Caminho para a pasta pb_data que contém data.db (e opcionalmente storage).
        path: PathBuf,
        /// Nome do projeto destino no Forja.
        #[arg(long)]
        name: String,
    },
}

#[derive(Subcommand)]
enum AcaoColecoes {
    /// Lista as coleções do projeto.
    List {
        #[arg(long, default_value = "default")]
        project: String,
    },
    /// Esvazia (truncate) todos os registros e arquivos de uma coleção.
    Truncate {
        name: String,
        #[arg(long, default_value = "default")]
        project: String,
    },
}

#[derive(Subcommand)]
enum AcaoSuperuser {
    /// Cria o superusuário, ou troca a senha se o e-mail já existir.
    Upsert {
        email: String,
        senha: String,
        #[arg(long, default_value = "default")]
        project: String,
    },
    /// Remove um superusuário pelo e-mail.
    Delete {
        email: String,
        #[arg(long, default_value = "default")]
        project: String,
    },
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
        Comando::Serve {
            http,
            origins,
            public_dir,
            dev,
        } => {
            let config = forja_server::ServidorConfig {
                origins: origins
                    .split(',')
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect(),
                public_dir,
                dev,
            };
            forja_server::serve_com_config(http, dir, motor, config)
                .await
                .map_err(|e| e.to_string())
        }
        Comando::Projects { acao } => match acao {
            AcaoProjetos::List => {
                let gp = forja_server::GerenciadorProjetos::novo(
                    dir,
                    motor,
                    std::sync::Arc::new(forja_core::correio::Console),
                );
                let lista = gp.listar().await;
                println!("Projetos encontrados:");
                for p in lista {
                    println!("  - {p}");
                }
                Ok(())
            }
            AcaoProjetos::Create { name } => {
                let gp = forja_server::GerenciadorProjetos::novo(
                    dir,
                    motor,
                    std::sync::Arc::new(forja_core::correio::Console),
                );
                gp.criar(&name).await.map_err(|e| e.to_string())?;
                println!("Projeto '{name}' criado com sucesso.");
                Ok(())
            }
            AcaoProjetos::Delete { name } => {
                let gp = forja_server::GerenciadorProjetos::novo(
                    dir,
                    motor,
                    std::sync::Arc::new(forja_core::correio::Console),
                );
                gp.apagar(&name).await.map_err(|e| e.to_string())?;
                println!("Projeto '{name}' apagado.");
                Ok(())
            }
            AcaoProjetos::ImportPb { path, name } => {
                let gp = forja_server::GerenciadorProjetos::novo(
                    dir,
                    motor,
                    std::sync::Arc::new(forja_core::correio::Console),
                );
                gp.importar_pb(&path, &name)
                    .await
                    .map_err(|e| e.to_string())?;
                println!(
                    "Projeto '{name}' importado com sucesso de {}.",
                    path.display()
                );
                Ok(())
            }
        },
        Comando::Collections { acao } => match acao {
            AcaoColecoes::List { project } => {
                let b = Banco::abrir(&motor, &dir, &project)
                    .await
                    .map_err(|e| e.to_string())?;
                let cols = b.colecoes().await.map_err(|e| e.to_string())?;
                println!("Coleções no projeto '{project}':");
                for c in cols {
                    let tipo = match c.kind {
                        forja_core::CollectionType::Base => "base",
                        forja_core::CollectionType::Auth => "auth",
                        forja_core::CollectionType::View => "view",
                    };
                    println!("  - {} (id: {}, tipo: {})", c.name, c.id, tipo);
                }
                Ok(())
            }
            AcaoColecoes::Truncate { name, project } => {
                let b = Banco::abrir(&motor, &dir, &project)
                    .await
                    .map_err(|e| e.to_string())?;
                let c = b.colecao(&name).await.map_err(|e| e.to_string())?;
                b.colecao_esvaziar(&c).await.map_err(|e| e.to_string())?;
                println!("Coleção '{name}' no projeto '{project}' esvaziada com sucesso.");
                Ok(())
            }
        },
        Comando::Superuser { acao } => match acao {
            AcaoSuperuser::Upsert {
                email,
                senha,
                project,
            } => async {
                let b = Banco::abrir(&motor, &dir, &project).await?;
                b.superuser_salvar(&email, &senha).await
            }
            .await
            .map(|s| println!("Superusuário {} salvo no projeto '{project}'.", s.email))
            .map_err(formatar_erro),
            AcaoSuperuser::Delete { email, project } => async {
                let b = Banco::abrir(&motor, &dir, &project).await?;
                b.superuser_apagar(&email).await
            }
            .await
            .map(|s| println!("Superusuário {} apagado do projeto '{project}'.", s.email))
            .map_err(formatar_erro),
        },
    }
}

fn formatar_erro(e: forja_core::Erro) -> String {
    match e {
        forja_core::Erro::Invalido { campos, .. } => campos
            .values()
            .filter_map(|v| v["message"].as_str())
            .collect::<Vec<_>>()
            .join(" "),
        outro => outro.to_string(),
    }
}
