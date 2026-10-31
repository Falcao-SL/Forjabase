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
:root {
  --void: #07080a;
  --fundo: #0c0e12;
  --superficie: #11141c;
  --cartao: #11141c;
  --elevado: #161b26;
  --tinta: #f4f6fb;
  --fraco: #7e8b9f;
  --dim: #3d4656;
  --linha: #1f2633;
  --linha-forte: #2e384b;
  --linha-brilhante: #485772;
  --marca: #d4ff00;
  --marca-fundo: #1b2102;
  --marca-hover: #bce600;
  --marca-brilho: rgba(212, 255, 0, 0.4);
  --perigo: #ff3355;
  --perigo-fundo: rgba(255, 51, 85, 0.12);
  --aviso: #ffb800;
  --sucesso: #00ffaa;
  --ciano: #00e5ff;
  --mono: ui-monospace, "JetBrains Mono", "SF Mono", Consolas, monospace;
  --sans: ui-sans-serif, system-ui, -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, "Helvetica Neue", sans-serif;
}
* { box-sizing: border-box; }
body {
  margin: 0;
  font: 14px/1.55 var(--sans);
  color: var(--tinta);
  background-color: var(--void);
  background-image: 
    linear-gradient(to right, rgba(31, 38, 51, 0.25) 1px, transparent 1px),
    linear-gradient(to bottom, rgba(31, 38, 51, 0.25) 1px, transparent 1px);
  background-size: 32px 32px;
  min-height: 100vh;
  -webkit-font-smoothing: antialiased;
}
a { color: var(--marca); text-decoration: none; transition: color 0.15s ease; }
a:hover { color: var(--marca-hover); text-decoration: none; }
.topo {
  display: flex;
  align-items: center;
  gap: 24px;
  padding: 0 24px;
  height: 60px;
  background: rgba(12, 14, 18, 0.95);
  backdrop-filter: blur(12px);
  border-bottom: 1px solid var(--linha);
  position: sticky;
  top: 0;
  z-index: 100;
}
.marca {
  font-weight: 800;
  font-size: 17px;
  color: #ffffff;
  letter-spacing: -0.02em;
  text-transform: uppercase;
  display: flex;
  align-items: center;
  gap: 8px;
}
.marca::before {
  content: "▮";
  color: var(--marca);
  font-size: 18px;
  text-shadow: 0 0 10px var(--marca-brilho);
}
.marca-tag {
  font-family: var(--mono);
  font-size: 10px;
  color: var(--marca);
  background: var(--marca-fundo);
  border: 1px solid rgba(212, 255, 0, 0.3);
  padding: 2px 6px;
  letter-spacing: 0.05em;
}
.topo nav {
  display: flex;
  gap: 6px;
  flex: 1;
}
.topo nav a {
  color: var(--fraco);
  font-size: 12px;
  font-weight: 600;
  font-family: var(--mono);
  text-transform: uppercase;
  letter-spacing: 0.06em;
  padding: 6px 12px;
  border: 1px solid transparent;
  transition: all 0.15s ease;
}
.topo nav a:hover {
  color: var(--tinta);
  border-color: var(--linha-forte);
  background: var(--superficie);
  text-decoration: none;
}
.topo nav a.ativo, .topo nav a:focus-visible {
  color: #000;
  background: var(--marca);
  border-color: var(--marca);
  font-weight: 700;
  box-shadow: 0 0 12px var(--marca-brilho);
}
.telemetria {
  display: flex;
  align-items: center;
  gap: 8px;
  font-family: var(--mono);
  font-size: 11px;
  color: var(--fraco);
  border: 1px solid var(--linha);
  background: var(--superficie);
  padding: 5px 12px;
}
.pulso-verde {
  width: 7px;
  height: 7px;
  background: var(--marca);
  box-shadow: 0 0 8px var(--marca);
  border-radius: 50%;
  display: inline-block;
  animation: pulso 2s infinite ease-in-out;
}
@keyframes pulso {
  0%, 100% { opacity: 1; transform: scale(1); }
  50% { opacity: 0.35; transform: scale(0.8); }
}
.topo form { margin: 0; }
.link {
  background: none;
  border: 0;
  color: var(--fraco);
  cursor: pointer;
  font: inherit;
  font-family: var(--mono);
  font-size: 12px;
  text-transform: uppercase;
  padding: 0;
  transition: color 0.15s ease;
}
.link:hover { color: var(--marca); }
.conteudo { max-width: 1180px; margin: 32px auto; padding: 0 24px; }
.cabecalho {
  display: flex;
  align-items: baseline;
  gap: 16px;
  margin-bottom: 24px;
  padding-bottom: 16px;
  border-bottom: 1px solid var(--linha);
}
.cabecalho h1 {
  font-size: 26px;
  font-weight: 800;
  margin: 0;
  letter-spacing: -0.03em;
  text-transform: uppercase;
  color: #ffffff;
}
.cabecalho .contagem {
  font-family: var(--mono);
  color: var(--marca);
  font-size: 12px;
  letter-spacing: 0.05em;
  background: var(--superficie);
  border: 1px solid var(--linha);
  padding: 3px 8px;
}
.cabecalho .acoes { margin-left: auto; display: flex; gap: 10px; align-items: center; }
.botao {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  gap: 6px;
  padding: 8px 16px;
  border-radius: 0;
  border: 1px solid var(--linha-forte);
  background: var(--superficie);
  color: var(--tinta);
  font-family: var(--mono);
  font-size: 12px;
  font-weight: 600;
  text-transform: uppercase;
  letter-spacing: 0.04em;
  cursor: pointer;
  transition: all 0.15s ease;
  text-decoration: none;
}
.botao:hover {
  border-color: var(--marca);
  color: var(--marca);
  background: var(--elevado);
  text-decoration: none;
  box-shadow: 0 0 12px rgba(212, 255, 0, 0.15);
}
.botao.solido {
  background: var(--marca);
  border-color: var(--marca);
  color: #000000;
  font-weight: 800;
}
.botao.solido:hover {
  background: var(--marca-hover);
  border-color: var(--marca-hover);
  color: #000000;
  box-shadow: 0 0 18px var(--marca-brilho);
  transform: translateY(-1px);
}
.botao.perigo {
  background: transparent;
  border-color: var(--perigo);
  color: var(--perigo);
}
.botao.perigo:hover {
  background: var(--perigo-fundo);
  box-shadow: 0 0 12px rgba(255, 51, 85, 0.25);
}
table {
  width: 100%;
  border-collapse: collapse;
  background: var(--cartao);
  border: 1px solid var(--linha);
  border-radius: 0;
  overflow: hidden;
}
th, td {
  text-align: left;
  padding: 12px 16px;
  border-bottom: 1px solid var(--linha);
  vertical-align: middle;
  font-size: 13px;
}
th {
  background: var(--elevado);
  font-weight: 700;
  font-family: var(--mono);
  font-size: 11px;
  text-transform: uppercase;
  letter-spacing: 0.08em;
  color: var(--fraco);
  border-bottom: 1px solid var(--linha-forte);
}
tbody tr {
  transition: background 0.12s ease;
}
tbody tr:hover {
  background: rgba(212, 255, 0, 0.03);
}
tr:last-child td {
  border-bottom: 0;
}
td.direita, th.direita { text-align: right; white-space: nowrap; }
td code {
  font-family: var(--mono);
  font-size: 12px;
  color: var(--tinta);
  background: rgba(255, 255, 255, 0.05);
  padding: 2px 6px;
  border: 1px solid var(--linha);
}
.etiqueta {
  display: inline-flex;
  align-items: center;
  gap: 4px;
  padding: 2px 8px;
  border-radius: 0;
  background: var(--elevado);
  border: 1px solid var(--linha-forte);
  color: var(--fraco);
  font-family: var(--mono);
  font-size: 11px;
  text-transform: uppercase;
  letter-spacing: 0.05em;
}
.vazio {
  color: var(--fraco);
  background: var(--cartao);
  border: 1px dashed var(--linha-forte);
  border-radius: 0;
  padding: 36px;
  text-align: center;
  font-family: var(--mono);
  font-size: 13px;
}
.busca { margin-bottom: 16px; }
.busca input {
  width: 100%;
  max-width: 440px;
  padding: 10px 14px;
  border: 1px solid var(--linha-forte);
  border-radius: 0;
  font-family: var(--mono);
  font-size: 13px;
  background: var(--superficie);
  color: var(--tinta);
  transition: all 0.15s ease;
}
.busca input:focus {
  outline: none;
  border-color: var(--marca);
  box-shadow: 0 0 0 1px var(--marca), 0 0 12px rgba(212, 255, 0, 0.2);
}
.paginacao {
  display: flex;
  gap: 12px;
  align-items: center;
  margin-top: 18px;
  color: var(--fraco);
  font-family: var(--mono);
  font-size: 12px;
}
.formulario, .secao-config {
  background: var(--cartao);
  border: 1px solid var(--linha);
  border-radius: 0;
  padding: 28px;
  max-width: 680px;
}
.secao-config {
  max-width: 780px;
  margin-bottom: 24px;
}
.secao-config h2 {
  font-size: 16px;
  font-weight: 800;
  text-transform: uppercase;
  letter-spacing: -0.01em;
  color: #ffffff;
  margin: 0 0 18px;
  border-bottom: 1px solid var(--linha);
  padding-bottom: 10px;
}
.campo { margin-bottom: 20px; }
.campo label {
  display: block;
  font-family: var(--mono);
  font-size: 11px;
  font-weight: 600;
  text-transform: uppercase;
  letter-spacing: 0.06em;
  color: var(--fraco);
  margin-bottom: 8px;
}
.campo .obrigatorio { color: var(--marca); margin-left: 3px; }
.campo input, .campo select, .campo textarea, .linha-campo input, .linha-campo select {
  width: 100%;
  padding: 10px 14px;
  border: 1px solid var(--linha-forte);
  border-radius: 0;
  font-family: var(--mono);
  font-size: 13px;
  background: var(--void);
  color: var(--tinta);
  transition: all 0.15s ease;
}
.campo input:focus, .campo select:focus, .campo textarea:focus, .linha-campo input:focus, .linha-campo select:focus {
  outline: none;
  border-color: var(--marca);
  box-shadow: 0 0 0 1px var(--marca), 0 0 12px rgba(212, 255, 0, 0.2);
}
.campo textarea {
  min-height: 120px;
  resize: vertical;
  font-family: var(--mono);
  font-size: 13px;
  line-height: 1.5;
}
.campo .erro {
  color: var(--perigo);
  font-family: var(--mono);
  font-size: 12px;
  margin-top: 6px;
}
.campo .dica {
  color: var(--fraco);
  font-family: var(--mono);
  font-size: 11px;
  margin-top: 6px;
}
.global {
  background: var(--perigo-fundo);
  border: 1px solid var(--perigo);
  color: #ff6b84;
  font-family: var(--mono);
  border-radius: 0;
  padding: 12px 16px;
  margin-bottom: 20px;
  font-size: 13px;
}
.cartao {
  max-width: 420px;
  margin: 12vh auto;
  background: var(--cartao);
  border: 1px solid var(--linha-forte);
  border-radius: 0;
  padding: 36px;
  box-shadow: 0 16px 40px rgba(0, 0, 0, 0.6);
  position: relative;
}
.cartao::before {
  content: "// ACCESS_CONTROL // FORJABASE";
  display: block;
  font-family: var(--mono);
  font-size: 10px;
  color: var(--marca);
  letter-spacing: 0.1em;
  margin-bottom: 16px;
}
.cartao h1 {
  font-size: 24px;
  font-weight: 800;
  text-transform: uppercase;
  letter-spacing: -0.02em;
  margin: 0 0 8px;
  color: #ffffff;
}
.cartao .ajuda {
  color: var(--fraco);
  font-size: 13px;
  margin: 0 0 24px;
}
.cartao .campo input { font-size: 14px; }
.cartao button { width: 100%; margin-top: 12px; padding: 12px; }
.linha-campo { display: flex; gap: 10px; margin-bottom: 10px; align-items: center; }
.linha-campo input, .linha-campo select { padding: 8px 12px; font-size: 13px; }
.zona-perigo {
  margin-top: 36px;
  padding: 24px;
  border: 1px solid var(--perigo);
  border-radius: 0;
  background: rgba(255, 51, 85, 0.04);
  max-width: 680px;
}
.zona-perigo h3 {
  margin: 0 0 8px;
  color: var(--perigo);
  font-size: 16px;
  font-weight: 800;
  text-transform: uppercase;
}
.zona-perigo p { margin: 0 0 14px; font-size: 13px; color: var(--fraco); }
.status-badge {
  display: inline-block;
  padding: 2px 8px;
  border-radius: 0;
  font-family: var(--mono);
  font-weight: 700;
  font-size: 11px;
  letter-spacing: 0.05em;
}
.status-2xx {
  background: rgba(0, 255, 170, 0.10);
  color: var(--sucesso);
  border: 1px solid rgba(0, 255, 170, 0.35);
}
.status-3xx {
  background: rgba(0, 229, 255, 0.10);
  color: var(--ciano);
  border: 1px solid rgba(0, 229, 255, 0.35);
}
.status-4xx {
  background: rgba(255, 184, 0, 0.10);
  color: var(--aviso);
  border: 1px solid rgba(255, 184, 0, 0.35);
}
.status-5xx {
  background: rgba(255, 51, 85, 0.12);
  color: var(--perigo);
  border: 1px solid rgba(255, 51, 85, 0.35);
}
.grafico-barras {
  display: flex;
  gap: 6px;
  align-items: flex-end;
  height: 80px;
  padding: 12px;
  background: var(--void);
  border: 1px solid var(--linha);
  margin-bottom: 24px;
}
.barra-item {
  flex: 1;
  background: var(--linha-forte);
  border-radius: 0;
  min-height: 4px;
  position: relative;
  transition: all 0.2s ease;
}
.barra-item:hover {
  background: var(--marca);
  box-shadow: 0 0 12px var(--marca-brilho);
}
.grid-2 { display: grid; grid-template-columns: 1fr 1fr; gap: 16px; }
.grid-kpi {
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(200px, 1fr));
  gap: 16px;
  margin-bottom: 24px;
}
.cartao-kpi {
  background: var(--cartao);
  border: 1px solid var(--linha);
  padding: 18px 20px;
  position: relative;
  transition: all 0.15s ease;
}
.cartao-kpi:hover {
  border-color: var(--linha-forte);
  box-shadow: 0 0 16px rgba(0, 0, 0, 0.4);
}
.cartao-kpi::after {
  content: "+";
  position: absolute;
  top: 8px;
  right: 12px;
  font-family: var(--mono);
  font-size: 12px;
  color: var(--dim);
}
.kpi-label {
  display: block;
  font-family: var(--mono);
  font-size: 11px;
  color: var(--fraco);
  text-transform: uppercase;
  letter-spacing: 0.08em;
  margin-bottom: 8px;
}
.kpi-valor {
  font-size: 28px;
  font-weight: 800;
  color: #ffffff;
  letter-spacing: -0.02em;
  font-family: var(--mono);
  line-height: 1.1;
  margin-bottom: 4px;
}
.kpi-sub {
  font-family: var(--mono);
  font-size: 11px;
  color: var(--marca);
}
.faixa-perigo {
  height: 6px;
  background: repeating-linear-gradient(
    -45deg,
    var(--perigo),
    var(--perigo) 10px,
    var(--void) 10px,
    var(--void) 20px
  );
  margin-bottom: 16px;
}
.faixa-marca {
  height: 4px;
  background: repeating-linear-gradient(
    -45deg,
    var(--marca),
    var(--marca) 8px,
    var(--void) 8px,
    var(--void) 16px
  );
  margin-bottom: 16px;
}
.tag-secao {
  display: block;
  font-family: var(--mono);
  font-size: 11px;
  font-weight: 700;
  color: var(--marca);
  letter-spacing: 0.1em;
  text-transform: uppercase;
  margin-bottom: 8px;
}
.rodape {
  margin-top: 56px;
  padding: 20px 0 32px;
  border-top: 1px solid var(--linha);
  display: flex;
  justify-content: space-between;
  align-items: center;
  font-family: var(--mono);
  font-size: 11px;
  color: var(--dim);
  text-transform: uppercase;
  letter-spacing: 0.05em;
}
.rodape span.destaque {
  color: var(--marca);
}
.sucesso {
  background: rgba(0, 255, 170, 0.08);
  border: 1px solid var(--sucesso);
  color: #55ffbd;
  font-family: var(--mono);
  border-radius: 0;
  padding: 12px 16px;
  margin-bottom: 20px;
  font-size: 13px;
}
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
                title { "Instalação · Forjabase" }
                style { (PreEscaped(CSS)) }
            }
            body {
                div .cartao {
                    div .faixa-marca {}
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
                        button .botao.solido type="submit" { "Criar e entrar ↗" }
                    }
                }
            }
        }
    }
}

/// Página completa com o cabeçalho do painel.
pub fn pagina(titulo: &str, conteudo: Markup) -> Markup {
    let ativo = |esperado: &str| if titulo == esperado { "ativo" } else { "" };
    html! {
        (DOCTYPE)
        html lang="pt-BR" {
            head {
                meta charset="utf-8";
                meta name="viewport" content="width=device-width, initial-scale=1";
                title { (titulo) " · Forjabase" }
                style { (PreEscaped(CSS)) }
                script src="/_/htmx.min.js" defer {}
            }
            body {
                header .topo {
                    a .marca href="/_/" {
                        "Forja"
                        span .marca-tag { "v0.6" }
                    }
                    nav {
                        a class=(ativo("Coleções")) href="/_/" { "Coleções" }
                        a class=(ativo("Logs")) href="/_/logs" { "Logs" }
                        a class=(ativo("Crons")) href="/_/crons" { "Crons" }
                        a class=(ativo("Backups")) href="/_/backups" { "Backups" }
                        a class=(ativo("Configurações")) href="/_/settings" { "Configurações" }
                    }
                    div .telemetria {
                        span .pulso-verde {}
                        "SYS: ONLINE_"
                    }
                    form method="post" action="/_/logout" {
                        button .botao type="submit" { "Sair ↗" }
                    }
                }
                main .conteudo {
                    (conteudo)
                    footer .rodape {
                        div {
                            "FORJABASE // RUST ENGINE // "
                            span .destaque { "SYS: OPERATIONAL" }
                        }
                        div {
                            "THE FUTURE ISN'T MINIMAL. IT'S SYSTEMATIC. ▮"
                        }
                    }
                }
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
                title { "Entrar · Forjabase" }
                style { (PreEscaped(CSS)) }
            }
            body {
                div .cartao {
                    div .faixa-marca {}
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
                        button .botao.solido type="submit" { "Entrar ↗" }
                    }
                }
            }
        }
    }
}

/// Lista de coleções.
pub fn colecoes(itens: &[Collection]) -> Markup {
    let base_qtd = itens
        .iter()
        .filter(|c| c.kind == CollectionType::Base)
        .count();
    let auth_qtd = itens
        .iter()
        .filter(|c| c.kind == CollectionType::Auth)
        .count();
    let view_qtd = itens
        .iter()
        .filter(|c| c.kind == CollectionType::View)
        .count();
    html! {
        div .cabecalho {
            h1 { "Coleções" }
            span .contagem { (itens.len()) " esquemas" }
            div .acoes {
                a .botao.solido href="/_/new-collection" { "+ Nova coleção ↗" }
            }
        }
        @if !itens.is_empty() {
            div .grid-kpi {
                div .cartao-kpi {
                    span .kpi-label { "// TOTAL COLEÇÕES" }
                    div .kpi-valor { (itens.len()) }
                    span .kpi-sub { "esquemas ativos" }
                }
                div .cartao-kpi {
                    span .kpi-label { "// BASE" }
                    div .kpi-valor { (base_qtd) }
                    span .kpi-sub { "tabelas de dados" }
                }
                div .cartao-kpi {
                    span .kpi-label { "// AUTH" }
                    div .kpi-valor { (auth_qtd) }
                    span .kpi-sub { "usuários & mfa" }
                }
                div .cartao-kpi {
                    span .kpi-label { "// VIEW (SQL)" }
                    div .kpi-valor { (view_qtd) }
                    span .kpi-sub { "consultas somente leitura" }
                }
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
                                a .botao href=(format!("/_/collections/{}", c.name)) { "Abrir ↗" }
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
                a .botao href=(format!("/_/collections/{}/settings", c.name)) { "Configurações ↗" }
                @if c.kind == CollectionType::View {
                    span .etiqueta { "somente leitura" }
                } @else {
                    a .botao.solido href=(format!("/_/collections/{}/new", c.name)) { "Novo ↗" }
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
                  placeholder="> filtrar registros (ex.: nome ~ 'ana')...";
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
                                        code { (id) }
                                    } @else {
                                        a href=(format!("/_/collections/{}/records/{id}", c.name)) { code { (id) } }
                                    }
                                }
                                @for f in &colunas {
                                    td { (valor_curto(r.get(&f.name))) }
                                }
                                td .direita {
                                    @if c.kind != CollectionType::View {
                                        a .botao href=(format!("/_/collections/{}/records/{id}", c.name)) { "Editar ↗" }
                                        " "
                                        button .botao.perigo type="button"
                                            hx-post=(format!("/_/collections/{}/records/{id}/delete", c.name))
                                            hx-confirm="Apagar este registro?"
                                            hx-target="#tabela"
                                            hx-swap="outerHTML" { "Apagar ✕" }
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
                        a .botao href=(url_pagina(c, p, filtro))
                          hx-get=(format!("/_/collections/{}/tabela?page={p}&filter={}", c.name, url_encode(filtro)))
                          hx-target="#tabela"
                          hx-push-url=(url_pagina(c, p, filtro)) { "← anterior" }
                    }
                    span { "página " (lista.page) " de " (lista.total_pages) }
                    @if lista.page < lista.total_pages as u32 {
                        @let p = lista.page + 1;
                        a .botao href=(url_pagina(c, p, filtro))
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
                a .botao href=(format!("/_/collections/{}", c.name)) { "← Voltar" }
            }
        }
        @if let Some(m) = mensagem {
            div .global { (m) }
        }
        form .formulario method="post" action=(acao) {
            span .tag-secao { "// EDITOR DE REGISTRO" }
            @for f in c.fields.iter().filter(|f| editavel(f)) {
                (campo(f, registro, erros, editando))
            }
            div style="margin-top:20px;" {
                button .botao.solido type="submit" { "Salvar registro ↗" }
            }
        }
    }
}

pub fn nao_encontrado(mensagem: &str) -> Markup {
    html! {
        div .cabecalho { h1 { "Não encontrado" } }
        p .vazio { (mensagem) }
        p { a .botao href="/_/" { "← Voltar para as coleções" } }
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
                a .botao href="/_/" { "← Voltar" }
            }
        }
        @if let Some(m) = erro {
            div .global { (m) }
        }
        form .formulario method="post" action="/_/new-collection" {
            span .tag-secao { "// 01 IDENTIFICAÇÃO" }
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
            h2 style="font-size:14px; text-transform:uppercase; font-family:var(--mono); color:var(--marca); margin:28px 0 12px; border-bottom:1px solid var(--linha-forte); padding-bottom:6px;" { "// 02 REGRAS DE ACESSO (PERMISSIONS)" }
            (campo_regra("Listar registros (listRule)", "list_rule", None))
            (campo_regra("Visualizar registro (viewRule)", "view_rule", None))
            (campo_regra("Criar registro (createRule)", "create_rule", None))
            (campo_regra("Atualizar registro (updateRule)", "update_rule", None))
            (campo_regra("Apagar registro (deleteRule)", "delete_rule", None))

            div id="bloco-campos" {
                div style="display:flex; justify-content:space-between; align-items:center; margin:28px 0 12px; border-bottom:1px solid var(--linha-forte); padding-bottom:6px;" {
                    h2 style="font-size:14px; text-transform:uppercase; font-family:var(--mono); color:var(--marca); margin:0;" { "// 03 CAMPOS PERSONALIZADOS" }
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
                button .botao.solido type="submit" { "Criar coleção ↗" }
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
                a .botao href=(format!("/_/collections/{}", c.name)) { "← Voltar para registros" }
            }
        }
        @if let Some(m) = erro {
            div .global { (m) }
        }
        form .formulario method="post" action=(format!("/_/collections/{}/settings", c.name)) {
            span .tag-secao { "// 01 ESQUEMA & DADOS" }
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

            h2 style="font-size:14px; text-transform:uppercase; font-family:var(--mono); color:var(--marca); margin:28px 0 12px; border-bottom:1px solid var(--linha-forte); padding-bottom:6px;" { "// 02 REGRAS DE ACESSO (PERMISSIONS)" }
            (campo_regra("Listar registros (listRule)", "list_rule", c.list_rule.as_deref()))
            (campo_regra("Visualizar registro (viewRule)", "view_rule", c.view_rule.as_deref()))
            @if c.kind != CollectionType::View {
                (campo_regra("Criar registro (createRule)", "create_rule", c.create_rule.as_deref()))
                (campo_regra("Atualizar registro (updateRule)", "update_rule", c.update_rule.as_deref()))
                (campo_regra("Apagar registro (deleteRule)", "delete_rule", c.delete_rule.as_deref()))
            }

            @if c.kind == CollectionType::Auth {
                h2 style="font-size:14px; text-transform:uppercase; font-family:var(--mono); color:var(--marca); margin:28px 0 12px; border-bottom:1px solid var(--linha-forte); padding-bottom:6px;" { "// 03 AUTENTICAÇÃO (OTP, MFA E OAUTH2)" }

                div .campo {
                    label {
                        input type="checkbox" name="otp_enabled" value="true" checked[c.otp_config().enabled];
                        " Habilitar OTP por e-mail (código de uso único)"
                    }
                    div style="display:flex; gap:12px; margin-top:8px;" {
                        div style="flex:1;" {
                            label for="otp_duration" style="font-size:12px;" { "Duração do código (segundos)" }
                            input type="number" id="otp_duration" name="otp_duration" value=(c.otp_config().duration) min="30" max="86400";
                        }
                        div style="flex:1;" {
                            label for="otp_length" style="font-size:12px;" { "Comprimento (dígitos)" }
                            input type="number" id="otp_length" name="otp_length" value=(c.otp_config().length) min="4" max="10";
                        }
                    }
                }

                div .campo {
                    label {
                        input type="checkbox" name="mfa_enabled" value="true" checked[c.mfa_config().enabled];
                        " Habilitar MFA (dois fatores obrigatórios)"
                    }
                    div style="display:flex; gap:12px; margin-top:8px;" {
                        div style="flex:1;" {
                            label for="mfa_duration" style="font-size:12px;" { "Duração da sessão MFA (segundos)" }
                            input type="number" id="mfa_duration" name="mfa_duration" value=(c.mfa_config().duration) min="60" max="86400";
                        }
                        div style="flex:2;" {
                            label for="mfa_rule" style="font-size:12px;" { "Regra de bypass (ex: @request.context != 'oauth2')" }
                            input type="text" id="mfa_rule" name="mfa_rule" value=(c.mfa_config().rule);
                        }
                    }
                }

                div .campo {
                    label {
                        input type="checkbox" name="oauth2_enabled" value="true" checked[c.oauth2_config().enabled];
                        " Habilitar login via OAuth2"
                    }
                }
            }

            @if c.kind != CollectionType::View {
                h2 style="font-size:14px; text-transform:uppercase; font-family:var(--mono); color:var(--marca); margin:28px 0 12px; border-bottom:1px solid var(--linha-forte); padding-bottom:6px;" { "// 04 CAMPOS EXISTENTES" }
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

                div style="display:flex; justify-content:space-between; align-items:center; margin:28px 0 12px; border-bottom:1px solid var(--linha-forte); padding-bottom:6px;" {
                    h2 style="font-size:14px; text-transform:uppercase; font-family:var(--mono); color:var(--marca); margin:0;" { "// 05 ADICIONAR NOVOS CAMPOS" }
                    button .botao type="button"
                           hx-get="/_/htmx/campo-linha"
                           hx-target="#campos"
                           hx-swap="beforeend" { "+ Adicionar campo" }
                }
                div id="campos" {}
            }

            div style="margin-top:24px;" {
                button .botao.solido type="submit" { "Salvar alterações ↗" }
            }
        }

        @if !c.system {
            div .zona-perigo {
                div .faixa-perigo {}
                h3 { "Zona de Perigo" }
                p { "Excluir esta coleção apagará permanentemente a tabela e todos os registros associados." }
                form method="post" action=(format!("/_/collections/{}/delete-collection", c.name)) {
                    button .botao.perigo type="submit"
                           hx-confirm=(format!("Tem certeza que deseja apagar a coleção '{}' e todos os dados? Esta ação não pode ser desfeita.", c.name)) {
                        "Excluir coleção ✕"
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
    let total_logs = itens.len();
    let erros = itens
        .iter()
        .filter(|i| {
            let status = i.data.get("status").and_then(Value::as_u64).unwrap_or(200);
            status >= 400
        })
        .count();
    let sucessos = total_logs.saturating_sub(erros);
    let taxa_sucesso = if total_logs > 0 {
        format!("{:.1}%", (sucessos as f64 / total_logs as f64) * 100.0)
    } else {
        "100.0%".to_string()
    };
    pagina(
        "Logs",
        html! {
            div .cabecalho {
                h1 { "Logs de Requisições" }
                span .contagem { (itens.len()) " eventos recentes" }
            }

            div .grid-kpi {
                div .cartao-kpi {
                    span .kpi-label { "// TOTAL EVENTOS" }
                    div .kpi-valor { (total_logs) }
                    span .kpi-sub { "em buffer recente" }
                }
                div .cartao-kpi {
                    span .kpi-label { "// REQUISIÇÕES 2XX" }
                    div .kpi-valor { (sucessos) }
                    span .kpi-sub style="color:var(--sucesso);" { "respostas com sucesso" }
                }
                div .cartao-kpi {
                    span .kpi-label { "// ALERTAS / ERROS" }
                    div .kpi-valor { (erros) }
                    span .kpi-sub style=(if erros > 0 { "color:var(--perigo);" } else { "color:var(--fraco);" }) {
                        (if erros > 0 { "status 4xx / 5xx" } else { "zero incidentes" })
                    }
                }
                div .cartao-kpi {
                    span .kpi-label { "// TAXA SUCESSO" }
                    div .kpi-valor { (taxa_sucesso) }
                    span .kpi-sub { (format!("pico {} req/h", max_stat)) }
                }
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
                                td { code { (item.created) } }
                                td { span class=(badge_class) { (status) } }
                                td { strong { (method) } }
                                td { code { (url) } }
                                td { code { (format!("{:.2} ms", exec_time * 1000.0)) } }
                                td { span .etiqueta { (item.level) } }
                            }
                        }
                    }
                }
            }
        },
    )
}

/// Tela de Tarefas Agendadas (Crons).
pub fn pagina_crons(itens: &[CronItem]) -> Markup {
    let total = itens.len();
    pagina(
        "Crons",
        html! {
            div .cabecalho {
                h1 { "Tarefas Agendadas" }
                span .contagem { (itens.len()) " agendamentos ativos" }
            }

            @if !itens.is_empty() {
                div .grid-kpi {
                    div .cartao-kpi {
                        span .kpi-label { "// TAREFAS ATIVAS" }
                        div .kpi-valor { (total) }
                        span .kpi-sub { "rotinas agendadas" }
                    }
                    div .cartao-kpi {
                        span .kpi-label { "// MOTOR CRON" }
                        div .kpi-valor style="color:var(--sucesso);" { "RUNNING" }
                        span .kpi-sub { "thread em segundo plano" }
                    }
                    div .cartao-kpi {
                        span .kpi-label { "// PRECISÃO" }
                        div .kpi-valor { "1 SEC" }
                        span .kpi-sub { "varredura de gatilhos" }
                    }
                }
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
                                td { strong { code { (item.id) } } }
                                td { code { (item.expression) } }
                                td .direita {
                                    form method="post" action=(format!("/_/crons/{}/run", item.id)) style="display:inline;" {
                                        button .botao type="submit" { "Executar agora ↗" }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        },
    )
}

/// Tela de Gerenciamento de Backups.
pub fn pagina_backups(itens: &[BackupItem], erro: Option<&str>, sucesso: Option<&str>) -> Markup {
    let total_bytes: u64 = itens.iter().map(|b| b.size).sum();
    let total_mb = total_bytes as f64 / (1024.0 * 1024.0);
    let total_str = if total_mb > 1024.0 {
        format!("{:.2} GB", total_mb / 1024.0)
    } else {
        format!("{:.2} MB", total_mb)
    };
    pagina(
        "Backups",
        html! {
            div .cabecalho {
                h1 { "Backups" }
                span .contagem { (itens.len()) " snapshots salvos" }
                div .acoes {
                    form method="post" action="/_/backups/new" style="margin:0;" {
                        button .botao.solido type="submit" { "+ Criar backup ↗" }
                    }
                }
            }

            @if let Some(m) = erro {
                div .global { (m) }
            }
            @if let Some(s) = sucesso {
                div .sucesso { (s) }
            }

            @if !itens.is_empty() {
                div .grid-kpi {
                    div .cartao-kpi {
                        span .kpi-label { "// TOTAL BACKUPS" }
                        div .kpi-valor { (itens.len()) }
                        span .kpi-sub { "snapshots pontuais" }
                    }
                    div .cartao-kpi {
                        span .kpi-label { "// ESPAÇO OCUPADO" }
                        div .kpi-valor { (total_str) }
                        span .kpi-sub { "armazenamento compactado" }
                    }
                    div .cartao-kpi {
                        span .kpi-label { "// ISOLAMENTO" }
                        div .kpi-valor { "SQLITE" }
                        span .kpi-sub { "cópia consistente wal" }
                    }
                }
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
                                td { strong { code { (item.key) } } }
                                td { code { (item.modified) } }
                                td { (tamanho_str) }
                                td .direita {
                                    a .botao href=(format!("/api/backups/{}", item.key)) target="_blank" download { "Baixar ↓" }
                                    " "
                                    form method="post" action=(format!("/_/backups/{}/restore", item.key)) style="display:inline;" onsubmit="return confirm('Deseja realmente restaurar este backup? O processo substituirá os dados atuais.');" {
                                        button .botao type="submit" { "Restaurar ↺" }
                                    }
                                    " "
                                    form method="post" action=(format!("/_/backups/{}/delete", item.key)) style="display:inline;" onsubmit="return confirm('Excluir este arquivo de backup?');" {
                                        button .botao.perigo type="submit" { "Excluir ✕" }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        },
    )
}

/// Tela de Configurações Gerais da Aplicação.
pub fn pagina_settings(cfg: &Configuracoes, erro: Option<&str>, sucesso: Option<&str>) -> Markup {
    pagina(
        "Configurações",
        html! {
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
                    span .tag-secao { "// 01 IDENTIDADE" }
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
                    span .tag-secao { "// 02 COMUNICAÇÃO" }
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
                    span .tag-secao { "// 03 TRANSPORTE DE CORREIO" }
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
                    span .tag-secao { "// 04 STORAGE DISTRIBUÍDO" }
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
                    span .tag-secao { "// 05 RESILIÊNCIA E ROTINAS" }
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
                    button .botao.solido type="submit" { "Salvar alterações ↗" }
                }
            }
        },
    )
}
