//! Painel administrativo do Forja: HTML gerado no servidor, com maud, e htmx
//! embutido no binário. Não há build de front-end — o `cargo build` já leva o
//! painel junto (ARQUITETURA.md, decisão 10).

use forja_core::store::{BackupItem, Configuracoes, CronItem, Lista, LogItem, LogStat};
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
.linha-campo { display:flex; gap:10px; margin-bottom:10px; align-items:center; }
.linha-campo input, .linha-campo select { padding:6px 10px; border:1px solid var(--linha); border-radius:8px; font:inherit; background:#fff; }
.zona-perigo { margin-top:32px; padding:20px; border:1px solid #fecaca; border-radius:10px; background:#fff5f5; max-width:640px; }
.zona-perigo h3 { margin:0 0 8px; color:#b91c1c; font-size:16px; }
.zona-perigo p { margin:0 0 14px; font-size:13px; color:var(--fraco); }
.status-badge { display:inline-block; padding:2px 8px; border-radius:6px; font-weight:600; font-size:12px; }
.status-2xx { background:#dcfce7; color:#15803d; }
.status-3xx { background:#e0f2fe; color:#0369a1; }
.status-4xx { background:#fef9c3; color:#a16207; }
.status-5xx { background:#fee2e2; color:#b91c1c; }
.grafico-barras { display:flex; gap:6px; align-items:flex-end; height:70px; padding:10px 0; border-bottom:1px solid var(--linha); margin-bottom:20px; }
.barra-item { flex:1; background:#e7e5e4; border-radius:3px 3px 0 0; min-height:4px; position:relative; }
.barra-item:hover { background:var(--marca); }
.secao-config { background:#fff; border:1px solid var(--linha); border-radius:10px; padding:20px; margin-bottom:20px; max-width:720px; }
.secao-config h2 { font-size:17px; margin:0 0 14px; border-bottom:1px solid var(--linha); padding-bottom:8px; }
.grid-2 { display:grid; grid-template-columns:1fr 1fr; gap:14px; }
.sucesso { background:#f0fdf4; border:1px solid #bbf7d0; color:#166534; border-radius:8px; padding:10px 14px; margin-bottom:18px; font-size:14px; }
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
                    nav {
                        a href="/_/" { "Coleções" }
                        a href="/_/logs" { "Logs" }
                        a href="/_/crons" { "Crons" }
                        a href="/_/backups" { "Backups" }
                        a href="/_/settings" { "Configurações" }
                    }
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
            div .acoes {
                a .botao.solido href="/_/new-collection" { "+ Nova coleção" }
            }
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
                a .botao href=(format!("/_/collections/{}/settings", c.name)) { "Configurações" }
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

/// Fragmento de linha para adicionar um campo novo via htmx.
pub fn linha_campo() -> Markup {
    html! {
        div .linha-campo {
            input type="text" name="field_name" placeholder="Nome do campo (ex: titulo)" required style="flex:2;";
            select name="field_type" style="flex:1.5;" {
                option value="text" { "text" }
                option value="number" { "number" }
                option value="bool" { "bool" }
                option value="editor" { "editor" }
                option value="email" { "email" }
                option value="url" { "url" }
                option value="date" { "date" }
                option value="select" { "select" }
                option value="file" { "file" }
                option value="relation" { "relation" }
                option value="json" { "json" }
            }
            select name="field_required" style="flex:1;" {
                option value="false" { "Opcional" }
                option value="true" { "Obrigatório" }
            }
            button .botao.perigo type="button" onclick="this.closest('.linha-campo').remove()" title="Remover campo" { "✕" }
        }
    }
}

fn campo_regra(rotulo: &str, nome: &str, valor_atual: Option<&str>) -> Markup {
    let (tipo, expr) = match valor_atual {
        None => ("super", ""),
        Some("") => ("public", ""),
        Some(s) => ("custom", s),
    };
    html! {
        div .campo {
            label { (rotulo) }
            div style="display:flex; gap:8px;" {
                select name=(format!("{nome}_tipo")) style="flex:1;" {
                    option value="super" selected[tipo == "super"] { "Apenas Superusuário" }
                    option value="public" selected[tipo == "public"] { "Público (aberto a todos)" }
                    option value="custom" selected[tipo == "custom"] { "Expressão personalizada" }
                }
                input type="text" name=(format!("{nome}_expr")) value=(expr)
                      placeholder="ex: @request.auth.id != ''" style="flex:2;";
            }
        }
    }
}

/// Tela para criar uma nova coleção.
pub fn pagina_nova_colecao(erro: Option<&str>) -> Markup {
    html! {
        div .cabecalho {
            h1 { "Nova Coleção" }
            div .acoes {
                a .botao href="/_/" { "Voltar" }
            }
        }
        @if let Some(m) = erro {
            div .global { (m) }
        }
        form .formulario method="post" action="/_/new-collection" {
            div .campo {
                label for="name" { "Nome da coleção" span .obrigatorio { " *" } }
                input type="text" id="name" name="name" required placeholder="ex: artigos, categorias";
                p .dica { "Apenas letras minúsculas, números e sublinhados." }
            }
            div .campo {
                label for="type" { "Tipo de coleção" }
                select id="type" name="type" onchange="document.getElementById('bloco-view').style.display = this.value === 'view' ? 'block' : 'none'; document.getElementById('bloco-campos').style.display = this.value === 'view' ? 'none' : 'block';" {
                    option value="base" selected { "Base (registros normais)" }
                    option value="auth" { "Auth (usuários com senha/autenticação)" }
                    option value="view" { "View (consulta SQL somente leitura)" }
                }
            }
            div id="bloco-view" .campo style="display:none;" {
                label for="view_query" { "Consulta SQL da View (viewQuery)" span .obrigatorio { " *" } }
                textarea id="view_query" name="view_query" placeholder="SELECT id, nome, preco FROM produtos WHERE ativo = 1" {}
                p .dica { "A consulta precisa incluir obrigatoriamente a coluna 'id'." }
            }
            h2 style="font-size:16px; margin:24px 0 12px;" { "Regras de Acesso" }
            (campo_regra("Listar registros (listRule)", "list_rule", None))
            (campo_regra("Visualizar registro (viewRule)", "view_rule", None))
            (campo_regra("Criar registro (createRule)", "create_rule", None))
            (campo_regra("Atualizar registro (updateRule)", "update_rule", None))
            (campo_regra("Apagar registro (deleteRule)", "delete_rule", None))

            div id="bloco-campos" {
                div style="display:flex; justify-content:space-between; align-items:center; margin:24px 0 12px;" {
                    h2 style="font-size:16px; margin:0;" { "Campos Personalizados" }
                    button .botao type="button"
                           hx-get="/_/htmx/campo-linha"
                           hx-target="#campos"
                           hx-swap="beforeend" { "+ Adicionar campo" }
                }
                div id="campos" {
                    (linha_campo())
                }
            }
            div style="margin-top:24px;" {
                button .botao.solido type="submit" { "Criar coleção" }
            }
        }
    }
}

/// Tela para editar configurações, regras e adicionar campos a uma coleção existente.
pub fn pagina_editar_colecao(c: &Collection, erro: Option<&str>) -> Markup {
    html! {
        div .cabecalho {
            h1 { "Configurações: " (c.name) }
            div .acoes {
                a .botao href=(format!("/_/collections/{}", c.name)) { "Voltar para registros" }
            }
        }
        @if let Some(m) = erro {
            div .global { (m) }
        }
        form .formulario method="post" action=(format!("/_/collections/{}/settings", c.name)) {
            div .campo {
                label for="name" { "Nome da coleção" span .obrigatorio { " *" } }
                @if c.system {
                    input type="text" id="name" name="name" value=(c.name) readonly;
                    p .dica { "O nome de coleções de sistema não pode ser alterado." }
                } @else {
                    input type="text" id="name" name="name" value=(c.name) required;
                }
            }
            div .campo {
                label { "Tipo de coleção" }
                input type="text" value=(tipo_nome(c.kind)) disabled;
            }
            @if c.kind == CollectionType::View {
                div .campo {
                    label for="view_query" { "Consulta SQL da View (viewQuery)" span .obrigatorio { " *" } }
                    textarea id="view_query" name="view_query" required { (c.view_query.as_deref().unwrap_or_default()) }
                    p .dica { "A consulta precisa incluir obrigatoriamente a coluna 'id'." }
                }
            }

            h2 style="font-size:16px; margin:24px 0 12px;" { "Regras de Acesso" }
            (campo_regra("Listar registros (listRule)", "list_rule", c.list_rule.as_deref()))
            (campo_regra("Visualizar registro (viewRule)", "view_rule", c.view_rule.as_deref()))
            @if c.kind != CollectionType::View {
                (campo_regra("Criar registro (createRule)", "create_rule", c.create_rule.as_deref()))
                (campo_regra("Atualizar registro (updateRule)", "update_rule", c.update_rule.as_deref()))
                (campo_regra("Apagar registro (deleteRule)", "delete_rule", c.delete_rule.as_deref()))
            }

            @if c.kind != CollectionType::View {
                h2 style="font-size:16px; margin:24px 0 12px;" { "Campos Existentes" }
                table style="margin-bottom:18px;" {
                    thead {
                        tr {
                            th { "Nome" }
                            th { "Tipo" }
                            th { "Obrigatório" }
                            th { "Origem" }
                        }
                    }
                    tbody {
                        @for f in &c.fields {
                            tr {
                                td { code { (f.name) } }
                                td { span .etiqueta { (tipo_campo_nome(f.kind)) } }
                                td { @if f.required { "Sim" } @else { "Não" } }
                                td { @if f.system { span .etiqueta { "sistema" } } @else { "usuário" } }
                            }
                        }
                    }
                }

                div style="display:flex; justify-content:space-between; align-items:center; margin:24px 0 12px;" {
                    h2 style="font-size:16px; margin:0;" { "Adicionar Novos Campos" }
                    button .botao type="button"
                           hx-get="/_/htmx/campo-linha"
                           hx-target="#campos"
                           hx-swap="beforeend" { "+ Adicionar campo" }
                }
                div id="campos" {}
            }

            div style="margin-top:24px;" {
                button .botao.solido type="submit" { "Salvar alterações" }
            }
        }

        @if !c.system {
            div .zona-perigo {
                h3 { "Zona de Perigo" }
                p { "Excluir esta coleção apagará permanentemente a tabela e todos os registros associados." }
                form method="post" action=(format!("/_/collections/{}/delete-collection", c.name)) {
                    button .botao.perigo type="submit"
                           hx-confirm=(format!("Tem certeza que deseja apagar a coleção '{}' e todos os dados? Esta ação não pode ser desfeita.", c.name)) {
                        "Excluir coleção"
                    }
                }
            }
        }
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

fn tipo_campo_nome(k: FieldType) -> &'static str {
    match k {
        FieldType::Text => "text",
        FieldType::Editor => "editor",
        FieldType::Number => "number",
        FieldType::Bool => "bool",
        FieldType::Email => "email",
        FieldType::Url => "url",
        FieldType::Date => "date",
        FieldType::Autodate => "autodate",
        FieldType::Select => "select",
        FieldType::File => "file",
        FieldType::Relation => "relation",
        FieldType::Json => "json",
        FieldType::Password => "password",
        FieldType::GeoPoint => "geoPoint",
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

// ── Páginas do Marco 0.5: Logs, Crons, Backups, Configurações ────────────

/// Tela de Logs de Requisições com gráfico horário e tabela de eventos.
pub fn pagina_logs(itens: &[LogItem], stats: &[LogStat]) -> Markup {
    let max_stat = stats.iter().map(|s| s.total).max().unwrap_or(1).max(1);
    pagina("Logs", html! {
        div .cabecalho {
            h1 { "Logs de Requisições" }
            span .contagem { (itens.len()) " eventos recentes" }
        }

        @if !stats.is_empty() {
            div .secao-config {
                h2 { "Volume de requisições por hora" }
                div .grafico-barras {
                    @for s in stats {
                        @let altura = ((s.total as f64 / max_stat as f64) * 55.0).max(4.0) as i64;
                        div .barra-item style=(format!("height: {altura}px")) title=(format!("{}: {} requisições", s.date, s.total)) {}
                    }
                }
            }
        }

        @if itens.is_empty() {
            p .vazio { "Nenhum log registrado ainda." }
        } @else {
            table {
                thead {
                    tr {
                        th { "Data/Hora (UTC)" }
                        th { "Status" }
                        th { "Método" }
                        th { "URL" }
                        th { "Duração" }
                        th { "Nível" }
                    }
                }
                tbody {
                    @for item in itens {
                        @let status = item.data.get("status").and_then(Value::as_u64).unwrap_or(200);
                        @let method = item.data.get("method").and_then(Value::as_str).unwrap_or("GET");
                        @let url = item.data.get("url").and_then(Value::as_str).unwrap_or(&item.message);
                        @let exec_time = item.data.get("execTime").and_then(Value::as_f64).unwrap_or(0.0);
                        @let badge_class = if status >= 500 {
                            "status-badge status-5xx"
                        } else if status >= 400 {
                            "status-badge status-4xx"
                        } else if status >= 300 {
                            "status-badge status-3xx"
                        } else {
                            "status-badge status-2xx"
                        };

                        tr {
                            td { (item.created) }
                            td { span class=(badge_class) { (status) } }
                            td { strong { (method) } }
                            td { code { (url) } }
                            td { (format!("{:.2} ms", exec_time * 1000.0)) }
                            td { (item.level) }
                        }
                    }
                }
            }
        }
    })
}

/// Tela de Tarefas Agendadas (Crons).
pub fn pagina_crons(itens: &[CronItem]) -> Markup {
    pagina("Crons", html! {
        div .cabecalho {
            h1 { "Tarefas Agendadas" }
            span .contagem { (itens.len()) " agendamentos ativos" }
        }

        @if itens.is_empty() {
            p .vazio { "Nenhuma tarefa cron registrada." }
        } @else {
            table {
                thead {
                    tr {
                        th { "Identificador" }
                        th { "Expressão Cron" }
                        th .direita { "Ações" }
                    }
                }
                tbody {
                    @for item in itens {
                        tr {
                            td { strong { (item.id) } }
                            td { code { (item.expression) } }
                            td .direita {
                                form method="post" action=(format!("/_/crons/{}/run", item.id)) style="display:inline;" {
                                    button .botao type="submit" { "Executar agora" }
                                }
                            }
                        }
                    }
                }
            }
        }
    })
}

/// Tela de Gerenciamento de Backups.
pub fn pagina_backups(itens: &[BackupItem], erro: Option<&str>, sucesso: Option<&str>) -> Markup {
    pagina("Backups", html! {
        div .cabecalho {
            h1 { "Backups" }
            span .contagem { (itens.len()) " snapshots salvos" }
            div .acoes {
                form method="post" action="/_/backups/new" style="margin:0;" {
                    button .botao.solido type="submit" { "+ Criar backup" }
                }
            }
        }

        @if let Some(m) = erro {
            div .global { (m) }
        }
        @if let Some(s) = sucesso {
            div .sucesso { (s) }
        }

        @if itens.is_empty() {
            p .vazio { "Nenhum backup disponível. Clique no botão acima para criar o primeiro snapshot." }
        } @else {
            table {
                thead {
                    tr {
                        th { "Arquivo" }
                        th { "Data de Modificação" }
                        th { "Tamanho" }
                        th .direita { "Ações" }
                    }
                }
                tbody {
                    @for item in itens {
                        @let tamanho_kb = item.size as f64 / 1024.0;
                        @let tamanho_str = if tamanho_kb > 1024.0 {
                            format!("{:.2} MB", tamanho_kb / 1024.0)
                        } else {
                            format!("{:.1} KB", tamanho_kb)
                        };
                        tr {
                            td { strong { (item.key) } }
                            td { (item.modified) }
                            td { (tamanho_str) }
                            td .direita {
                                a .botao href=(format!("/api/backups/{}", item.key)) target="_blank" download { "Baixar" }
                                " "
                                form method="post" action=(format!("/_/backups/{}/restore", item.key)) style="display:inline;" onsubmit="return confirm('Deseja realmente restaurar este backup? O processo substituirá os dados atuais.');" {
                                    button .botao type="submit" { "Restaurar" }
                                }
                                " "
                                form method="post" action=(format!("/_/backups/{}/delete", item.key)) style="display:inline;" onsubmit="return confirm('Excluir este arquivo de backup?');" {
                                    button .botao.perigo type="submit" { "Excluir" }
                                }
                            }
                        }
                    }
                }
            }
        }
    })
}

/// Tela de Configurações Gerais da Aplicação.
pub fn pagina_settings(
    cfg: &Configuracoes,
    erro: Option<&str>,
    sucesso: Option<&str>,
) -> Markup {
    pagina("Configurações", html! {
        div .cabecalho {
            h1 { "Configurações da Aplicação" }
        }

        @if let Some(m) = erro {
            div .global { (m) }
        }
        @if let Some(s) = sucesso {
            div .sucesso { (s) }
        }

        form method="post" action="/_/settings" {
            // 1. Identidade da Aplicação
            div .secao-config {
                h2 { "Identidade e URL" }
                div .grid-2 {
                    div .campo {
                        label for="appName" { "Nome da Aplicação" }
                        input type="text" id="appName" name="appName" value=(cfg.meta.app_name) required;
                    }
                    div .campo {
                        label for="appUrl" { "URL Pública da Aplicação" }
                        input type="url" id="appUrl" name="appUrl" value=(cfg.meta.app_url) required;
                        p .dica { "Utilizada nos links de confirmação e e-mails." }
                    }
                }
            }

            // 2. Remetente de E-mails
            div .secao-config {
                h2 { "Remetente de E-mails" }
                div .grid-2 {
                    div .campo {
                        label for="senderName" { "Nome do Remetente" }
                        input type="text" id="senderName" name="senderName" value=(cfg.meta.sender_name) required;
                    }
                    div .campo {
                        label for="senderAddress" { "Endereço do Remetente" }
                        input type="email" id="senderAddress" name="senderAddress" value=(cfg.meta.sender_address) required;
                    }
                }
            }

            // 3. Servidor SMTP
            div .secao-config {
                h2 { "Servidor SMTP" }
                div .campo {
                    label {
                        input type="checkbox" name="smtpEnabled" value="true" checked[cfg.smtp.enabled];
                        " Habilitar envio de e-mails via SMTP"
                    }
                }
                div .grid-2 {
                    div .campo {
                        label for="smtpHost" { "Host SMTP" }
                        input type="text" id="smtpHost" name="smtpHost" value=(cfg.smtp.host);
                    }
                    div .campo {
                        label for="smtpPort" { "Porta" }
                        input type="number" id="smtpPort" name="smtpPort" value=(cfg.smtp.port);
                    }
                    div .campo {
                        label for="smtpUsername" { "Usuário" }
                        input type="text" id="smtpUsername" name="smtpUsername" value=(cfg.smtp.username);
                    }
                    div .campo {
                        label for="smtpPassword" { "Senha" }
                        input type="password" id="smtpPassword" name="smtpPassword" value=(cfg.smtp.password);
                    }
                    div .campo {
                        label for="smtpAuthMethod" { "Método de Autenticação" }
                        select id="smtpAuthMethod" name="smtpAuthMethod" {
                            option value="PLAIN" selected[cfg.smtp.auth_method == "PLAIN"] { "PLAIN" }
                            option value="LOGIN" selected[cfg.smtp.auth_method == "LOGIN"] { "LOGIN" }
                        }
                    }
                    div .campo {
                        label for="smtpLocalName" { "Nome Local (EHLO / HELO)" }
                        input type="text" id="smtpLocalName" name="smtpLocalName" value=(cfg.smtp.local_name);
                    }
                }
                div .campo {
                    label {
                        input type="checkbox" name="smtpTls" value="true" checked[cfg.smtp.tls];
                        " Utilizar criptografia TLS / STARTTLS"
                    }
                }
            }

            // 4. Armazenamento S3
            div .secao-config {
                h2 { "Armazenamento em Nuvem (S3 Compatível)" }
                div .campo {
                    label {
                        input type="checkbox" name="s3Enabled" value="true" checked[cfg.s3.enabled];
                        " Habilitar armazenamento em bucket S3 (AWS, MinIO, R2, Wasabi)"
                    }
                }
                div .grid-2 {
                    div .campo {
                        label for="s3Bucket" { "Bucket" }
                        input type="text" id="s3Bucket" name="s3Bucket" value=(cfg.s3.bucket);
                    }
                    div .campo {
                        label for="s3Region" { "Região" }
                        input type="text" id="s3Region" name="s3Region" value=(cfg.s3.region);
                    }
                    div .campo {
                        label for="s3Endpoint" { "Endpoint Customizado (opcional)" }
                        input type="text" id="s3Endpoint" name="s3Endpoint" value=(cfg.s3.endpoint);
                        p .dica { "Ex: http://127.0.0.1:9000 para MinIO ou https://<account>.r2.cloudflarestorage.com" }
                    }
                    div .campo {
                        label for="s3AccessKey" { "Access Key" }
                        input type="text" id="s3AccessKey" name="s3AccessKey" value=(cfg.s3.access_key);
                    }
                    div .campo {
                        label for="s3Secret" { "Secret Key" }
                        input type="password" id="s3Secret" name="s3Secret" value=(cfg.s3.secret);
                    }
                }
                div .campo {
                    label {
                        input type="checkbox" name="s3ForcePathStyle" value="true" checked[cfg.s3.force_path_style];
                        " Forçar Path-Style (obrigatório para MinIO e hosts locais)"
                    }
                }
            }

            // 5. Backups e Retenção
            div .secao-config {
                h2 { "Backups e Retenção de Logs" }
                div .grid-2 {
                    div .campo {
                        label for="backupsCron" { "Agendamento Automático de Backups (Cron)" }
                        input type="text" id="backupsCron" name="backupsCron" value=(cfg.backups.cron);
                        p .dica { "Ex: 0 0 * * * (todo dia à meia-noite). Deixe vazio para desligar." }
                    }
                    div .campo {
                        label for="backupsCronMaxKeep" { "Máximo de Backups Mantidos" }
                        input type="number" id="backupsCronMaxKeep" name="backupsCronMaxKeep" value=(cfg.backups.cron_max_keep) min="1";
                    }
                    div .campo {
                        label for="logsMaxDays" { "Retenção de Logs (dias)" }
                        input type="number" id="logsMaxDays" name="logsMaxDays" value=(cfg.logs.max_days) min="1";
                    }
                }
            }

            div style="margin-top:24px;" {
                button .botao.solido type="submit" { "Salvar alterações" }
            }
        }
    })
}
