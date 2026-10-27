//! Painel administrativo do Forja: HTML gerado no servidor, com maud, e htmx
//! embutido no binário. Não há build de front-end — o `cargo build` já leva o
//! painel junto (ARQUITETURA.md, decisão 10).

use forja_core::store::Lista;
use forja_core::{Collection, CollectionType, Field, FieldType};
use maud::{DOCTYPE, Markup, PreEscaped, html};
use serde_json::{Map, Value};

/// htmx 2.0.11 (BSD-2-Clause), baixado uma vez em `assets/`.
pub const HTMX: &[u8] = include_bytes!("../assets/htmx.min.js");

const CSS: &str = r#"
:root { --tinta:#1c1917; --fraco:#78716c; --linha:#e7e5e4; --fundo:#fafaf9; --marca:#b23a1f; }
* { box-sizing: border-box; }
body { margin:0; font:15px/1.5 ui-sans-serif, system-ui, -apple-system, "Segoe UI", sans-serif;
       color:var(--tinta); background:var(--fundo); }
a { color:var(--marca); text-decoration:none; }
a:hover { text-decoration:underline; }
.topo { display:flex; align-items:center; gap:24px; padding:0 24px; height:56px;
        background:#fff; border-bottom:1px solid var(--linha); }
.marca { font-weight:700; font-size:18px; color:var(--tinta); letter-spacing:-.02em; }
.marca::before { content:"▮"; color:var(--marca); margin-right:6px; }
.topo nav { display:flex; gap:18px; flex:1; }
.topo form { margin:0; }
.link { background:none; border:0; color:var(--fraco); cursor:pointer; font:inherit; padding:0; }
.link:hover { color:var(--tinta); }
.conteudo { max-width:1100px; margin:32px auto; padding:0 24px; }
.cabecalho { display:flex; align-items:baseline; gap:12px; margin-bottom:20px; }
.cabecalho h1 { font-size:24px; margin:0; letter-spacing:-.02em; }
.cabecalho .contagem { color:var(--fraco); font-size:13px; }
.cabecalho .acoes { margin-left:auto; display:flex; gap:10px; }
.botao { display:inline-block; padding:7px 14px; border-radius:8px; border:1px solid var(--linha);
         background:#fff; color:var(--tinta); font-size:14px; cursor:pointer; }
.botao:hover { border-color:var(--marca); text-decoration:none; }
.botao.solido { background:var(--marca); border-color:var(--marca); color:#fff; }
.botao.solido:hover { filter:brightness(.93); }
.botao.perigo { color:#b91c1c; }
table { width:100%; border-collapse:collapse; background:#fff; border:1px solid var(--linha);
        border-radius:10px; overflow:hidden; }
th, td { text-align:left; padding:10px 12px; border-bottom:1px solid var(--linha);
         vertical-align:top; font-size:14px; }
th { background:#f5f5f4; font-weight:600; font-size:12px; text-transform:uppercase;
     letter-spacing:.04em; color:var(--fraco); }
tr:last-child td { border-bottom:0; }
td.direita, th.direita { text-align:right; white-space:nowrap; }
.etiqueta { display:inline-block; padding:1px 8px; border-radius:999px; background:#f5f5f4;
            color:var(--fraco); font-size:12px; }
.vazio { color:var(--fraco); background:#fff; border:1px dashed var(--linha);
         border-radius:10px; padding:28px; text-align:center; }
.busca { margin-bottom:12px; }
.busca input { width:100%; max-width:380px; padding:8px 12px; border:1px solid var(--linha);
               border-radius:8px; font:inherit; background:#fff; }
.paginacao { display:flex; gap:12px; align-items:center; margin-top:14px; color:var(--fraco);
             font-size:13px; }
.formulario { background:#fff; border:1px solid var(--linha); border-radius:10px;
              padding:24px; max-width:640px; }
.campo { margin-bottom:18px; }
.campo label { display:block; font-size:13px; font-weight:600; margin-bottom:6px; }
.campo .obrigatorio { color:var(--marca); }
.campo input, .campo select, .campo textarea {
  width:100%; padding:8px 10px; border:1px solid var(--linha); border-radius:8px;
  font:inherit; background:#fff; }
.campo textarea { min-height:120px; resize:vertical; font-family:ui-monospace, monospace;
                  font-size:13px; }
.campo .erro { color:#b91c1c; font-size:13px; margin-top:5px; }
.campo .dica { color:var(--fraco); font-size:12px; margin-top:5px; }
.global { background:#fef2f2; border:1px solid #fecaca; color:#991b1b; border-radius:8px;
          padding:10px 14px; margin-bottom:18px; font-size:14px; }
.cartao { max-width:380px; margin:10vh auto; background:#fff; border:1px solid var(--linha);
          border-radius:12px; padding:30px; }
.cartao h1 { font-size:22px; margin:0 0 6px; }
.cartao .ajuda { color:var(--fraco); font-size:14px; margin:0 0 20px; }
.cartao .campo input { font-size:15px; }
.cartao button { width:100%; margin-top:6px; }
"#;

/// Tela de onboarding do primeiro superusuário (o "usuário mestre").
///
/// Aparece quando o projeto ainda não tem nenhum superusuário. Exige o segredo
/// de instalação, que sai no console na subida do servidor.
pub fn pagina_install(erro: Option<&str>, token: &str, emails: &[String]) -> Markup {
    html! {
        (DOCTYPE)
        html lang="pt-BR" {
            head {
                meta charset="utf-8";
                meta name="viewport" content="width=device-width, initial-scale=1";
                title { "Instalação · Forja" }
                style { (PreEscaped(CSS)) }
            }
            body {
                div .cartao {
                    h1 { "Criar o usuário mestre" }
                    p .ajuda {
                        "Este projeto ainda não tem superusuário. Defina o e-mail e a senha "
                        "do usuário mestre para concluir a instalação."
                    }
                    @if let Some(m) = erro {
                        div .global { (m) }
                    }
                    form method="post" action="/_/install" {
                        input type="hidden" name="token" value=(token);
                        div .campo {
                            label for="email" { "E-mail" }
                            input type="email" id="email" name="email"
                                  autocomplete="username" required autofocus;
                            @if !emails.is_empty() {
                                p .dica { "Permitidos: " (emails.join(", ")) }
                            }
                        }
                        div .campo {
                            label for="password" { "Senha" }
                            input type="password" id="password" name="password"
                                  autocomplete="new-password" minlength="10" required;
                            p .dica { "Pelo menos 10 caracteres." }
                        }
                        div .campo {
                            label for="passwordConfirm" { "Repita a senha" }
                            input type="password" id="passwordConfirm" name="passwordConfirm"
                                  autocomplete="new-password" minlength="10" required;
                        }
                        button .botao.solido type="submit" { "Criar e entrar" }
                    }
                }
            }
        }
    }
}

/// Página completa com o cabeçalho do painel.
pub fn pagina(titulo: &str, conteudo: Markup) -> Markup {
    html! {
        (DOCTYPE)
        html lang="pt-BR" {
            head {
                meta charset="utf-8";
                meta name="viewport" content="width=device-width, initial-scale=1";
                title { (titulo) " · Forja" }
                style { (PreEscaped(CSS)) }
                script src="/_/htmx.min.js" defer {}
            }
            body {
                header .topo {
                    a .marca href="/_/" { "Forja" }
                    nav { a href="/_/" { "Coleções" } }
                    form method="post" action="/_/logout" {
                        button .link type="submit" { "Sair" }
                    }
                }
                main .conteudo { (conteudo) }
            }
        }
    }
}

/// Tela de login, sem o cabeçalho do painel.
pub fn pagina_login(erro: Option<&str>) -> Markup {
    html! {
        (DOCTYPE)
        html lang="pt-BR" {
            head {
                meta charset="utf-8";
                meta name="viewport" content="width=device-width, initial-scale=1";
                title { "Entrar · Forja" }
                style { (PreEscaped(CSS)) }
            }
            body {
                div .cartao {
                    h1 { "Entrar no painel" }
                    p .ajuda { "Use o e-mail e a senha de um superusuário." }
                    @if let Some(m) = erro {
                        div .global { (m) }
                    }
                    form method="post" action="/_/login" {
                        div .campo {
                            label for="identity" { "E-mail" }
                            input type="email" id="identity" name="identity"
                                  autocomplete="username" required autofocus;
                        }
                        div .campo {
                            label for="password" { "Senha" }
                            input type="password" id="password" name="password"
                                  autocomplete="current-password" required;
                        }
                        button .botao.solido type="submit" { "Entrar" }
                    }
                }
            }
        }
    }
}

/// Lista de coleções.
pub fn colecoes(itens: &[Collection]) -> Markup {
    html! {
        div .cabecalho {
            h1 { "Coleções" }
            span .contagem { (itens.len()) " no projeto" }
        }
        @if itens.is_empty() {
            p .vazio { "Nenhuma coleção ainda. Crie uma pela API ou pela CLI." }
        } @else {
            table {
                thead {
                    tr {
                        th { "Nome" }
                        th { "Tipo" }
                        th { "Campos" }
                        th .direita { "" }
                    }
                }
                tbody {
                    @for c in itens {
                        tr {
                            td {
                                a href=(format!("/_/collections/{}", c.name)) { (c.name) }
                            }
                            td { span .etiqueta { (tipo_nome(c.kind)) } }
                            td { (c.fields.len()) }
                            td .direita {
                                a .botao href=(format!("/_/collections/{}", c.name)) { "Abrir" }
                            }
                        }
                    }
                }
            }
        }
    }
}

/// Página de uma coleção: o cabeçalho e a busca ficam fora do alvo do htmx,
/// para o campo não perder o foco a cada resposta.
pub fn colecao(c: &Collection, lista: &Lista, filtro: &str) -> Markup {
    html! {
        div .cabecalho {
            h1 { (c.name) }
            span .contagem { (tipo_nome(c.kind)) " · " (lista.total_items) " registros" }
            div .acoes {
                @if c.kind == CollectionType::View {
                    span .etiqueta { "somente leitura" }
                } @else {
                    a .botao.solido href=(format!("/_/collections/{}/new", c.name)) { "Novo" }
                }
            }
        }
        @if let Some(q) = &c.view_query {
            p { code { (q) } }
        }
        form .busca
             hx-get=(format!("/_/collections/{}/tabela", c.name))
             hx-target="#tabela"
             hx-trigger="keyup changed delay:350ms, search"
             hx-push-url="false" {
            input type="search" name="filter" value=(filtro)
                  placeholder="filtrar (ex.: nome ~ 'ana')";
        }
        (tabela(c, lista, filtro))
    }
}

/// Só a tabela, com paginação. É o fragmento que o htmx troca.
pub fn tabela(c: &Collection, lista: &Lista, filtro: &str) -> Markup {
    let colunas: Vec<&Field> = c
        .fields
        .iter()
        .filter(|f| visivel_na_lista(f))
        .take(7)
        .collect();
    html! {
        div id="tabela" {
            @if lista.items.is_empty() {
                p .vazio { "Nenhum registro encontrado." }
            } @else {
                table {
                    thead {
                        tr {
                            th { "id" }
                            @for f in &colunas { th { (f.name) } }
                            th .direita { "" }
                        }
                    }
                    tbody {
                        @for r in &lista.items {
                            @let id = r["id"].as_str().unwrap_or_default();
                            tr {
                                td {
                                    @if c.kind == CollectionType::View {
                                        (id)
                                    } @else {
                                        a href=(format!("/_/collections/{}/records/{id}", c.name)) { (id) }
                                    }
                                }
                                @for f in &colunas {
                                    td { (valor_curto(r.get(&f.name))) }
                                }
                                td .direita {
                                    @if c.kind != CollectionType::View {
                                        a .botao href=(format!("/_/collections/{}/records/{id}", c.name)) { "Editar" }
                                        button .botao.perigo type="button"
                                            hx-post=(format!("/_/collections/{}/records/{id}/delete", c.name))
                                            hx-confirm="Apagar este registro?"
                                            hx-target="#tabela"
                                            hx-swap="outerHTML" { "Apagar" }
                                    }
                                }
                            }
                        }
                    }
                }
            }
            @if lista.total_pages > 1 {
                div .paginacao {
                    @if lista.page > 1 {
                        @let p = lista.page - 1;
                        a href=(url_pagina(c, p, filtro))
                          hx-get=(format!("/_/collections/{}/tabela?page={p}&filter={}", c.name, url_encode(filtro)))
                          hx-target="#tabela"
                          hx-push-url=(url_pagina(c, p, filtro)) { "← anterior" }
                    }
                    span { "página " (lista.page) " de " (lista.total_pages) }
                    @if lista.page < lista.total_pages as u32 {
                        @let p = lista.page + 1;
                        a href=(url_pagina(c, p, filtro))
                          hx-get=(format!("/_/collections/{}/tabela?page={p}&filter={}", c.name, url_encode(filtro)))
                          hx-target="#tabela"
                          hx-push-url=(url_pagina(c, p, filtro)) { "próxima →" }
                    }
                }
            }
        }
    }
}

/// Formulário de criação (`registro` = `None`) ou edição.
pub fn formulario(
    c: &Collection,
    registro: Option<&Value>,
    erros: &Map<String, Value>,
    mensagem: Option<&str>,
) -> Markup {
    let editando = registro.is_some();
    let titulo = if editando {
        format!("Editar {}", c.name)
    } else {
        format!("Novo em {}", c.name)
    };
    let acao = match registro.and_then(|r| r["id"].as_str()) {
        Some(id) => format!("/_/collections/{}/records/{id}", c.name),
        None => format!("/_/collections/{}", c.name),
    };
    html! {
        div .cabecalho {
            h1 { (titulo) }
            div .acoes {
                a .botao href=(format!("/_/collections/{}", c.name)) { "Voltar" }
            }
        }
        @if let Some(m) = mensagem {
            div .global { (m) }
        }
        form .formulario method="post" action=(acao) {
            @for f in c.fields.iter().filter(|f| editavel(f)) {
                (campo(f, registro, erros, editando))
            }
            button .botao.solido type="submit" { "Salvar" }
        }
    }
}

pub fn nao_encontrado(mensagem: &str) -> Markup {
    html! {
        div .cabecalho { h1 { "Não encontrado" } }
        p .vazio { (mensagem) }
        p { a .botao href="/_/" { "Voltar para as coleções" } }
    }
}

// ── Auxiliares ───────────────────────────────────────────────────────────

fn tipo_nome(k: CollectionType) -> &'static str {
    match k {
        CollectionType::Base => "base",
        CollectionType::Auth => "auth",
        CollectionType::View => "view",
    }
}

/// Campos que aparecem na tabela: tudo que não é segredo.
fn visivel_na_lista(f: &Field) -> bool {
    !f.hidden
        && f.kind != FieldType::Password
        && !(f.system && f.name == "tokenKey")
        && !matches!(f.name.as_str(), "created" | "updated")
}

/// Campos do formulário: os visíveis, mais a senha (que é `hidden`).
fn editavel(f: &Field) -> bool {
    if f.system && f.name == "tokenKey" {
        return false;
    }
    if f.kind == FieldType::Password {
        return true;
    }
    !f.hidden
}

fn campo(
    f: &Field,
    registro: Option<&Value>,
    erros: &Map<String, Value>,
    editando: bool,
) -> Markup {
    let valor = registro.and_then(|r| r.get(&f.name));
    let erro = erros
        .get(&f.name)
        .and_then(|e| e["message"].as_str())
        .map(str::to_string);
    html! {
        div .campo {
            label for=(f.name) {
                (f.name)
                @if f.required { span .obrigatorio { " *" } }
            }
            @match f.kind {
                FieldType::Bool => {
                    @let v = valor.and_then(Value::as_bool).unwrap_or(false);
                    select id=(f.name) name=(f.name) {
                        @if v {
                            option value="true" selected { "Sim" }
                            option value="false" { "Não" }
                        } @else {
                            option value="true" { "Sim" }
                            option value="false" selected { "Não" }
                        }
                    }
                }
                FieldType::Number => {
                    @let v = valor.map(texto_puro).unwrap_or_default();
                    input type="number" step="any" id=(f.name) name=(f.name) value=(v) required[f.required];
                }
                FieldType::Editor | FieldType::Json => {
                    @let v = valor.map(texto_puro).unwrap_or_default();
                    textarea id=(f.name) name=(f.name) required[f.required] { (v) }
                }
                FieldType::Password => {
                    input type="password" id=(f.name) name=(f.name) value=""
                          autocomplete="new-password" required[!editando];
                    @if editando {
                        p .dica { "Deixe em branco para manter a senha atual." }
                    }
                }
                FieldType::Select => {
                    @let opcoes: Vec<String> = f
                        .options
                        .get("values")
                        .and_then(Value::as_array)
                        .map(|a| a.iter().filter_map(Value::as_str).map(str::to_string).collect())
                        .unwrap_or_default();
                    @let atuais: Vec<String> = match valor {
                        Some(Value::Array(a)) => a.iter().filter_map(Value::as_str).map(str::to_string).collect(),
                        Some(Value::String(s)) => vec![s.clone()],
                        _ => vec![],
                    };
                    @if f.multiplo() {
                        select id=(f.name) name=(f.name) multiple required[f.required] {
                            @for o in &opcoes {
                                @if atuais.contains(o) {
                                    option value=(o) selected { (o) }
                                } @else {
                                    option value=(o) { (o) }
                                }
                            }
                        }
                        p .dica { "Ctrl/Cmd para escolher mais de um." }
                    } @else {
                        select id=(f.name) name=(f.name) required[f.required] {
                            option value="" { "—" }
                            @for o in &opcoes {
                                @if atuais.first() == Some(o) {
                                    option value=(o) selected { (o) }
                                } @else {
                                    option value=(o) { (o) }
                                }
                            }
                        }
                    }
                }
                FieldType::Email => {
                    @let v = valor.map(texto_puro).unwrap_or_default();
                    input type="email" id=(f.name) name=(f.name) value=(v) required[f.required];
                }
                FieldType::Url => {
                    @let v = valor.map(texto_puro).unwrap_or_default();
                    input type="url" id=(f.name) name=(f.name) value=(v) required[f.required];
                }
                FieldType::File => {
                    @let v = match valor {
                        Some(Value::Array(a)) => a
                            .iter()
                            .filter_map(Value::as_str)
                            .collect::<Vec<_>>()
                            .join(", "),
                        Some(Value::String(s)) if !s.is_empty() => s.clone(),
                        _ => "—".into(),
                    };
                    p { (v) }
                    p .dica { "O envio do arquivo é pela API, em multipart." }
                }
                FieldType::Relation => {
                    @let v = match valor {
                        Some(Value::Array(a)) => a
                            .iter()
                            .filter_map(Value::as_str)
                            .collect::<Vec<_>>()
                            .join(", "),
                        Some(Value::String(s)) => s.clone(),
                        Some(outro) => texto_puro(outro),
                        None => String::new(),
                    };
                    input type="text" id=(f.name) name=(f.name) value=(v) required[f.required];
                    @if f.multiplo() {
                        p .dica { "Ids separados por vírgula." }
                    } @else {
                        p .dica { "Id do registro relacionado." }
                    }
                }
                _ => {
                    @let v = valor.map(texto_puro).unwrap_or_default();
                    input type="text" id=(f.name) name=(f.name) value=(v) required[f.required];
                }
            }
            @if let Some(m) = erro {
                div .erro { (m) }
            }
        }
    }
}

fn texto_puro(v: &Value) -> String {
    match v {
        Value::Null => String::new(),
        Value::String(s) => s.clone(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => n.to_string(),
        _ => serde_json::to_string(v).unwrap_or_default(),
    }
}

fn valor_curto(v: Option<&Value>) -> String {
    let s = match v {
        None | Some(Value::Null) => "—".to_string(),
        Some(Value::Bool(b)) => (if *b { "Sim" } else { "Não" }).to_string(),
        Some(Value::String(s)) if s.is_empty() => "—".to_string(),
        Some(Value::String(s)) => s.clone(),
        Some(Value::Array(_)) | Some(Value::Object(_)) => {
            serde_json::to_string(v.unwrap()).unwrap_or_default()
        }
        Some(Value::Number(n)) => n.to_string(),
    };
    let limite: usize = 48;
    if s.chars().count() > limite {
        format!("{}…", s.chars().take(limite).collect::<String>())
    } else {
        s
    }
}

fn url_pagina(c: &Collection, pagina: u32, filtro: &str) -> String {
    if filtro.is_empty() {
        format!("/_/collections/{}?page={pagina}", c.name)
    } else {
        format!(
            "/_/collections/{}?page={pagina}&filter={}",
            c.name,
            url_encode(filtro)
        )
    }
}

/// Escapa o suficiente para caber num valor de query string.
fn url_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            b' ' => out.push('+'),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}
