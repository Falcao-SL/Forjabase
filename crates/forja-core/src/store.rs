//! Armazenamento. `Banco` é a interface; `Sqlite` é a implementação desta
//! versão. O PostgreSQL entra depois como outra implementação, escolhida por
//! projeto (PLANO §4), sem mudar o servidor.
//!
//! Cada coleção base vira uma tabela com o nome dela: `id`, uma coluna por
//! campo, `created` e `updated`. As coleções ficam descritas em `_collections`.

use crate::erro::{Erro, Resultado};
use crate::filtro::{self, Dialeto, Literal};
use crate::{Collection, CollectionType, Field, FieldType, agora, auth, id};
use rusqlite::types::Value as Sv;
use rusqlite::{Connection, OptionalExtension, params, params_from_iter};
use serde::Serialize;
use serde_json::{Map, Value, json};
use std::path::Path;
use std::sync::Mutex;

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
        Consulta { filtro: None, ordem: None, pagina: 1, por_pagina: 30, pular_total: false, regra: None }
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

/// O que o servidor precisa de um banco.
pub trait Banco: Send + Sync {
    fn dialeto(&self) -> Dialeto;
    /// Segredo dos tokens deste projeto (gerado na primeira vez).
    fn segredo(&self) -> Resultado<String>;

    fn colecoes(&self) -> Resultado<Vec<Collection>>;
    fn colecao(&self, id_ou_nome: &str) -> Resultado<Collection>;
    fn criar_colecao(&self, c: Collection) -> Resultado<Collection>;
    fn apagar_colecao(&self, id_ou_nome: &str) -> Resultado<()>;

    fn listar(&self, c: &Collection, q: &Consulta) -> Resultado<Lista>;
    fn registro(&self, c: &Collection, id: &str, regra: Option<&str>) -> Resultado<Value>;
    fn criar(&self, c: &Collection, dados: &Map<String, Value>) -> Resultado<Value>;
    fn atualizar(&self, c: &Collection, id: &str, dados: &Map<String, Value>, regra: Option<&str>) -> Resultado<Value>;
    fn apagar(&self, c: &Collection, id: &str, regra: Option<&str>) -> Resultado<()>;

    fn superuser_salvar(&self, email: &str, senha: &str) -> Resultado<Superusuario>;
    fn superuser_por_email(&self, email: &str) -> Resultado<Superusuario>;
    fn superuser(&self, id: &str) -> Resultado<Superusuario>;
    fn total_superusers(&self) -> Resultado<i64>;
}

// ── SQLite ───────────────────────────────────────────────────────────────

pub struct Sqlite {
    conn: Mutex<Connection>,
}

const ESQUEMA: &str = "
CREATE TABLE IF NOT EXISTS _params (
    id    TEXT PRIMARY KEY NOT NULL,
    value TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS _collections (
    id         TEXT PRIMARY KEY NOT NULL,
    name       TEXT UNIQUE NOT NULL COLLATE NOCASE,
    type       TEXT NOT NULL,
    system     BOOLEAN NOT NULL DEFAULT FALSE,
    fields     JSON NOT NULL DEFAULT '[]',
    listRule   TEXT,
    viewRule   TEXT,
    createRule TEXT,
    updateRule TEXT,
    deleteRule TEXT,
    indexes    JSON NOT NULL DEFAULT '[]',
    created    TEXT NOT NULL,
    updated    TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS _superusers (
    id       TEXT PRIMARY KEY NOT NULL,
    email    TEXT UNIQUE NOT NULL COLLATE NOCASE,
    password TEXT NOT NULL,
    tokenKey TEXT NOT NULL,
    created  TEXT NOT NULL,
    updated  TEXT NOT NULL
);
";

impl Sqlite {
    /// Abre (ou cria) o banco de um projeto em `pasta/data.db`.
    pub fn abrir(pasta: &Path) -> Resultado<Sqlite> {
        std::fs::create_dir_all(pasta).map_err(|e| Erro::Interno(format!("pasta de dados: {e}")))?;
        Sqlite::iniciar(Connection::open(pasta.join("data.db"))?)
    }

    /// Banco em memória, para testes.
    pub fn em_memoria() -> Resultado<Sqlite> {
        Sqlite::iniciar(Connection::open_in_memory()?)
    }

    fn iniciar(conn: Connection) -> Resultado<Sqlite> {
        conn.execute_batch(
            "PRAGMA journal_mode = WAL; PRAGMA foreign_keys = ON; PRAGMA busy_timeout = 10000; PRAGMA synchronous = NORMAL;",
        )?;
        conn.execute_batch(ESQUEMA)?;
        Ok(Sqlite { conn: Mutex::new(conn) })
    }

    fn conn(&self) -> std::sync::MutexGuard<'_, Connection> {
        self.conn.lock().unwrap_or_else(|e| e.into_inner())
    }
}

fn nome_valido(n: &str) -> bool {
    let mut cs = n.chars();
    matches!(cs.next(), Some(c) if c.is_ascii_alphabetic() || c == '_')
        && cs.all(|c| c.is_ascii_alphanumeric() || c == '_')
        && n.len() <= 100
}

const RESERVADOS: [&str; 6] = ["id", "created", "updated", "collectionId", "collectionName", "expand"];

fn tipo_sql(f: &Field) -> &'static str {
    match f.kind {
        FieldType::Number => "NUMERIC NOT NULL DEFAULT 0",
        FieldType::Bool => "BOOLEAN NOT NULL DEFAULT FALSE",
        FieldType::Json => "JSON DEFAULT NULL",
        FieldType::Select if f.multiplo() => "JSON NOT NULL DEFAULT '[]'",
        _ => "TEXT NOT NULL DEFAULT ''",
    }
}

fn literal_sql(l: Literal) -> Sv {
    match l {
        Literal::Texto(t) => Sv::Text(t),
        Literal::Numero(n) => Sv::Real(n),
        Literal::Bool(b) => Sv::Integer(b as i64),
        Literal::Nulo => Sv::Null,
    }
}

fn erro_campo(erros: &mut Map<String, Value>, nome: &str, codigo: &str, msg: String) {
    erros.insert(nome.into(), json!({ "code": codigo, "message": msg }));
}

/// Valida e converte o valor de um campo. `None` = campo ausente (usa o vazio).
fn valor_do_campo(f: &Field, v: Option<&Value>, erros: &mut Map<String, Value>) -> Sv {
    let n = f.name.as_str();
    let obrigatorio = |erros: &mut Map<String, Value>| {
        erro_campo(erros, n, "validation_required", "Cannot be blank.".into())
    };
    let invalido = |erros: &mut Map<String, Value>| {
        erro_campo(erros, n, "validation_invalid_value", "Invalid value.".into())
    };
    match f.kind {
        FieldType::Text | FieldType::Editor | FieldType::Email | FieldType::Url | FieldType::Date => {
            let t = match v {
                None | Some(Value::Null) => String::new(),
                Some(Value::String(s)) => s.clone(),
                Some(Value::Number(x)) => x.to_string(),
                Some(Value::Bool(b)) => b.to_string(),
                _ => {
                    invalido(erros);
                    return Sv::Text(String::new());
                }
            };
            if t.is_empty() {
                if f.required {
                    obrigatorio(erros);
                }
                return Sv::Text(t);
            }
            let tam = t.chars().count() as f64;
            match f.kind {
                FieldType::Text | FieldType::Editor => {
                    if let Some(min) = f.opcao_f64("min").filter(|m| *m > 0.0)
                        && tam < min
                    {
                        erro_campo(erros, n, "validation_min_text_constraint", format!("Must be at least {min} character(s)."));
                    }
                    let max = f.opcao_f64("max").filter(|m| *m > 0.0).unwrap_or(5000.0);
                    if f.kind == FieldType::Text && tam > max {
                        erro_campo(erros, n, "validation_max_text_constraint", format!("Must be less than {max} character(s)."));
                    }
                }
                FieldType::Email => {
                    let ok = t.split_once('@').is_some_and(|(a, d)| {
                        !a.is_empty() && d.contains('.') && !d.starts_with('.') && !d.ends_with('.')
                    }) && !t.contains(char::is_whitespace)
                        && t.matches('@').count() == 1;
                    if !ok {
                        erro_campo(erros, n, "validation_is_email", "Must be a valid email address.".into());
                    }
                }
                FieldType::Url => {
                    let resto = t.strip_prefix("https://").or_else(|| t.strip_prefix("http://"));
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
                        erro_campo(erros, n, "validation_invalid_date", "Must be a valid date.".into());
                    }
                }
                _ => {}
            }
            Sv::Text(t)
        }
        FieldType::Number => {
            let x = match v {
                None | Some(Value::Null) => 0.0,
                Some(Value::Number(x)) => x.as_f64().unwrap_or(0.0),
                Some(Value::String(s)) if s.trim().is_empty() => 0.0,
                Some(Value::String(s)) => match s.trim().parse::<f64>() {
                    Ok(x) => x,
                    Err(_) => {
                        invalido(erros);
                        return Sv::Real(0.0);
                    }
                },
                _ => {
                    invalido(erros);
                    return Sv::Real(0.0);
                }
            };
            if x == 0.0 && f.required {
                obrigatorio(erros);
            }
            if let Some(min) = f.opcao_f64("min")
                && x < min
            {
                erro_campo(erros, n, "validation_min_number_constraint", format!("Must be larger than {min}."));
            }
            if let Some(max) = f.opcao_f64("max")
                && x > max
            {
                erro_campo(erros, n, "validation_max_number_constraint", format!("Must be less than {max}."));
            }
            if f.opcao_bool("onlyInt") && x.fract() != 0.0 {
                erro_campo(erros, n, "validation_only_int_constraint", "Decimal numbers are not allowed.".into());
            }
            Sv::Real(x)
        }
        FieldType::Bool => {
            let b = match v {
                None | Some(Value::Null) => false,
                Some(Value::Bool(b)) => *b,
                Some(Value::Number(x)) => x.as_f64() != Some(0.0),
                Some(Value::String(s)) => matches!(s.as_str(), "true" | "1"),
                _ => {
                    invalido(erros);
                    false
                }
            };
            if !b && f.required {
                obrigatorio(erros);
            }
            Sv::Integer(b as i64)
        }
        FieldType::Select => {
            let permitidos: Vec<String> = f
                .options
                .get("values")
                .and_then(Value::as_array)
                .map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect())
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
                            None => invalido(erros),
                        }
                    }
                    out
                }
                _ => {
                    invalido(erros);
                    vec![]
                }
            };
            if escolhidos.iter().any(|s| !permitidos.contains(s)) {
                invalido(erros);
            }
            let max = f.opcao_f64("maxSelect").unwrap_or(1.0).max(1.0) as usize;
            if escolhidos.len() > max {
                erro_campo(erros, n, "validation_too_many_values", format!("Select no more than {max}."));
            }
            if escolhidos.is_empty() && f.required {
                obrigatorio(erros);
            }
            if f.multiplo() {
                Sv::Text(serde_json::to_string(&escolhidos).unwrap_or_else(|_| "[]".into()))
            } else {
                Sv::Text(escolhidos.into_iter().next().unwrap_or_default())
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
                Sv::Null
            } else {
                Sv::Text(valor.to_string())
            }
        }
        // Tipos ainda não suportados não chegam aqui: a coleção é recusada antes.
        _ => Sv::Null,
    }
}

/// Valor do banco para o JSON da resposta.
fn para_json(f: &Field, v: Sv) -> Value {
    match (f.kind, v) {
        (FieldType::Number, Sv::Integer(i)) => json!(i),
        (FieldType::Number, Sv::Real(x)) if x.fract() == 0.0 && x.abs() < 9.0e15 => json!(x as i64),
        (FieldType::Number, Sv::Real(x)) => json!(x),
        (FieldType::Bool, Sv::Integer(i)) => json!(i != 0),
        (FieldType::Json, Sv::Text(t)) => serde_json::from_str(&t).unwrap_or(Value::Null),
        (FieldType::Select, Sv::Text(t)) if f.multiplo() => serde_json::from_str(&t).unwrap_or(json!([])),
        (_, Sv::Text(t)) => json!(t),
        (_, Sv::Integer(i)) => json!(i),
        (_, Sv::Real(x)) => json!(x),
        (_, Sv::Null) => Value::Null,
        (_, Sv::Blob(_)) => Value::Null,
    }
}

fn colunas(c: &Collection) -> String {
    let mut cols = vec!["\"id\"".to_string()];
    cols.extend(c.fields.iter().map(|f| format!("\"{}\"", f.name)));
    cols.push("\"created\"".into());
    cols.push("\"updated\"".into());
    cols.join(", ")
}

fn linha_para_registro(c: &Collection, r: &rusqlite::Row) -> rusqlite::Result<Value> {
    let mut m = Map::new();
    m.insert("collectionId".into(), json!(c.id));
    m.insert("collectionName".into(), json!(c.name));
    m.insert("id".into(), json!(r.get::<_, String>(0)?));
    for (i, f) in c.fields.iter().enumerate() {
        m.insert(f.name.clone(), para_json(f, r.get::<_, Sv>(i + 1)?));
    }
    let n = c.fields.len();
    m.insert("created".into(), json!(r.get::<_, String>(n + 1)?));
    m.insert("updated".into(), json!(r.get::<_, String>(n + 2)?));
    Ok(Value::Object(m))
}

/// Regra de acesso + filtro do usuário em um `WHERE`, com os parâmetros.
fn onde(c: &Collection, regra: Option<&str>, filtro_usuario: Option<&str>) -> Resultado<(String, Vec<Sv>)> {
    let valido = |n: &str| c.coluna_valida(n);
    let mut partes = vec![];
    let mut params = vec![];
    for f in [regra, filtro_usuario].into_iter().flatten().filter(|f| !f.trim().is_empty()) {
        let sql = filtro::para_sql(f, &valido, Dialeto::Sqlite)?;
        partes.push(sql.texto);
        params.extend(sql.params.into_iter().map(literal_sql));
    }
    let texto = if partes.is_empty() { String::new() } else { format!(" WHERE {}", partes.join(" AND ")) };
    Ok((texto, params))
}

fn ler_colecao(r: &rusqlite::Row) -> rusqlite::Result<Collection> {
    let tipo: String = r.get("type")?;
    let campos: String = r.get("fields")?;
    let indices: String = r.get("indexes")?;
    Ok(Collection {
        id: r.get("id")?,
        name: r.get("name")?,
        kind: serde_json::from_value(json!(tipo)).unwrap_or_default(),
        system: r.get("system")?,
        fields: serde_json::from_str(&campos).unwrap_or_default(),
        list_rule: r.get("listRule")?,
        view_rule: r.get("viewRule")?,
        create_rule: r.get("createRule")?,
        update_rule: r.get("updateRule")?,
        delete_rule: r.get("deleteRule")?,
        indexes: serde_json::from_str(&indices).unwrap_or_default(),
        created: r.get("created")?,
        updated: r.get("updated")?,
    })
}

fn ler_superuser(r: &rusqlite::Row) -> rusqlite::Result<Superusuario> {
    Ok(Superusuario {
        collection_id: auth::COLECAO_SUPERUSERS.into(),
        collection_name: "_superusers".into(),
        id: r.get("id")?,
        email: r.get("email")?,
        senha: r.get("password")?,
        token_key: r.get("tokenKey")?,
        created: r.get("created")?,
        updated: r.get("updated")?,
    })
}

fn validar_colecao(c: &Collection) -> Resultado<()> {
    let mut erros = Map::new();
    if !nome_valido(&c.name) || c.name.starts_with('_') || c.name.to_lowercase().starts_with("sqlite_") {
        erro_campo(&mut erros, "name", "validation_invalid_name", "Invalid collection name.".into());
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
            erro_campo(&mut erros, &chave, "validation_invalid_name", "Invalid field name.".into());
        }
        if vistos.iter().any(|v| v.eq_ignore_ascii_case(&f.name)) {
            erro_campo(&mut erros, &chave, "validation_duplicated_field_name", "Duplicated field name.".into());
        }
        vistos.push(f.name.clone());
        if !f.kind.suportado() {
            erro_campo(
                &mut erros,
                &format!("fields.{i}.type"),
                "validation_not_supported",
                format!("Field type '{}' is not supported in this version.", serde_json::to_value(f.kind).unwrap_or_default().as_str().unwrap_or("")),
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
                && let Err(e) = filtro::para_sql(r, &|n| c.coluna_valida(n), Dialeto::Sqlite)
            {
                erro_campo(&mut erros, nome, "validation_invalid_rule", e.to_string());
            }
        }
    }
    if erros.is_empty() {
        Ok(())
    } else {
        Err(Erro::Invalido { mensagem: "Failed to create collection.".into(), campos: erros })
    }
}

impl Banco for Sqlite {
    fn dialeto(&self) -> Dialeto {
        Dialeto::Sqlite
    }

    fn segredo(&self) -> Resultado<String> {
        let conn = self.conn();
        if let Some(s) = conn
            .query_row("SELECT value FROM _params WHERE id = 'jwtSecret'", [], |r| r.get::<_, String>(0))
            .optional()?
        {
            return Ok(s);
        }
        let s = id::aleatorio(50);
        conn.execute("INSERT INTO _params (id, value) VALUES ('jwtSecret', ?1)", params![s])?;
        Ok(s)
    }

    fn colecoes(&self) -> Resultado<Vec<Collection>> {
        let conn = self.conn();
        let mut st = conn.prepare("SELECT * FROM _collections ORDER BY created, rowid")?;
        let v = st.query_map([], ler_colecao)?.collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(v)
    }

    fn colecao(&self, id_ou_nome: &str) -> Resultado<Collection> {
        self.conn()
            .query_row(
                "SELECT * FROM _collections WHERE id = ?1 OR name = ?1 COLLATE NOCASE",
                params![id_ou_nome],
                ler_colecao,
            )
            .optional()?
            .ok_or_else(|| Erro::NaoEncontrado("Missing collection context.".into()))
    }

    fn criar_colecao(&self, mut c: Collection) -> Resultado<Collection> {
        validar_colecao(&c)?;
        let agora = agora();
        c.id = if c.id.is_empty() { format!("pbc_{}", id::novo()) } else { c.id };
        c.system = false;
        c.created = agora.clone();
        c.updated = agora;
        for f in &mut c.fields {
            if f.id.is_empty() {
                let tipo = serde_json::to_value(f.kind).ok().and_then(|v| v.as_str().map(String::from)).unwrap_or_default();
                f.id = format!("{tipo}{}", id::aleatorio(10));
            }
        }
        let mut conn = self.conn();
        let tx = conn.transaction()?;
        let existe: bool = tx.query_row(
            "SELECT EXISTS (SELECT 1 FROM _collections WHERE name = ?1 COLLATE NOCASE OR id = ?2)",
            params![c.name, c.id],
            |r| r.get(0),
        )?;
        if existe {
            let mut erros = Map::new();
            erro_campo(&mut erros, "name", "validation_collection_name_exists", "Collection name must be unique (case insensitive).".into());
            return Err(Erro::Invalido { mensagem: "Failed to create collection.".into(), campos: erros });
        }
        let mut colunas_sql = vec!["\"id\" TEXT PRIMARY KEY NOT NULL".to_string()];
        colunas_sql.extend(c.fields.iter().map(|f| format!("\"{}\" {}", f.name, tipo_sql(f))));
        colunas_sql.push("\"created\" TEXT NOT NULL DEFAULT ''".into());
        colunas_sql.push("\"updated\" TEXT NOT NULL DEFAULT ''".into());
        tx.execute_batch(&format!("CREATE TABLE \"{}\" ({});", c.name, colunas_sql.join(", ")))?;
        tx.execute(
            "INSERT INTO _collections (id, name, type, system, fields, listRule, viewRule, createRule, updateRule, deleteRule, indexes, created, updated)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
            params![
                c.id,
                c.name,
                "base",
                c.system,
                serde_json::to_string(&c.fields).unwrap_or_else(|_| "[]".into()),
                c.list_rule,
                c.view_rule,
                c.create_rule,
                c.update_rule,
                c.delete_rule,
                serde_json::to_string(&c.indexes).unwrap_or_else(|_| "[]".into()),
                c.created,
                c.updated,
            ],
        )?;
        tx.commit()?;
        Ok(c)
    }

    fn apagar_colecao(&self, id_ou_nome: &str) -> Resultado<()> {
        let c = self.colecao(id_ou_nome)?;
        if c.system {
            return Err(Erro::invalido("System collections can't be deleted."));
        }
        let mut conn = self.conn();
        let tx = conn.transaction()?;
        tx.execute_batch(&format!("DROP TABLE IF EXISTS \"{}\";", c.name))?;
        tx.execute("DELETE FROM _collections WHERE id = ?1", params![c.id])?;
        tx.commit()?;
        Ok(())
    }

    fn listar(&self, c: &Collection, q: &Consulta) -> Resultado<Lista> {
        let por_pagina = q.por_pagina.clamp(1, 1000);
        let pagina = q.pagina.max(1);
        let (onde_sql, params) = onde(c, q.regra.as_deref(), q.filtro.as_deref())?;
        let ordem = match q.ordem.as_deref().filter(|s| !s.trim().is_empty()) {
            Some(s) => format!(" ORDER BY {}", filtro::ordem(s, &|n| c.coluna_valida(n), Dialeto::Sqlite)?),
            None => " ORDER BY rowid".into(),
        };
        let conn = self.conn();
        let total: i64 = if q.pular_total {
            -1
        } else {
            conn.query_row(
                &format!("SELECT COUNT(*) FROM \"{}\"{onde_sql}", c.name),
                params_from_iter(params.iter()),
                |r| r.get(0),
            )?
        };
        let sql = format!(
            "SELECT {} FROM \"{}\"{onde_sql}{ordem} LIMIT {por_pagina} OFFSET {}",
            colunas(c),
            c.name,
            (pagina as i64 - 1) * por_pagina as i64
        );
        let mut st = conn.prepare(&sql)?;
        let items = st
            .query_map(params_from_iter(params.iter()), |r| linha_para_registro(c, r))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let paginas = if total < 0 { -1 } else { (total + por_pagina as i64 - 1) / por_pagina as i64 };
        Ok(Lista { page: pagina, per_page: por_pagina, total_items: total, total_pages: paginas, items })
    }

    fn registro(&self, c: &Collection, id: &str, regra: Option<&str>) -> Resultado<Value> {
        let (onde_sql, mut params) = onde(c, regra, None)?;
        let filtro_id = if onde_sql.is_empty() { " WHERE \"id\" = ?".to_string() } else { format!("{onde_sql} AND \"id\" = ?") };
        params.push(Sv::Text(id.into()));
        let conn = self.conn();
        conn.query_row(
            &format!("SELECT {} FROM \"{}\"{filtro_id}", colunas(c), c.name),
            params_from_iter(params.iter()),
            |r| linha_para_registro(c, r),
        )
        .optional()?
        .ok_or_else(|| Erro::NaoEncontrado("The requested resource wasn't found.".into()))
    }

    fn criar(&self, c: &Collection, dados: &Map<String, Value>) -> Resultado<Value> {
        let mut erros = Map::new();
        let novo_id = match dados.get("id").and_then(Value::as_str).filter(|s| !s.is_empty()) {
            Some(s) if id::valido(s) => s.to_string(),
            Some(_) => {
                erro_campo(&mut erros, "id", "validation_invalid_id", "Must be 15 characters [a-z0-9].".into());
                String::new()
            }
            None => id::novo(),
        };
        let valores: Vec<Sv> = c.fields.iter().map(|f| valor_do_campo(f, dados.get(&f.name), &mut erros)).collect();
        if !erros.is_empty() {
            return Err(Erro::Invalido { mensagem: "Failed to create record.".into(), campos: erros });
        }
        let agora = agora();
        let mut todos = vec![Sv::Text(novo_id.clone())];
        todos.extend(valores);
        todos.push(Sv::Text(agora.clone()));
        todos.push(Sv::Text(agora));
        let marcadores = vec!["?"; todos.len()].join(", ");
        {
            let conn = self.conn();
            let r = conn.execute(
                &format!("INSERT INTO \"{}\" ({}) VALUES ({marcadores})", c.name, colunas(c)),
                params_from_iter(todos.iter()),
            );
            if let Err(rusqlite::Error::SqliteFailure(e, _)) = &r
                && e.code == rusqlite::ErrorCode::ConstraintViolation
            {
                let mut erros = Map::new();
                erro_campo(&mut erros, "id", "validation_invalid_id", "The id is invalid or already exists.".into());
                return Err(Erro::Invalido { mensagem: "Failed to create record.".into(), campos: erros });
            }
            r?;
        }
        self.registro(c, &novo_id, None)
    }

    fn atualizar(&self, c: &Collection, id: &str, dados: &Map<String, Value>, regra: Option<&str>) -> Resultado<Value> {
        let atual = self.registro(c, id, regra)?;
        let mut erros = Map::new();
        let mut sets = vec![];
        let mut valores = vec![];
        for f in &c.fields {
            // Só os campos enviados mudam; os outros são revalidados como estão
            // para manter "obrigatório" coerente.
            let v = dados.get(&f.name).or_else(|| atual.get(&f.name));
            let sv = valor_do_campo(f, v, &mut erros);
            if dados.contains_key(&f.name) {
                sets.push(format!("\"{}\" = ?", f.name));
                valores.push(sv);
            }
        }
        if !erros.is_empty() {
            return Err(Erro::Invalido { mensagem: "Failed to update record.".into(), campos: erros });
        }
        sets.push("\"updated\" = ?".into());
        valores.push(Sv::Text(agora()));
        valores.push(Sv::Text(id.into()));
        self.conn().execute(
            &format!("UPDATE \"{}\" SET {} WHERE \"id\" = ?", c.name, sets.join(", ")),
            params_from_iter(valores.iter()),
        )?;
        self.registro(c, id, None)
    }

    fn apagar(&self, c: &Collection, id: &str, regra: Option<&str>) -> Resultado<()> {
        self.registro(c, id, regra)?;
        self.conn().execute(&format!("DELETE FROM \"{}\" WHERE \"id\" = ?1", c.name), params![id])?;
        Ok(())
    }

    fn superuser_salvar(&self, email: &str, senha: &str) -> Resultado<Superusuario> {
        let mut erros = Map::new();
        let email = email.trim();
        let ok_email = email.split_once('@').is_some_and(|(a, d)| !a.is_empty() && d.contains('.'));
        if !ok_email {
            erro_campo(&mut erros, "email", "validation_is_email", "Must be a valid email address.".into());
        }
        if senha.chars().count() < 10 {
            erro_campo(&mut erros, "password", "validation_min_text_constraint", "Must be at least 10 character(s).".into());
        }
        if !erros.is_empty() {
            return Err(Erro::Invalido { mensagem: "Failed to save superuser.".into(), campos: erros });
        }
        let hash = auth::hash_senha(senha)?;
        let chave = id::aleatorio(50);
        let agora = agora();
        let conn = self.conn();
        let existente: Option<String> = conn
            .query_row("SELECT id FROM _superusers WHERE email = ?1 COLLATE NOCASE", params![email], |r| r.get(0))
            .optional()?;
        let id_su = match existente {
            Some(i) => {
                conn.execute(
                    "UPDATE _superusers SET password = ?1, tokenKey = ?2, updated = ?3 WHERE id = ?4",
                    params![hash, chave, agora, i],
                )?;
                i
            }
            None => {
                let i = id::novo();
                conn.execute(
                    "INSERT INTO _superusers (id, email, password, tokenKey, created, updated) VALUES (?1, ?2, ?3, ?4, ?5, ?5)",
                    params![i, email, hash, chave, agora],
                )?;
                i
            }
        };
        drop(conn);
        self.superuser(&id_su)
    }

    fn superuser_por_email(&self, email: &str) -> Resultado<Superusuario> {
        self.conn()
            .query_row("SELECT * FROM _superusers WHERE email = ?1 COLLATE NOCASE", params![email.trim()], ler_superuser)
            .optional()?
            .ok_or_else(|| Erro::NaoEncontrado("Superuser not found.".into()))
    }

    fn superuser(&self, id: &str) -> Resultado<Superusuario> {
        self.conn()
            .query_row("SELECT * FROM _superusers WHERE id = ?1", params![id], ler_superuser)
            .optional()?
            .ok_or_else(|| Erro::NaoEncontrado("Superuser not found.".into()))
    }

    fn total_superusers(&self) -> Resultado<i64> {
        Ok(self.conn().query_row("SELECT COUNT(*) FROM _superusers", [], |r| r.get(0))?)
    }
}

#[cfg(test)]
mod tests {
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

    #[test]
    fn colecao_cria_tabela_e_valida() {
        let b = Sqlite::em_memoria().unwrap();
        let c = b.criar_colecao(produtos()).unwrap();
        assert!(c.id.starts_with("pbc_") && c.fields.iter().all(|f| !f.id.is_empty()));
        assert_eq!(b.colecao("PRODUTOS").unwrap().id, c.id);
        assert_eq!(b.colecoes().unwrap().len(), 1);
        // Nome repetido, nome inválido, campo reservado, tipo ainda não suportado, regra inválida.
        assert!(b.criar_colecao(produtos()).is_err());
        let mut ruim = produtos();
        ruim.name = "x; DROP TABLE y".into();
        assert!(b.criar_colecao(ruim).is_err());
        let ruim: Collection = serde_json::from_value(json!({"name":"a","fields":[{"name":"id","type":"text"}]})).unwrap();
        assert!(b.criar_colecao(ruim).is_err());
        let ruim: Collection = serde_json::from_value(json!({"name":"b","fields":[{"name":"f","type":"file"}]})).unwrap();
        assert!(b.criar_colecao(ruim).is_err());
        let ruim: Collection = serde_json::from_value(json!({"name":"c","listRule":"senha = 1"})).unwrap();
        assert!(b.criar_colecao(ruim).is_err());
        b.apagar_colecao("produtos").unwrap();
        assert!(b.colecao("produtos").is_err());
    }

    #[test]
    fn registros_crud_filtro_ordem_e_paginas() {
        let b = Sqlite::em_memoria().unwrap();
        let c = b.criar_colecao(produtos()).unwrap();
        for (nome, preco, ativo) in [("Café", 32.0, true), ("Chá", 18.5, true), ("Bolo", 45.0, false)] {
            b.criar(&c, &obj(json!({"nome": nome, "preco": preco, "ativo": ativo, "tags": ["novo"]}))).unwrap();
        }
        let r = b.criar(&c, &obj(json!({"nome": "Pão", "preco": "7", "extra": {"a": 1}}))).unwrap();
        assert_eq!(r["preco"], json!(7));
        assert_eq!(r["extra"], json!({"a": 1}));
        assert_eq!(r["tags"], json!([]));
        assert_eq!(r["collectionName"], json!("produtos"));
        assert!(id::valido(r["id"].as_str().unwrap()));

        let q = Consulta { filtro: Some("preco < 40 && ativo = true".into()), ordem: Some("-preco".into()), ..Default::default() };
        let l = b.listar(&c, &q).unwrap();
        assert_eq!(l.total_items, 2);
        assert_eq!(l.items[0]["nome"], json!("Café"));
        assert_eq!(l.items[1]["preco"], json!(18.5));

        let p = b.listar(&c, &Consulta { por_pagina: 3, pagina: 2, ..Default::default() }).unwrap();
        assert_eq!((p.total_items, p.total_pages, p.items.len()), (4, 2, 1));
        let s = b.listar(&c, &Consulta { pular_total: true, ..Default::default() }).unwrap();
        assert_eq!((s.total_items, s.total_pages), (-1, -1));

        let pid = r["id"].as_str().unwrap();
        let u = b.atualizar(&c, pid, &obj(json!({"preco": 9})), None).unwrap();
        assert_eq!((u["preco"].clone(), u["nome"].clone()), (json!(9), json!("Pão")));
        // A regra de visualização esconde o registro inativo.
        assert!(b.registro(&c, pid, Some("ativo = true")).is_err());
        assert!(b.registro(&c, pid, None).is_ok());
        b.apagar(&c, pid, None).unwrap();
        assert!(b.registro(&c, pid, None).is_err());
    }

    #[test]
    fn validacao_com_codigos_do_pocketbase() {
        let b = Sqlite::em_memoria().unwrap();
        let c = b.criar_colecao(produtos()).unwrap();
        let e = b
            .criar(&c, &obj(json!({"preco": -1, "site": "ftp://x", "tags": ["outro", "novo", "promo"], "id": "CURTO"})))
            .unwrap_err();
        let Erro::Invalido { campos, .. } = e else { panic!() };
        assert_eq!(campos["nome"]["code"], json!("validation_required"));
        assert_eq!(campos["preco"]["code"], json!("validation_min_number_constraint"));
        assert_eq!(campos["site"]["code"], json!("validation_is_url"));
        assert!(campos.contains_key("tags"));
        assert_eq!(campos["id"]["code"], json!("validation_invalid_id"));
        // Atualizar tirando um obrigatório também falha.
        let r = b.criar(&c, &obj(json!({"nome": "ok"}))).unwrap();
        assert!(b.atualizar(&c, r["id"].as_str().unwrap(), &obj(json!({"nome": ""})), None).is_err());
    }

    #[test]
    fn superusuarios_e_segredo() {
        let b = Sqlite::em_memoria().unwrap();
        assert_eq!(b.total_superusers().unwrap(), 0);
        assert!(b.superuser_salvar("ana@x.com", "curta").is_err());
        let s = b.superuser_salvar("ana@x.com", "senha-forte-123").unwrap();
        assert!(auth::conferir_senha(&s.senha, "senha-forte-123"));
        let chave = s.token_key.clone();
        // Salvar de novo troca a senha e a chave, e mantém o id.
        let s2 = b.superuser_salvar("ANA@x.com", "outra-senha-456").unwrap();
        assert_eq!(s2.id, s.id);
        assert_ne!(s2.token_key, chave);
        assert_eq!(b.total_superusers().unwrap(), 1);
        let seg = b.segredo().unwrap();
        assert_eq!(seg.len(), 50);
        assert_eq!(b.segredo().unwrap(), seg);
    }

    #[test]
    fn dados_ficam_no_disco() {
        let d = tempfile::tempdir().unwrap();
        {
            let b = Sqlite::abrir(d.path()).unwrap();
            let c = b.criar_colecao(produtos()).unwrap();
            b.criar(&c, &obj(json!({"nome": "persistente"}))).unwrap();
        }
        let b = Sqlite::abrir(d.path()).unwrap();
        let c = b.colecao("produtos").unwrap();
        assert_eq!(b.listar(&c, &Consulta::default()).unwrap().items[0]["nome"], json!("persistente"));
    }
}
