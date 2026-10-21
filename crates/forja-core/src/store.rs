//! Armazenamento. `Banco` tem toda a lógica (coleções, registros, regras,
//! validação, superusuários) escrita uma vez; o motor por baixo é um
//! `Executor`: SQLite (padrão), Turso ou PostgreSQL/Supabase, por projeto.
//!
//! Cada coleção base vira uma tabela com o nome dela: `id`, uma coluna por
//! campo, `created` e `updated`. As coleções ficam descritas em `_collections`.

use crate::erro::{Erro, Resultado};
use crate::filtro::{self, Dialeto, Literal};
use crate::motor::{Executor, Linha};
use crate::valor::Valor;
use crate::{Collection, CollectionType, Field, FieldType, agora, auth, id};
use serde::Serialize;
use serde_json::{Map, Value, json};
use std::sync::Arc;

/// Pedido de listagem, com os parâmetros da API do PocketBase.
#[derive(Debug, Clone)]
pub struct Consulta {
    pub filtro: Option<String>,
    pub ordem: Option<String>,
    pub pagina: u32,
    pub por_pagina: u32,
    pub pular_total: bool,
    /// Regra de acesso da coleção, somada ao filtro com AND.
    pub regra: Option<String>,
}

impl Default for Consulta {
    fn default() -> Self {
        Consulta {
            filtro: None,
            ordem: None,
            pagina: 1,
            por_pagina: 30,
            pular_total: false,
            regra: None,
        }
    }
}

/// Resposta de listagem no formato do PocketBase.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Lista {
    pub page: u32,
    pub per_page: u32,
    pub total_items: i64,
    pub total_pages: i64,
    pub items: Vec<Value>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Superusuario {
    pub collection_id: String,
    pub collection_name: String,
    pub id: String,
    pub email: String,
    pub created: String,
    pub updated: String,
    #[serde(skip)]
    pub senha: String,
    #[serde(skip)]
    pub token_key: String,
}

/// O banco de um projeto.
#[derive(Clone)]
pub struct Banco {
    ex: Arc<dyn Executor>,
}

// ── Abrir ────────────────────────────────────────────────────────────────

/// Qual motor um projeto usa. A API e o comportamento são os mesmos nos três.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Motor {
    /// rusqlite, um arquivo por projeto. O padrão.
    Sqlite,
    /// Turso: o mesmo arquivo, com escrita concorrente.
    Turso,
    /// PostgreSQL ou Supabase; cada projeto num esquema `forja_<projeto>`.
    Postgres { url: String },
}

impl Banco {
    /// Abre o projeto `projeto` (dados em `pasta/<projeto>` nos motores de arquivo).
    pub async fn abrir(motor: &Motor, pasta: &std::path::Path, projeto: &str) -> Resultado<Banco> {
        if !nome_valido(projeto) {
            return Err(Erro::invalido("Invalid project name."));
        }
        #[allow(unreachable_patterns)]
        match motor {
            #[cfg(feature = "sqlite")]
            Motor::Sqlite => Banco::sqlite(&pasta.join(projeto)).await,
            #[cfg(feature = "turso")]
            Motor::Turso => Banco::turso(&pasta.join(projeto)).await,
            #[cfg(feature = "postgres")]
            Motor::Postgres { url } => Banco::postgres(url, &format!("forja_{projeto}")).await,
            // Sem o motor no `{:?}`: a URL do Postgres traz a senha.
            _ => Err(Erro::invalido(
                "This engine is not compiled into this build.",
            )),
        }
    }
    /// Sobre um executor já aberto; cria as tabelas de sistema se faltarem.
    pub async fn novo(ex: Arc<dyn Executor>) -> Resultado<Banco> {
        let b = Banco { ex };
        b.ex.lote(
            esquema(b.dialeto())
                .into_iter()
                .map(|s| (s, vec![]))
                .collect(),
        )
        .await?;
        Ok(b)
    }

    /// SQLite em `pasta/data.db`.
    #[cfg(feature = "sqlite")]
    pub async fn sqlite(pasta: &std::path::Path) -> Resultado<Banco> {
        criar_pasta(pasta)?;
        Banco::novo(Arc::new(crate::motor::sqlite::Sqlite::abrir(
            &pasta.join("data.db"),
        )?))
        .await
    }

    #[cfg(feature = "sqlite")]
    pub async fn sqlite_memoria() -> Resultado<Banco> {
        Banco::novo(Arc::new(crate::motor::sqlite::Sqlite::em_memoria()?)).await
    }

    /// Turso em `pasta/data.db` (o mesmo formato de arquivo do SQLite).
    #[cfg(feature = "turso")]
    pub async fn turso(pasta: &std::path::Path) -> Resultado<Banco> {
        criar_pasta(pasta)?;
        let arquivo = pasta.join("data.db");
        let caminho = arquivo
            .to_str()
            .ok_or_else(|| Erro::invalido("Invalid data path."))?;
        Banco::novo(Arc::new(crate::motor::turso::Turso::abrir(caminho).await?)).await
    }

    #[cfg(feature = "turso")]
    pub async fn turso_memoria() -> Resultado<Banco> {
        Banco::novo(Arc::new(
            crate::motor::turso::Turso::abrir(":memory:").await?,
        ))
        .await
    }

    /// PostgreSQL (inclusive Supabase), com as tabelas no esquema `esquema`.
    #[cfg(feature = "postgres")]
    pub async fn postgres(url: &str, esquema: &str) -> Resultado<Banco> {
        Banco::novo(Arc::new(
            crate::motor::postgres::Postgres::conectar(url, esquema).await?,
        ))
        .await
    }

    pub fn dialeto(&self) -> Dialeto {
        self.ex.dialeto()
    }

    /// `sqlite`, `turso` ou `postgres`.
    pub fn motor(&self) -> &'static str {
        self.ex.nome()
    }
}

#[cfg(any(feature = "sqlite", feature = "turso"))]
fn criar_pasta(pasta: &std::path::Path) -> Resultado<()> {
    std::fs::create_dir_all(pasta).map_err(|e| Erro::Interno(format!("pasta de dados: {e}")))
}

fn esquema(d: Dialeto) -> Vec<String> {
    // SQLite e Turso comparam nome e e-mail sem diferenciar maiúsculas pela
    // coluna; o Postgres, por um índice em lower().
    let nocase = if d == Dialeto::Sqlite {
        " COLLATE NOCASE"
    } else {
        ""
    };
    let mut v = vec![
        "CREATE TABLE IF NOT EXISTS \"_params\" (\"id\" TEXT PRIMARY KEY NOT NULL, \"value\" TEXT NOT NULL)".to_string(),
        format!(
            "CREATE TABLE IF NOT EXISTS \"_collections\" (
                \"id\" TEXT PRIMARY KEY NOT NULL,
                \"name\" TEXT UNIQUE NOT NULL{nocase},
                \"type\" TEXT NOT NULL,
                \"system\" BOOLEAN NOT NULL DEFAULT FALSE,
                \"fields\" TEXT NOT NULL DEFAULT '[]',
                \"listRule\" TEXT,
                \"viewRule\" TEXT,
                \"createRule\" TEXT,
                \"updateRule\" TEXT,
                \"deleteRule\" TEXT,
                \"indexes\" TEXT NOT NULL DEFAULT '[]',
                \"created\" TEXT NOT NULL,
                \"updated\" TEXT NOT NULL
            )"
        ),
        format!(
            "CREATE TABLE IF NOT EXISTS \"_superusers\" (
                \"id\" TEXT PRIMARY KEY NOT NULL,
                \"email\" TEXT UNIQUE NOT NULL{nocase},
                \"password\" TEXT NOT NULL,
                \"tokenKey\" TEXT NOT NULL,
                \"created\" TEXT NOT NULL,
                \"updated\" TEXT NOT NULL
            )"
        ),
    ];
    if d == Dialeto::Postgres {
        v.push("CREATE UNIQUE INDEX IF NOT EXISTS \"_collections_name_ci\" ON \"_collections\" (lower(\"name\"))".into());
        v.push("CREATE UNIQUE INDEX IF NOT EXISTS \"_superusers_email_ci\" ON \"_superusers\" (lower(\"email\"))".into());
    }
    v
}

// ── Validação e conversão ────────────────────────────────────────────────

fn nome_valido(n: &str) -> bool {
    let mut cs = n.chars();
    matches!(cs.next(), Some(c) if c.is_ascii_alphabetic() || c == '_')
        && cs.all(|c| c.is_ascii_alphanumeric() || c == '_')
        && n.len() <= 100
}

const RESERVADOS: [&str; 6] = [
    "id",
    "created",
    "updated",
    "collectionId",
    "collectionName",
    "expand",
];

fn tipo_sql(f: &Field, d: Dialeto) -> &'static str {
    match (f.kind, d) {
        (FieldType::Number, Dialeto::Sqlite) => "NUMERIC NOT NULL DEFAULT 0",
        (FieldType::Number, Dialeto::Postgres) => "DOUBLE PRECISION NOT NULL DEFAULT 0",
        (FieldType::Bool, _) => "BOOLEAN NOT NULL DEFAULT FALSE",
        (FieldType::Json, _) => "TEXT DEFAULT NULL",
        (FieldType::Select, _) if f.multiplo() => "TEXT NOT NULL DEFAULT '[]'",
        _ => "TEXT NOT NULL DEFAULT ''",
    }
}

fn literal_valor(l: Literal) -> Valor {
    match l {
        Literal::Texto(t) => Valor::Texto(t),
        Literal::Numero(n) => Valor::Real(n),
        Literal::Bool(b) => Valor::Bool(b),
        Literal::Nulo => Valor::Nulo,
    }
}

fn erro_campo(erros: &mut Map<String, Value>, nome: &str, codigo: &str, msg: String) {
    erros.insert(nome.into(), json!({ "code": codigo, "message": msg }));
}

fn invalido(mensagem: &str, erros: Map<String, Value>) -> Erro {
    Erro::Invalido {
        mensagem: mensagem.into(),
        campos: erros,
    }
}

/// Valida e converte o valor de um campo. `None` = campo ausente (usa o vazio).
fn valor_do_campo(f: &Field, v: Option<&Value>, erros: &mut Map<String, Value>) -> Valor {
    let n = f.name.as_str();
    let obrigatorio = |erros: &mut Map<String, Value>| {
        erro_campo(erros, n, "validation_required", "Cannot be blank.".into())
    };
    let ruim = |erros: &mut Map<String, Value>| {
        erro_campo(
            erros,
            n,
            "validation_invalid_value",
            "Invalid value.".into(),
        )
    };
    match f.kind {
        FieldType::Text
        | FieldType::Editor
        | FieldType::Email
        | FieldType::Url
        | FieldType::Date => {
            let t = match v {
                None | Some(Value::Null) => String::new(),
                Some(Value::String(s)) => s.clone(),
                Some(Value::Number(x)) => x.to_string(),
                Some(Value::Bool(b)) => b.to_string(),
                _ => {
                    ruim(erros);
                    return Valor::Texto(String::new());
                }
            };
            if t.is_empty() {
                if f.required {
                    obrigatorio(erros);
                }
                return Valor::Texto(t);
            }
            let tam = t.chars().count() as f64;
            match f.kind {
                FieldType::Text | FieldType::Editor => {
                    if let Some(min) = f.opcao_f64("min").filter(|m| *m > 0.0)
                        && tam < min
                    {
                        erro_campo(
                            erros,
                            n,
                            "validation_min_text_constraint",
                            format!("Must be at least {min} character(s)."),
                        );
                    }
                    let max = f.opcao_f64("max").filter(|m| *m > 0.0).unwrap_or(5000.0);
                    if f.kind == FieldType::Text && tam > max {
                        erro_campo(
                            erros,
                            n,
                            "validation_max_text_constraint",
                            format!("Must be less than {max} character(s)."),
                        );
                    }
                }
                FieldType::Email => {
                    let ok = t.split_once('@').is_some_and(|(a, d)| {
                        !a.is_empty() && d.contains('.') && !d.starts_with('.') && !d.ends_with('.')
                    }) && !t.contains(char::is_whitespace)
                        && t.matches('@').count() == 1;
                    if !ok {
                        erro_campo(
                            erros,
                            n,
                            "validation_is_email",
                            "Must be a valid email address.".into(),
                        );
                    }
                }
                FieldType::Url => {
                    let resto = t
                        .strip_prefix("https://")
                        .or_else(|| t.strip_prefix("http://"));
                    if !resto.is_some_and(|r| !r.is_empty() && !r.contains(char::is_whitespace)) {
                        erro_campo(erros, n, "validation_is_url", "Must be a valid url.".into());
                    }
                }
                FieldType::Date => {
                    let b = t.as_bytes();
                    let ok = b.len() >= 10
                        && b[..4].iter().all(u8::is_ascii_digit)
                        && b[4] == b'-'
                        && b[5..7].iter().all(u8::is_ascii_digit)
                        && b[7] == b'-'
                        && b[8..10].iter().all(u8::is_ascii_digit);
                    if !ok {
                        erro_campo(
                            erros,
                            n,
                            "validation_invalid_date",
                            "Must be a valid date.".into(),
                        );
                    }
                }
                _ => {}
            }
            Valor::Texto(t)
        }
        FieldType::Number => {
            let x = match v {
                None | Some(Value::Null) => 0.0,
                Some(Value::Number(x)) => x.as_f64().unwrap_or(0.0),
                Some(Value::String(s)) if s.trim().is_empty() => 0.0,
                Some(Value::String(s)) => match s.trim().parse::<f64>() {
                    Ok(x) => x,
                    Err(_) => {
                        ruim(erros);
                        return Valor::Real(0.0);
                    }
                },
                _ => {
                    ruim(erros);
                    return Valor::Real(0.0);
                }
            };
            if x == 0.0 && f.required {
                obrigatorio(erros);
            }
            if let Some(min) = f.opcao_f64("min")
                && x < min
            {
                erro_campo(
                    erros,
                    n,
                    "validation_min_number_constraint",
                    format!("Must be larger than {min}."),
                );
            }
            if let Some(max) = f.opcao_f64("max")
                && x > max
            {
                erro_campo(
                    erros,
                    n,
                    "validation_max_number_constraint",
                    format!("Must be less than {max}."),
                );
            }
            if f.opcao_bool("onlyInt") && x.fract() != 0.0 {
                erro_campo(
                    erros,
                    n,
                    "validation_only_int_constraint",
                    "Decimal numbers are not allowed.".into(),
                );
            }
            Valor::Real(x)
        }
        FieldType::Bool => {
            let b = match v {
                None | Some(Value::Null) => false,
                Some(Value::Bool(b)) => *b,
                Some(Value::Number(x)) => x.as_f64() != Some(0.0),
                Some(Value::String(s)) => matches!(s.as_str(), "true" | "1"),
                _ => {
                    ruim(erros);
                    false
                }
            };
            if !b && f.required {
                obrigatorio(erros);
            }
            Valor::Bool(b)
        }
        FieldType::Select => {
            let permitidos: Vec<String> = f
                .options
                .get("values")
                .and_then(Value::as_array)
                .map(|a| {
                    a.iter()
                        .filter_map(|x| x.as_str().map(String::from))
                        .collect()
                })
                .unwrap_or_default();
            let escolhidos: Vec<String> = match v {
                None | Some(Value::Null) => vec![],
                Some(Value::String(s)) if s.is_empty() => vec![],
                Some(Value::String(s)) => vec![s.clone()],
                Some(Value::Array(a)) => {
                    let mut out: Vec<String> = vec![];
                    for x in a {
                        match x.as_str() {
                            Some(s) if !out.iter().any(|o| o == s) => out.push(s.into()),
                            Some(_) => {}
                            None => ruim(erros),
                        }
                    }
                    out
                }
                _ => {
                    ruim(erros);
                    vec![]
                }
            };
            if escolhidos.iter().any(|s| !permitidos.contains(s)) {
                ruim(erros);
            }
            let max = f.opcao_f64("maxSelect").unwrap_or(1.0).max(1.0) as usize;
            if escolhidos.len() > max {
                erro_campo(
                    erros,
                    n,
                    "validation_too_many_values",
                    format!("Select no more than {max}."),
                );
            }
            if escolhidos.is_empty() && f.required {
                obrigatorio(erros);
            }
            if f.multiplo() {
                Valor::Texto(serde_json::to_string(&escolhidos).unwrap_or_else(|_| "[]".into()))
            } else {
                Valor::Texto(escolhidos.into_iter().next().unwrap_or_default())
            }
        }
        FieldType::Json => {
            let valor = v.cloned().unwrap_or(Value::Null);
            let vazio = match &valor {
                Value::Null => true,
                Value::String(s) => s.is_empty(),
                Value::Array(a) => a.is_empty(),
                Value::Object(o) => o.is_empty(),
                _ => false,
            };
            if vazio && f.required {
                obrigatorio(erros);
            }
            if valor.is_null() {
                Valor::Nulo
            } else {
                Valor::Texto(valor.to_string())
            }
        }
        // Tipos ainda não suportados não chegam aqui: a coleção é recusada antes.
        _ => Valor::Nulo,
    }
}

fn numero_json(x: f64) -> Value {
    if x.fract() == 0.0 && x.abs() < 9.0e15 {
        json!(x as i64)
    } else {
        json!(x)
    }
}

/// Valor do banco para o JSON da resposta, igual em todos os motores.
fn para_json(f: &Field, v: Valor) -> Value {
    match (f.kind, v) {
        (_, Valor::Nulo) if f.kind == FieldType::Select && f.multiplo() => json!([]),
        (FieldType::Json, Valor::Nulo) => Value::Null,
        (FieldType::Number, Valor::Nulo) => json!(0),
        (FieldType::Bool, Valor::Nulo) => json!(false),
        (_, Valor::Nulo) => json!(""),
        (FieldType::Number, Valor::Int(i)) => json!(i),
        (FieldType::Number, Valor::Real(x)) => numero_json(x),
        (FieldType::Number, Valor::Texto(t)) => {
            t.parse::<f64>().map(numero_json).unwrap_or(json!(0))
        }
        (FieldType::Bool, v) => json!(v.bool()),
        (FieldType::Json, Valor::Texto(t)) => serde_json::from_str(&t).unwrap_or(Value::Null),
        (FieldType::Select, Valor::Texto(t)) if f.multiplo() => {
            serde_json::from_str(&t).unwrap_or(json!([]))
        }
        (_, Valor::Texto(t)) => json!(t),
        (_, Valor::Int(i)) => json!(i),
        (_, Valor::Real(x)) => numero_json(x),
        (_, Valor::Bool(b)) => json!(b),
    }
}

fn colunas(c: &Collection) -> String {
    let mut cols = vec!["\"id\"".to_string()];
    cols.extend(c.fields.iter().map(|f| format!("\"{}\"", f.name)));
    cols.push("\"created\"".into());
    cols.push("\"updated\"".into());
    cols.join(", ")
}

fn linha_para_registro(c: &Collection, l: Linha) -> Value {
    let mut it = l.into_iter();
    let mut m = Map::new();
    m.insert("collectionId".into(), json!(c.id));
    m.insert("collectionName".into(), json!(c.name));
    m.insert(
        "id".into(),
        json!(it.next().unwrap_or(Valor::Nulo).texto_ou_vazio()),
    );
    for f in &c.fields {
        m.insert(
            f.name.clone(),
            para_json(f, it.next().unwrap_or(Valor::Nulo)),
        );
    }
    m.insert(
        "created".into(),
        json!(it.next().unwrap_or(Valor::Nulo).texto_ou_vazio()),
    );
    m.insert(
        "updated".into(),
        json!(it.next().unwrap_or(Valor::Nulo).texto_ou_vazio()),
    );
    Value::Object(m)
}

const COLS_COLECAO: &str = "\"id\", \"name\", \"type\", \"system\", \"fields\", \"listRule\", \"viewRule\", \"createRule\", \"updateRule\", \"deleteRule\", \"indexes\", \"created\", \"updated\"";

fn ler_colecao(l: Linha) -> Collection {
    let mut it = l.into_iter();
    let mut prox = || it.next().unwrap_or(Valor::Nulo);
    let id = prox().texto_ou_vazio();
    let name = prox().texto_ou_vazio();
    let tipo = prox().texto_ou_vazio();
    let system = prox().bool();
    let campos = prox().texto_ou_vazio();
    let regra = |v: Valor| match v {
        Valor::Texto(t) => Some(t),
        _ => None,
    };
    let (list_rule, view_rule, create_rule, update_rule, delete_rule) = (
        regra(prox()),
        regra(prox()),
        regra(prox()),
        regra(prox()),
        regra(prox()),
    );
    let indices = prox().texto_ou_vazio();
    Collection {
        id,
        name,
        kind: serde_json::from_value(json!(tipo)).unwrap_or_default(),
        system,
        fields: serde_json::from_str(&campos).unwrap_or_default(),
        list_rule,
        view_rule,
        create_rule,
        update_rule,
        delete_rule,
        indexes: serde_json::from_str(&indices).unwrap_or_default(),
        created: prox().texto_ou_vazio(),
        updated: prox().texto_ou_vazio(),
    }
}

const COLS_SUPERUSER: &str =
    "\"id\", \"email\", \"password\", \"tokenKey\", \"created\", \"updated\"";

fn ler_superuser(l: Linha) -> Superusuario {
    let t: Vec<String> = l.into_iter().map(|v| v.texto_ou_vazio()).collect();
    let g = |i: usize| t.get(i).cloned().unwrap_or_default();
    Superusuario {
        collection_id: auth::COLECAO_SUPERUSERS.into(),
        collection_name: "_superusers".into(),
        id: g(0),
        email: g(1),
        senha: g(2),
        token_key: g(3),
        created: g(4),
        updated: g(5),
    }
}

fn validar_colecao(c: &Collection, d: Dialeto) -> Resultado<()> {
    let mut erros = Map::new();
    if !nome_valido(&c.name)
        || c.name.starts_with('_')
        || c.name.to_lowercase().starts_with("sqlite_")
    {
        erro_campo(
            &mut erros,
            "name",
            "validation_invalid_name",
            "Invalid collection name.".into(),
        );
    }
    if c.kind != CollectionType::Base {
        erro_campo(
            &mut erros,
            "type",
            "validation_not_supported",
            "Only 'base' collections are supported in this version.".into(),
        );
    }
    let mut vistos: Vec<String> = vec![];
    for (i, f) in c.fields.iter().enumerate() {
        let chave = format!("fields.{i}.name");
        if !nome_valido(&f.name) || RESERVADOS.contains(&f.name.as_str()) {
            erro_campo(
                &mut erros,
                &chave,
                "validation_invalid_name",
                "Invalid field name.".into(),
            );
        }
        if vistos.iter().any(|v| v.eq_ignore_ascii_case(&f.name)) {
            erro_campo(
                &mut erros,
                &chave,
                "validation_duplicated_field_name",
                "Duplicated field name.".into(),
            );
        }
        vistos.push(f.name.clone());
        if !f.kind.suportado() {
            let tipo = serde_json::to_value(f.kind)
                .ok()
                .and_then(|v| v.as_str().map(String::from))
                .unwrap_or_default();
            erro_campo(
                &mut erros,
                &format!("fields.{i}.type"),
                "validation_not_supported",
                format!("Field type '{tipo}' is not supported in this version."),
            );
        }
    }
    if erros.is_empty() {
        // As regras são filtros: precisam ser válidas para esta coleção.
        for (nome, regra) in [
            ("listRule", &c.list_rule),
            ("viewRule", &c.view_rule),
            ("createRule", &c.create_rule),
            ("updateRule", &c.update_rule),
            ("deleteRule", &c.delete_rule),
        ] {
            if let Some(r) = regra.as_deref().filter(|r| !r.trim().is_empty())
                && let Err(e) = filtro::para_sql(r, &|n| c.coluna_valida(n), d)
            {
                erro_campo(&mut erros, nome, "validation_invalid_rule", e.to_string());
            }
        }
    }
    if erros.is_empty() {
        Ok(())
    } else {
        Err(invalido("Failed to create collection.", erros))
    }
}

fn nao_encontrado() -> Erro {
    Erro::NaoEncontrado("The requested resource wasn't found.".into())
}

// ── Operações ────────────────────────────────────────────────────────────

impl Banco {
    /// Regra de acesso + filtro do usuário em um `WHERE`, com os parâmetros.
    fn onde(
        &self,
        c: &Collection,
        regra: Option<&str>,
        filtro_usuario: Option<&str>,
    ) -> Resultado<(String, Vec<Valor>)> {
        let valido = |n: &str| c.coluna_valida(n);
        let mut partes = vec![];
        let mut params = vec![];
        for f in [regra, filtro_usuario]
            .into_iter()
            .flatten()
            .filter(|f| !f.trim().is_empty())
        {
            let sql = filtro::para_sql(f, &valido, self.dialeto())?;
            partes.push(sql.texto);
            params.extend(sql.params.into_iter().map(literal_valor));
        }
        let texto = if partes.is_empty() {
            String::new()
        } else {
            format!(" WHERE {}", partes.join(" AND "))
        };
        Ok((texto, params))
    }

    async fn contar(&self, sql: &str, params: Vec<Valor>) -> Resultado<i64> {
        Ok(self
            .ex
            .consultar(sql, params)
            .await?
            .first()
            .and_then(|l| l.first())
            .map(Valor::int)
            .unwrap_or(0))
    }

    /// Segredo dos tokens deste projeto (gerado na primeira vez).
    pub async fn segredo(&self) -> Resultado<String> {
        let l = self
            .ex
            .consultar(
                "SELECT \"value\" FROM \"_params\" WHERE \"id\" = 'jwtSecret'",
                vec![],
            )
            .await?;
        if let Some(v) = l.first().and_then(|l| l.first()).and_then(|v| v.texto()) {
            return Ok(v.to_string());
        }
        let s = id::aleatorio(50);
        self.ex
            .executar(
                "INSERT INTO \"_params\" (\"id\", \"value\") VALUES ('jwtSecret', ?)",
                vec![s.clone().into()],
            )
            .await?;
        Ok(s)
    }

    pub async fn colecoes(&self) -> Resultado<Vec<Collection>> {
        let l = self
            .ex
            .consultar(
                &format!(
                    "SELECT {COLS_COLECAO} FROM \"_collections\" ORDER BY \"created\", \"id\""
                ),
                vec![],
            )
            .await?;
        Ok(l.into_iter().map(ler_colecao).collect())
    }

    pub async fn colecao(&self, id_ou_nome: &str) -> Resultado<Collection> {
        let l = self
            .ex
            .consultar(
                &format!("SELECT {COLS_COLECAO} FROM \"_collections\" WHERE \"id\" = ? OR lower(\"name\") = lower(?)"),
                vec![id_ou_nome.into(), id_ou_nome.into()],
            )
            .await?;
        l.into_iter()
            .next()
            .map(ler_colecao)
            .ok_or_else(|| Erro::NaoEncontrado("Missing collection context.".into()))
    }

    pub async fn criar_colecao(&self, mut c: Collection) -> Resultado<Collection> {
        let d = self.dialeto();
        validar_colecao(&c, d)?;
        let agora = agora();
        if c.id.is_empty() {
            c.id = format!("pbc_{}", id::novo());
        }
        c.system = false;
        c.created = agora.clone();
        c.updated = agora;
        for f in &mut c.fields {
            if f.id.is_empty() {
                let tipo = serde_json::to_value(f.kind)
                    .ok()
                    .and_then(|v| v.as_str().map(String::from))
                    .unwrap_or_default();
                f.id = format!("{tipo}{}", id::aleatorio(10));
            }
        }
        let existe = self
            .contar(
                "SELECT COUNT(*) FROM \"_collections\" WHERE lower(\"name\") = lower(?) OR \"id\" = ?",
                vec![c.name.clone().into(), c.id.clone().into()],
            )
            .await?;
        let duplicado = || {
            let mut erros = Map::new();
            erro_campo(
                &mut erros,
                "name",
                "validation_collection_name_exists",
                "Collection name must be unique (case insensitive).".into(),
            );
            invalido("Failed to create collection.", erros)
        };
        if existe > 0 {
            return Err(duplicado());
        }
        let mut colunas_sql = vec!["\"id\" TEXT PRIMARY KEY NOT NULL".to_string()];
        colunas_sql.extend(
            c.fields
                .iter()
                .map(|f| format!("\"{}\" {}", f.name, tipo_sql(f, d))),
        );
        colunas_sql.push("\"created\" TEXT NOT NULL DEFAULT ''".into());
        colunas_sql.push("\"updated\" TEXT NOT NULL DEFAULT ''".into());
        let insert = (
            format!(
                "INSERT INTO \"_collections\" ({COLS_COLECAO}) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)"
            ),
            vec![
                c.id.clone().into(),
                c.name.clone().into(),
                "base".into(),
                Valor::Bool(c.system),
                serde_json::to_string(&c.fields)
                    .unwrap_or_else(|_| "[]".into())
                    .into(),
                c.list_rule.clone().into(),
                c.view_rule.clone().into(),
                c.create_rule.clone().into(),
                c.update_rule.clone().into(),
                c.delete_rule.clone().into(),
                serde_json::to_string(&c.indexes)
                    .unwrap_or_else(|_| "[]".into())
                    .into(),
                c.created.clone().into(),
                c.updated.clone().into(),
            ],
        );
        self.ex
            .lote(vec![
                (
                    format!("CREATE TABLE \"{}\" ({})", c.name, colunas_sql.join(", ")),
                    vec![],
                ),
                insert,
            ])
            .await
            .map_err(|e| {
                if matches!(e, Erro::Duplicado(_)) {
                    duplicado()
                } else {
                    e
                }
            })?;
        Ok(c)
    }

    pub async fn apagar_colecao(&self, id_ou_nome: &str) -> Resultado<()> {
        let c = self.colecao(id_ou_nome).await?;
        if c.system {
            return Err(Erro::invalido("System collections can't be deleted."));
        }
        self.ex
            .lote(vec![
                (format!("DROP TABLE IF EXISTS \"{}\"", c.name), vec![]),
                (
                    "DELETE FROM \"_collections\" WHERE \"id\" = ?".into(),
                    vec![c.id.clone().into()],
                ),
            ])
            .await
    }

    pub async fn listar(&self, c: &Collection, q: &Consulta) -> Resultado<Lista> {
        let por_pagina = q.por_pagina.clamp(1, 1000);
        let pagina = q.pagina.max(1);
        let (onde_sql, params) = self.onde(c, q.regra.as_deref(), q.filtro.as_deref())?;
        // "id" desempata valores iguais: a mesma consulta devolve sempre a mesma
        // ordem, e a paginação não repete nem pula registros.
        let ordem = match q.ordem.as_deref().filter(|s| !s.trim().is_empty()) {
            Some(s) => format!(
                " ORDER BY {}, \"id\"",
                filtro::ordem(s, &|n| c.coluna_valida(n), self.dialeto())?
            ),
            None => " ORDER BY \"created\", \"id\"".into(),
        };
        let total = if q.pular_total {
            -1
        } else {
            self.contar(
                &format!("SELECT COUNT(*) FROM \"{}\"{onde_sql}", c.name),
                params.clone(),
            )
            .await?
        };
        let sql = format!(
            "SELECT {} FROM \"{}\"{onde_sql}{ordem} LIMIT {por_pagina} OFFSET {}",
            colunas(c),
            c.name,
            (pagina as i64 - 1) * por_pagina as i64
        );
        let items = self
            .ex
            .consultar(&sql, params)
            .await?
            .into_iter()
            .map(|l| linha_para_registro(c, l))
            .collect();
        let paginas = if total < 0 {
            -1
        } else {
            (total + por_pagina as i64 - 1) / por_pagina as i64
        };
        Ok(Lista {
            page: pagina,
            per_page: por_pagina,
            total_items: total,
            total_pages: paginas,
            items,
        })
    }

    pub async fn registro(
        &self,
        c: &Collection,
        id: &str,
        regra: Option<&str>,
    ) -> Resultado<Value> {
        let (onde_sql, mut params) = self.onde(c, regra, None)?;
        let filtro_id = if onde_sql.is_empty() {
            " WHERE \"id\" = ?".to_string()
        } else {
            format!("{onde_sql} AND \"id\" = ?")
        };
        params.push(id.into());
        let l = self
            .ex
            .consultar(
                &format!("SELECT {} FROM \"{}\"{filtro_id}", colunas(c), c.name),
                params,
            )
            .await?;
        l.into_iter()
            .next()
            .map(|l| linha_para_registro(c, l))
            .ok_or_else(nao_encontrado)
    }

    pub async fn criar(&self, c: &Collection, dados: &Map<String, Value>) -> Resultado<Value> {
        let mut erros = Map::new();
        let novo_id = match dados
            .get("id")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
        {
            Some(s) if id::valido(s) => s.to_string(),
            Some(_) => {
                erro_campo(
                    &mut erros,
                    "id",
                    "validation_invalid_id",
                    "Must be 15 characters [a-z0-9].".into(),
                );
                String::new()
            }
            None => id::novo(),
        };
        let valores: Vec<Valor> = c
            .fields
            .iter()
            .map(|f| valor_do_campo(f, dados.get(&f.name), &mut erros))
            .collect();
        if !erros.is_empty() {
            return Err(invalido("Failed to create record.", erros));
        }
        let agora = agora();
        let mut todos = vec![Valor::Texto(novo_id.clone())];
        todos.extend(valores);
        todos.push(agora.clone().into());
        todos.push(agora.into());
        let marcadores = vec!["?"; todos.len()].join(", ");
        let r = self
            .ex
            .executar(
                &format!(
                    "INSERT INTO \"{}\" ({}) VALUES ({marcadores})",
                    c.name,
                    colunas(c)
                ),
                todos,
            )
            .await;
        if let Err(Erro::Duplicado(_)) = r {
            let mut erros = Map::new();
            erro_campo(
                &mut erros,
                "id",
                "validation_invalid_id",
                "The id is invalid or already exists.".into(),
            );
            return Err(invalido("Failed to create record.", erros));
        }
        r?;
        self.registro(c, &novo_id, None).await
    }

    pub async fn atualizar(
        &self,
        c: &Collection,
        id: &str,
        dados: &Map<String, Value>,
        regra: Option<&str>,
    ) -> Resultado<Value> {
        let atual = self.registro(c, id, regra).await?;
        let mut erros = Map::new();
        let mut sets = vec![];
        let mut valores = vec![];
        for f in &c.fields {
            // Só os campos enviados mudam; os outros são revalidados como estão
            // para manter "obrigatório" coerente.
            let v = dados.get(&f.name).or_else(|| atual.get(&f.name));
            let valor = valor_do_campo(f, v, &mut erros);
            if dados.contains_key(&f.name) {
                sets.push(format!("\"{}\" = ?", f.name));
                valores.push(valor);
            }
        }
        if !erros.is_empty() {
            return Err(invalido("Failed to update record.", erros));
        }
        sets.push("\"updated\" = ?".into());
        valores.push(agora().into());
        valores.push(id.into());
        self.ex
            .executar(
                &format!(
                    "UPDATE \"{}\" SET {} WHERE \"id\" = ?",
                    c.name,
                    sets.join(", ")
                ),
                valores,
            )
            .await?;
        self.registro(c, id, None).await
    }

    pub async fn apagar(&self, c: &Collection, id: &str, regra: Option<&str>) -> Resultado<()> {
        self.registro(c, id, regra).await?;
        self.ex
            .executar(
                &format!("DELETE FROM \"{}\" WHERE \"id\" = ?", c.name),
                vec![id.into()],
            )
            .await?;
        Ok(())
    }

    pub async fn superuser_salvar(&self, email: &str, senha: &str) -> Resultado<Superusuario> {
        let mut erros = Map::new();
        let email = email.trim().to_string();
        let ok_email = email
            .split_once('@')
            .is_some_and(|(a, d)| !a.is_empty() && d.contains('.'));
        if !ok_email {
            erro_campo(
                &mut erros,
                "email",
                "validation_is_email",
                "Must be a valid email address.".into(),
            );
        }
        if senha.chars().count() < 10 {
            erro_campo(
                &mut erros,
                "password",
                "validation_min_text_constraint",
                "Must be at least 10 character(s).".into(),
            );
        }
        if !erros.is_empty() {
            return Err(invalido("Failed to save superuser.", erros));
        }
        // O Argon2 é caro de propósito: fora das threads do runtime.
        let senha = senha.to_string();
        let hash = tokio::task::spawn_blocking(move || auth::hash_senha(&senha))
            .await
            .map_err(|e| Erro::Interno(e.to_string()))??;
        let chave = id::aleatorio(50);
        let agora = agora();
        let existente = self
            .ex
            .consultar(
                "SELECT \"id\" FROM \"_superusers\" WHERE lower(\"email\") = lower(?)",
                vec![email.clone().into()],
            )
            .await?
            .into_iter()
            .next()
            .map(|l| l.into_iter().next().unwrap_or(Valor::Nulo).texto_ou_vazio());
        let id_su = match existente {
            Some(i) => {
                self.ex
                    .executar(
                        "UPDATE \"_superusers\" SET \"password\" = ?, \"tokenKey\" = ?, \"updated\" = ? WHERE \"id\" = ?",
                        vec![hash.into(), chave.into(), agora.into(), i.clone().into()],
                    )
                    .await?;
                i
            }
            None => {
                let i = id::novo();
                self.ex
                    .executar(
                        &format!("INSERT INTO \"_superusers\" ({COLS_SUPERUSER}) VALUES (?, ?, ?, ?, ?, ?)"),
                        vec![i.clone().into(), email.into(), hash.into(), chave.into(), agora.clone().into(), agora.into()],
                    )
                    .await?;
                i
            }
        };
        self.superuser(&id_su).await
    }

    pub async fn superuser_por_email(&self, email: &str) -> Resultado<Superusuario> {
        self.ex
            .consultar(
                &format!(
                    "SELECT {COLS_SUPERUSER} FROM \"_superusers\" WHERE lower(\"email\") = lower(?)"
                ),
                vec![email.trim().into()],
            )
            .await?
            .into_iter()
            .next()
            .map(ler_superuser)
            .ok_or_else(|| Erro::NaoEncontrado("Superuser not found.".into()))
    }

    pub async fn superuser(&self, id: &str) -> Resultado<Superusuario> {
        self.ex
            .consultar(
                &format!("SELECT {COLS_SUPERUSER} FROM \"_superusers\" WHERE \"id\" = ?"),
                vec![id.into()],
            )
            .await?
            .into_iter()
            .next()
            .map(ler_superuser)
            .ok_or_else(|| Erro::NaoEncontrado("Superuser not found.".into()))
    }

    pub async fn total_superusers(&self) -> Resultado<i64> {
        self.contar("SELECT COUNT(*) FROM \"_superusers\"", vec![])
            .await
    }
}

// ── Conformidade: os mesmos testes em todos os motores ───────────────────

/// Bateria que todo motor precisa passar. Usada pelos testes de cada motor
/// (e disponível para quem escrever um motor novo).
pub mod conformidade {
    use super::*;

    fn produtos() -> Collection {
        serde_json::from_value(json!({
            "name": "produtos",
            "fields": [
                {"name": "nome", "type": "text", "required": true, "max": 50},
                {"name": "preco", "type": "number", "min": 0},
                {"name": "ativo", "type": "bool"},
                {"name": "site", "type": "url"},
                {"name": "tags", "type": "select", "values": ["novo", "promo"], "maxSelect": 2},
                {"name": "extra", "type": "json"}
            ],
            "listRule": "",
            "viewRule": "ativo = true"
        }))
        .unwrap()
    }

    fn obj(v: Value) -> Map<String, Value> {
        v.as_object().unwrap().clone()
    }

    pub async fn tudo(b: &Banco) {
        colecoes(b).await;
        registros(b).await;
        validacao(b).await;
        superusuarios(b).await;
    }

    pub async fn colecoes(b: &Banco) {
        let c = b.criar_colecao(produtos()).await.unwrap();
        assert!(c.id.starts_with("pbc_") && c.fields.iter().all(|f| !f.id.is_empty()));
        assert_eq!(b.colecao("PRODUTOS").await.unwrap().id, c.id);
        assert_eq!(b.colecao(&c.id).await.unwrap().fields.len(), 6);
        assert_eq!(b.colecoes().await.unwrap().len(), 1);
        assert!(b.criar_colecao(produtos()).await.is_err(), "nome repetido");
        let mut ruim = produtos();
        ruim.name = "x; DROP TABLE y".into();
        assert!(b.criar_colecao(ruim).await.is_err());
        let ruim: Collection =
            serde_json::from_value(json!({"name":"a","fields":[{"name":"id","type":"text"}]}))
                .unwrap();
        assert!(b.criar_colecao(ruim).await.is_err());
        let ruim: Collection =
            serde_json::from_value(json!({"name":"b","fields":[{"name":"f","type":"file"}]}))
                .unwrap();
        assert!(b.criar_colecao(ruim).await.is_err());
        let ruim: Collection =
            serde_json::from_value(json!({"name":"c","listRule":"senha = 1"})).unwrap();
        assert!(b.criar_colecao(ruim).await.is_err());
        b.apagar_colecao("produtos").await.unwrap();
        assert!(b.colecao("produtos").await.is_err());
        assert!(b.colecoes().await.unwrap().is_empty());
    }

    pub async fn registros(b: &Banco) {
        let c = b.criar_colecao(produtos()).await.unwrap();
        for (nome, preco, ativo) in [
            ("Café", 32.0, true),
            ("Chá", 18.5, true),
            ("Bolo", 45.0, false),
        ] {
            b.criar(
                &c,
                &obj(json!({"nome": nome, "preco": preco, "ativo": ativo, "tags": ["novo"]})),
            )
            .await
            .unwrap();
        }
        let r = b
            .criar(
                &c,
                &obj(json!({"nome": "Pão", "preco": "7", "extra": {"a": 1}})),
            )
            .await
            .unwrap();
        assert_eq!(r["preco"], json!(7));
        assert_eq!(r["extra"], json!({"a": 1}));
        assert_eq!(r["tags"], json!([]));
        assert_eq!(r["ativo"], json!(false));
        assert_eq!(r["site"], json!(""));
        assert_eq!(r["collectionName"], json!("produtos"));
        assert!(id::valido(r["id"].as_str().unwrap()));

        let q = Consulta {
            filtro: Some("preco < 40 && ativo = true".into()),
            ordem: Some("-preco".into()),
            ..Default::default()
        };
        let l = b.listar(&c, &q).await.unwrap();
        assert_eq!(l.total_items, 2);
        assert_eq!(l.items[0]["nome"], json!("Café"));
        assert_eq!(l.items[1]["preco"], json!(18.5));
        assert_eq!(l.items[0]["tags"], json!(["novo"]));

        let contem = b
            .listar(
                &c,
                &Consulta {
                    filtro: Some("nome ~ 'caf'".into()),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        assert_eq!(contem.total_items, 1, "~ sem diferenciar maiúsculas");
        let vazio = b
            .listar(
                &c,
                &Consulta {
                    filtro: Some("site = null".into()),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        assert_eq!(vazio.total_items, 4);

        let p = b
            .listar(
                &c,
                &Consulta {
                    por_pagina: 3,
                    pagina: 2,
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        assert_eq!((p.total_items, p.total_pages, p.items.len()), (4, 2, 1));
        let s = b
            .listar(
                &c,
                &Consulta {
                    pular_total: true,
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        assert_eq!((s.total_items, s.total_pages), (-1, -1));

        let pid = r["id"].as_str().unwrap();
        let u = b
            .atualizar(&c, pid, &obj(json!({"preco": 9})), None)
            .await
            .unwrap();
        assert_eq!(
            (u["preco"].clone(), u["nome"].clone()),
            (json!(9), json!("Pão"))
        );
        assert!(
            b.registro(&c, pid, Some("ativo = true")).await.is_err(),
            "regra esconde"
        );
        assert!(b.registro(&c, pid, None).await.is_ok());
        b.apagar(&c, pid, None).await.unwrap();
        assert!(b.registro(&c, pid, None).await.is_err());
        // id escolhido e repetido
        let fixo = "abcdefghijklmno";
        b.criar(&c, &obj(json!({"id": fixo, "nome": "fixo"})))
            .await
            .unwrap();
        assert!(
            b.criar(&c, &obj(json!({"id": fixo, "nome": "de novo"})))
                .await
                .is_err()
        );
        b.apagar_colecao("produtos").await.unwrap();
    }

    pub async fn validacao(b: &Banco) {
        let c = b.criar_colecao(produtos()).await.unwrap();
        let e = b
            .criar(&c, &obj(json!({"preco": -1, "site": "ftp://x", "tags": ["outro", "novo", "promo"], "id": "CURTO"})))
            .await
            .unwrap_err();
        let Erro::Invalido { campos, .. } = e else {
            panic!("{e:?}")
        };
        assert_eq!(campos["nome"]["code"], json!("validation_required"));
        assert_eq!(
            campos["preco"]["code"],
            json!("validation_min_number_constraint")
        );
        assert_eq!(campos["site"]["code"], json!("validation_is_url"));
        assert!(campos.contains_key("tags"));
        assert_eq!(campos["id"]["code"], json!("validation_invalid_id"));
        let r = b.criar(&c, &obj(json!({"nome": "ok"}))).await.unwrap();
        assert!(
            b.atualizar(
                &c,
                r["id"].as_str().unwrap(),
                &obj(json!({"nome": ""})),
                None
            )
            .await
            .is_err()
        );
        b.apagar_colecao("produtos").await.unwrap();
    }

    pub async fn superusuarios(b: &Banco) {
        assert_eq!(b.total_superusers().await.unwrap(), 0);
        assert!(b.superuser_salvar("ana@x.com", "curta").await.is_err());
        let s = b
            .superuser_salvar("ana@x.com", "senha-forte-123")
            .await
            .unwrap();
        assert!(auth::conferir_senha(&s.senha, "senha-forte-123"));
        let s2 = b
            .superuser_salvar("ANA@x.com", "outra-senha-456")
            .await
            .unwrap();
        assert_eq!(s2.id, s.id);
        assert_ne!(s2.token_key, s.token_key);
        assert_eq!(b.total_superusers().await.unwrap(), 1);
        assert_eq!(b.superuser_por_email("Ana@X.com").await.unwrap().id, s.id);
        let seg = b.segredo().await.unwrap();
        assert_eq!(seg.len(), 50);
        assert_eq!(b.segredo().await.unwrap(), seg);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(feature = "sqlite")]
    #[tokio::test]
    async fn conformidade_sqlite() {
        let b = Banco::sqlite_memoria().await.unwrap();
        assert_eq!(b.motor(), "sqlite");
        conformidade::tudo(&b).await;
    }

    #[cfg(feature = "turso")]
    #[tokio::test]
    async fn conformidade_turso() {
        let b = Banco::turso_memoria().await.unwrap();
        assert_eq!(b.motor(), "turso");
        conformidade::tudo(&b).await;
    }

    /// Roda só com `FORJA_TEST_PG=postgres://…` (no CI, um Postgres de serviço).
    #[cfg(feature = "postgres")]
    #[tokio::test]
    async fn conformidade_postgres() {
        let Ok(url) = std::env::var("FORJA_TEST_PG") else {
            eprintln!("FORJA_TEST_PG ausente: conformidade do Postgres pulada");
            return;
        };
        let esquema = format!("teste_{}", id::novo());
        let b = Banco::postgres(&url, &esquema).await.unwrap();
        assert_eq!(b.motor(), "postgres");
        conformidade::tudo(&b).await;
    }

    #[cfg(feature = "sqlite")]
    #[tokio::test]
    async fn dados_ficam_no_disco_sqlite() {
        let d = tempfile::tempdir().unwrap();
        {
            let b = Banco::sqlite(d.path()).await.unwrap();
            let c = b
                .criar_colecao(
                    serde_json::from_value(
                        json!({"name":"notas","fields":[{"name":"t","type":"text"}]}),
                    )
                    .unwrap(),
                )
                .await
                .unwrap();
            b.criar(&c, json!({"t": "persistente"}).as_object().unwrap())
                .await
                .unwrap();
        }
        let b = Banco::sqlite(d.path()).await.unwrap();
        let c = b.colecao("notas").await.unwrap();
        assert_eq!(
            b.listar(&c, &Consulta::default()).await.unwrap().items[0]["t"],
            json!("persistente")
        );
    }

    /// O Turso abre o arquivo que o SQLite gravou: trocar de motor não perde dados.
    #[cfg(all(feature = "sqlite", feature = "turso"))]
    #[tokio::test]
    async fn turso_le_o_arquivo_do_sqlite() {
        let d = tempfile::tempdir().unwrap();
        {
            let b = Banco::sqlite(d.path()).await.unwrap();
            let c = b
                .criar_colecao(
                    serde_json::from_value(
                        json!({"name":"notas","fields":[{"name":"t","type":"text"}]}),
                    )
                    .unwrap(),
                )
                .await
                .unwrap();
            b.criar(&c, json!({"t": "veio do sqlite"}).as_object().unwrap())
                .await
                .unwrap();
        }
        let b = Banco::turso(d.path()).await.unwrap();
        let c = b.colecao("notas").await.unwrap();
        assert_eq!(
            b.listar(&c, &Consulta::default()).await.unwrap().items[0]["t"],
            json!("veio do sqlite")
        );
    }
}
