//! `forja`: o executável único (servidor + painel embutido, quando pronto).

use clap::{Parser, Subcommand, ValueEnum};
use forja_core::store::{Banco, Consulta, Motor};
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
    /// Gerencia registros das coleções (CRUD).
    Records {
        #[command(subcommand)]
        acao: AcaoRegistros,
    },
    /// Gerencia backups compactados (.zip).
    Backups {
        #[command(subcommand)]
        acao: AcaoBackups,
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
    /// Exibe os detalhes e esquema de uma coleção.
    Show {
        name: String,
        #[arg(long, default_value = "default")]
        project: String,
        #[arg(long, default_value_t = false)]
        json: bool,
    },
    /// Exporta o esquema das coleções em JSON.
    Export {
        #[arg(long, default_value = "default")]
        project: String,
    },
    /// Importa esquemas de coleções a partir de um arquivo JSON.
    Import {
        /// Caminho para o arquivo JSON com a lista de coleções.
        file: PathBuf,
        /// Remove coleções locais ausentes no arquivo de importação.
        #[arg(long, default_value_t = false)]
        delete_missing: bool,
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
enum AcaoRegistros {
    /// Lista registros de uma coleção.
    List {
        collection: String,
        #[arg(long)]
        filter: Option<String>,
        #[arg(long)]
        sort: Option<String>,
        #[arg(long, default_value_t = 1)]
        page: usize,
        #[arg(long, default_value_t = 30)]
        per_page: usize,
        #[arg(long, default_value = "default")]
        project: String,
        #[arg(long, default_value_t = false)]
        json: bool,
    },
    /// Obtém um registro específico pelo ID.
    Get {
        collection: String,
        id: String,
        #[arg(long, default_value = "default")]
        project: String,
        #[arg(long, default_value_t = false)]
        json: bool,
    },
    /// Cria um novo registro a partir de dados JSON.
    Create {
        collection: String,
        /// JSON dos campos (ex: '{"titulo":"Artigo 1"}').
        data: String,
        #[arg(long, default_value = "default")]
        project: String,
    },
    /// Atualiza um registro existente a partir de dados JSON.
    Update {
        collection: String,
        id: String,
        /// JSON dos campos para atualizar (ex: '{"status":"publicado"}').
        data: String,
        #[arg(long, default_value = "default")]
        project: String,
    },
    /// Remove um registro pelo ID.
    Delete {
        collection: String,
        id: String,
        #[arg(long, default_value = "default")]
        project: String,
    },
}

#[derive(Subcommand)]
enum AcaoBackups {
    /// Lista todos os backups disponíveis no projeto.
    List {
        #[arg(long, default_value = "default")]
        project: String,
        #[arg(long, default_value_t = false)]
        json: bool,
    },
    /// Cria um novo arquivo de backup zip.
    Create {
        #[arg(long)]
        name: Option<String>,
        #[arg(long, default_value = "default")]
        project: String,
    },
    /// Restaura um arquivo de backup zip.
    Restore {
        key: String,
        #[arg(long, default_value = "default")]
        project: String,
    },
    /// Apaga um arquivo de backup zip.
    Delete {
        key: String,
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
            AcaoColecoes::Show {
                name,
                project,
                json,
            } => {
                let b = Banco::abrir(&motor, &dir, &project)
                    .await
                    .map_err(|e| e.to_string())?;
                let c = b.colecao(&name).await.map_err(|e| e.to_string())?;
                if json {
                    println!("{}", serde_json::to_string_pretty(&c).unwrap());
                } else {
                    println!("Coleção: {}", c.name);
                    println!("ID: {}", c.id);
                    let tipo = match c.kind {
                        forja_core::CollectionType::Base => "base",
                        forja_core::CollectionType::Auth => "auth",
                        forja_core::CollectionType::View => "view",
                    };
                    println!("Tipo: {tipo}");
                    println!("Campos ({}):", c.fields.len());
                    for f in &c.fields {
                        println!("  - {} ({:?})", f.name, f.kind);
                    }
                    if !c.indexes.is_empty() {
                        println!("Índices ({}):", c.indexes.len());
                        for idx in &c.indexes {
                            println!("  - {idx}");
                        }
                    }
                }
                Ok(())
            }
            AcaoColecoes::Export { project } => {
                let b = Banco::abrir(&motor, &dir, &project)
                    .await
                    .map_err(|e| e.to_string())?;
                let cols = b.colecoes().await.map_err(|e| e.to_string())?;
                println!("{}", serde_json::to_string_pretty(&cols).unwrap());
                Ok(())
            }
            AcaoColecoes::Import {
                file,
                delete_missing,
                project,
            } => {
                let conteudo = std::fs::read_to_string(&file)
                    .map_err(|e| format!("Falha ao ler {}: {e}", file.display()))?;
                let colecoes: Vec<forja_core::Collection> = serde_json::from_str(&conteudo)
                    .map_err(|e| format!("JSON de coleções inválido: {e}"))?;
                let b = Banco::abrir(&motor, &dir, &project)
                    .await
                    .map_err(|e| e.to_string())?;
                b.colecoes_importar(&colecoes, delete_missing)
                    .await
                    .map_err(|e| e.to_string())?;
                println!("Coleções importadas com sucesso no projeto '{project}'.");
                Ok(())
            }
        },
        Comando::Records { acao } => match acao {
            AcaoRegistros::List {
                collection,
                filter,
                sort,
                page,
                per_page,
                project,
                json,
            } => {
                let b = Banco::abrir(&motor, &dir, &project)
                    .await
                    .map_err(|e| e.to_string())?;
                let c = b.colecao(&collection).await.map_err(|e| e.to_string())?;
                let mut consulta = Consulta {
                    pagina: page as u32,
                    por_pagina: per_page as u32,
                    ..Default::default()
                };
                if let Some(f) = filter {
                    consulta.filtro = Some(f);
                }
                if let Some(s) = sort {
                    consulta.ordem = Some(s);
                }
                let lista = b.listar(&c, &consulta).await.map_err(|e| e.to_string())?;
                if json {
                    println!("{}", serde_json::to_string_pretty(&lista).unwrap());
                } else {
                    println!(
                        "Registros em '{}' (página {}/{}, total: {}):",
                        collection, lista.page, lista.total_pages, lista.total_items
                    );
                    for item in lista.items {
                        println!(
                            "  - ID: {}",
                            item.get("id").and_then(|v| v.as_str()).unwrap_or("-")
                        );
                        for (k, v) in item.as_object().unwrap_or(&serde_json::Map::new()) {
                            if k != "id" && k != "created" && k != "updated" {
                                println!("      {k}: {v}");
                            }
                        }
                    }
                }
                Ok(())
            }
            AcaoRegistros::Get {
                collection,
                id,
                project,
                json,
            } => {
                let b = Banco::abrir(&motor, &dir, &project)
                    .await
                    .map_err(|e| e.to_string())?;
                let c = b.colecao(&collection).await.map_err(|e| e.to_string())?;
                let r = b.registro(&c, &id, None).await.map_err(|e| e.to_string())?;
                if json {
                    println!("{}", serde_json::to_string_pretty(&r).unwrap());
                } else {
                    println!("Registro {id} em '{collection}':");
                    for (k, v) in r.as_object().unwrap_or(&serde_json::Map::new()) {
                        println!("  {k}: {v}");
                    }
                }
                Ok(())
            }
            AcaoRegistros::Create {
                collection,
                data,
                project,
            } => {
                let b = Banco::abrir(&motor, &dir, &project)
                    .await
                    .map_err(|e| e.to_string())?;
                let c = b.colecao(&collection).await.map_err(|e| e.to_string())?;
                let valor: serde_json::Value =
                    serde_json::from_str(&data).map_err(|e| format!("JSON inválido: {e}"))?;
                let obj = valor
                    .as_object()
                    .ok_or_else(|| "Dados devem ser um objeto JSON.".to_string())?;
                let rec = b.criar(&c, obj).await.map_err(formatar_erro)?;
                let id = rec.get("id").and_then(|v| v.as_str()).unwrap_or("?");
                println!("Registro '{id}' criado com sucesso em '{collection}'.");
                Ok(())
            }
            AcaoRegistros::Update {
                collection,
                id,
                data,
                project,
            } => {
                let b = Banco::abrir(&motor, &dir, &project)
                    .await
                    .map_err(|e| e.to_string())?;
                let c = b.colecao(&collection).await.map_err(|e| e.to_string())?;
                let valor: serde_json::Value =
                    serde_json::from_str(&data).map_err(|e| format!("JSON inválido: {e}"))?;
                let obj = valor
                    .as_object()
                    .ok_or_else(|| "Dados devem ser um objeto JSON.".to_string())?;
                b.atualizar(&c, &id, obj, None)
                    .await
                    .map_err(formatar_erro)?;
                println!("Registro '{id}' atualizado com sucesso em '{collection}'.");
                Ok(())
            }
            AcaoRegistros::Delete {
                collection,
                id,
                project,
            } => {
                let b = Banco::abrir(&motor, &dir, &project)
                    .await
                    .map_err(|e| e.to_string())?;
                let c = b.colecao(&collection).await.map_err(|e| e.to_string())?;
                b.apagar(&c, &id, None).await.map_err(|e| e.to_string())?;
                println!("Registro '{id}' apagado de '{collection}'.");
                Ok(())
            }
        },
        Comando::Backups { acao } => match acao {
            AcaoBackups::List { project, json } => {
                let b = Banco::abrir(&motor, &dir, &project)
                    .await
                    .map_err(|e| e.to_string())?;
                let lista = b.backups_listar().await.map_err(|e| e.to_string())?;
                if json {
                    println!("{}", serde_json::to_string_pretty(&lista).unwrap());
                } else {
                    println!("Backups no projeto '{project}' (total: {}):", lista.len());
                    for item in lista {
                        println!(
                            "  - {} ({} bytes, modificado: {})",
                            item.key, item.size, item.modified
                        );
                    }
                }
                Ok(())
            }
            AcaoBackups::Create { name, project } => {
                let b = Banco::abrir(&motor, &dir, &project)
                    .await
                    .map_err(|e| e.to_string())?;
                let info = b
                    .backup_criar(name.as_deref())
                    .await
                    .map_err(|e| e.to_string())?;
                println!(
                    "Backup '{}' criado com sucesso ({} bytes).",
                    info.key, info.size
                );
                Ok(())
            }
            AcaoBackups::Restore { key, project } => {
                let b = Banco::abrir(&motor, &dir, &project)
                    .await
                    .map_err(|e| e.to_string())?;
                b.backup_restaurar(&key).await.map_err(|e| e.to_string())?;
                println!("Backup '{key}' restaurado com sucesso no projeto '{project}'.");
                Ok(())
            }
            AcaoBackups::Delete { key, project } => {
                let b = Banco::abrir(&motor, &dir, &project)
                    .await
                    .map_err(|e| e.to_string())?;
                b.backup_apagar(&key).await.map_err(|e| e.to_string())?;
                println!("Backup '{key}' apagado com sucesso.");
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

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn cli_completa_projetos_colecoes_registros_backups_e_superusers() {
        let dir = tempfile::tempdir().unwrap();
        let pasta = dir.path().to_path_buf();
        let motor = Motor::Sqlite;

        // 1. Superuser upsert e delete
        let r_su = executar(
            Comando::Superuser {
                acao: AcaoSuperuser::Upsert {
                    email: "admin@forja.local".into(),
                    senha: "senha-forte-123".into(),
                    project: "default".into(),
                },
            },
            pasta.clone(),
            motor.clone(),
        )
        .await;
        assert!(r_su.is_ok());

        // 2. Criação e manipulação de coleção
        {
            let b = Banco::abrir(&motor, &pasta, "default").await.unwrap();
            let col = serde_json::from_value(serde_json::json!({
                "name": "notas",
                "fields": [
                    {"name": "titulo", "type": "text"}
                ]
            }))
            .unwrap();
            b.criar_colecao(col).await.unwrap();
        }

        // 3. Collections show e export
        let r_show = executar(
            Comando::Collections {
                acao: AcaoColecoes::Show {
                    name: "notas".into(),
                    project: "default".into(),
                    json: true,
                },
            },
            pasta.clone(),
            motor.clone(),
        )
        .await;
        assert!(r_show.is_ok());

        let r_exp = executar(
            Comando::Collections {
                acao: AcaoColecoes::Export {
                    project: "default".into(),
                },
            },
            pasta.clone(),
            motor.clone(),
        )
        .await;
        assert!(r_exp.is_ok());

        // 4. Records create, list, get, update, delete
        let r_create = executar(
            Comando::Records {
                acao: AcaoRegistros::Create {
                    collection: "notas".into(),
                    data: r#"{"titulo":"Aprender Rust"}"#.into(),
                    project: "default".into(),
                },
            },
            pasta.clone(),
            motor.clone(),
        )
        .await;
        assert!(r_create.is_ok());

        let b = Banco::abrir(&motor, &pasta, "default").await.unwrap();
        let c_notas = b.colecao("notas").await.unwrap();
        let recs = b.listar(&c_notas, &Consulta::default()).await.unwrap();
        assert_eq!(recs.items.len(), 1);
        let id = recs.items[0]["id"].as_str().unwrap().to_string();

        let r_get = executar(
            Comando::Records {
                acao: AcaoRegistros::Get {
                    collection: "notas".into(),
                    id: id.clone(),
                    project: "default".into(),
                    json: true,
                },
            },
            pasta.clone(),
            motor.clone(),
        )
        .await;
        assert!(r_get.is_ok());

        let r_update = executar(
            Comando::Records {
                acao: AcaoRegistros::Update {
                    collection: "notas".into(),
                    id: id.clone(),
                    data: r#"{"titulo":"Dominar Rust"}"#.into(),
                    project: "default".into(),
                },
            },
            pasta.clone(),
            motor.clone(),
        )
        .await;
        assert!(r_update.is_ok());

        // 5. Backups create e list
        let r_backup = executar(
            Comando::Backups {
                acao: AcaoBackups::Create {
                    name: Some("meu_backup.zip".into()),
                    project: "default".into(),
                },
            },
            pasta.clone(),
            motor.clone(),
        )
        .await;
        assert!(r_backup.is_ok());

        let r_backups_list = executar(
            Comando::Backups {
                acao: AcaoBackups::List {
                    project: "default".into(),
                    json: true,
                },
            },
            pasta.clone(),
            motor.clone(),
        )
        .await;
        assert!(r_backups_list.is_ok());

        // 6. Records delete e Collections truncate
        let r_del = executar(
            Comando::Records {
                acao: AcaoRegistros::Delete {
                    collection: "notas".into(),
                    id,
                    project: "default".into(),
                },
            },
            pasta.clone(),
            motor.clone(),
        )
        .await;
        assert!(r_del.is_ok());

        let r_trunc = executar(
            Comando::Collections {
                acao: AcaoColecoes::Truncate {
                    name: "notas".into(),
                    project: "default".into(),
                },
            },
            pasta.clone(),
            motor,
        )
        .await;
        assert!(r_trunc.is_ok());
    }
}
