//! Filtros e ordenação na sintaxe do PocketBase, convertidos em SQL com
//! parâmetros (marcador `?`). O texto do usuário nunca entra no SQL: nomes de campo são
//! conferidos contra a coleção e todo valor vira parâmetro.
//!
//! Suportado nesta versão: `=`, `!=`, `>`, `>=`, `<`, `<=`, `~` (contém),
//! `!~`, `&&`, `||`, parênteses, textos entre aspas simples ou duplas,
//! números, `true`, `false` e `null`. Macros: `@request.auth.*` (vira
//! parâmetro) e `@collection.colecao.campo` (vira `EXISTS`, uma linha da
//! outra coleção que satisfaz o `&&`). Operadores "algum item" (`?=`) e
//! modificadores (`:isset`) ainda não (FUNCIONALIDADES §4).

use crate::erro::{Erro, Resultado};
use std::collections::HashMap;

/// Tamanho máximo de um filtro, como no PocketBase.
const MAX_FILTRO: usize = 3500;

#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    E(Box<Expr>, Box<Expr>),
    Ou(Box<Expr>, Box<Expr>),
    Cmp {
        esq: Operando,
        op: Op,
        dir: Operando,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub enum Operando {
    Campo(String),
    /// `@request.auth.*` e companhia: resolvida para um parâmetro.
    Macro(String),
    Valor(Literal),
}

#[derive(Debug, Clone, PartialEq)]
pub enum Literal {
    Texto(String),
    Numero(f64),
    Bool(bool),
    Nulo,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Op {
    Igual,
    Diferente,
    Maior,
    MaiorIgual,
    Menor,
    MenorIgual,
    Contem,
    NaoContem,
    AlgumIgual,
    AlgumDiferente,
    AlgumMaior,
    AlgumMaiorIgual,
    AlgumMenor,
    AlgumMenorIgual,
    AlgumContem,
    AlgumNaoContem,
}

/// Como escrever o SQL: o SQLite usa `?` e `LIKE`; o PostgreSQL, `$1` e `ILIKE`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Dialeto {
    Sqlite,
    Postgres,
}

/// SQL pronto, com os parâmetros na ordem dos marcadores.
#[derive(Debug, Clone, PartialEq)]
pub struct Sql {
    pub texto: String,
    pub params: Vec<Literal>,
}

// ── Análise ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
enum Token {
    Ident(String),
    Lit(Literal),
    Op(Op),
    E,
    Ou,
    Abre,
    Fecha,
}

fn tokens(s: &str) -> Resultado<Vec<Token>> {
    let c: Vec<char> = s.chars().collect();
    let mut i = 0;
    let mut out = vec![];
    let erro = |m: &str| Erro::invalido(format!("Invalid filter: {m}"));
    while i < c.len() {
        let ch = c[i];
        if ch.is_whitespace() {
            i += 1;
            continue;
        }
        let prox = c.get(i + 1).copied();
        match ch {
            '(' => {
                out.push(Token::Abre);
                i += 1;
            }
            ')' => {
                out.push(Token::Fecha);
                i += 1;
            }
            '&' if prox == Some('&') => {
                out.push(Token::E);
                i += 2;
            }
            '|' if prox == Some('|') => {
                out.push(Token::Ou);
                i += 2;
            }
            '=' => {
                out.push(Token::Op(Op::Igual));
                i += 1;
            }
            '!' if prox == Some('=') => {
                out.push(Token::Op(Op::Diferente));
                i += 2;
            }
            '!' if prox == Some('~') => {
                out.push(Token::Op(Op::NaoContem));
                i += 2;
            }
            '~' => {
                out.push(Token::Op(Op::Contem));
                i += 1;
            }
            '?' => {
                let p2 = c.get(i + 2).copied();
                if prox == Some('=') {
                    out.push(Token::Op(Op::AlgumIgual));
                    i += 2;
                } else if prox == Some('~') {
                    out.push(Token::Op(Op::AlgumContem));
                    i += 2;
                } else if prox == Some('!') && p2 == Some('=') {
                    out.push(Token::Op(Op::AlgumDiferente));
                    i += 3;
                } else if prox == Some('!') && p2 == Some('~') {
                    out.push(Token::Op(Op::AlgumNaoContem));
                    i += 3;
                } else if prox == Some('>') {
                    let igual = p2 == Some('=');
                    out.push(Token::Op(if igual {
                        Op::AlgumMaiorIgual
                    } else {
                        Op::AlgumMaior
                    }));
                    i += if igual { 3 } else { 2 };
                } else if prox == Some('<') {
                    let igual = p2 == Some('=');
                    out.push(Token::Op(if igual {
                        Op::AlgumMenorIgual
                    } else {
                        Op::AlgumMenor
                    }));
                    i += if igual { 3 } else { 2 };
                } else {
                    return Err(erro(&format!("unexpected character '{ch}'")));
                }
            }
            '>' | '<' => {
                let igual = prox == Some('=');
                out.push(Token::Op(match (ch, igual) {
                    ('>', true) => Op::MaiorIgual,
                    ('>', false) => Op::Maior,
                    ('<', true) => Op::MenorIgual,
                    _ => Op::Menor,
                }));
                i += if igual { 2 } else { 1 };
            }
            '\'' | '"' => {
                let aspa = ch;
                let mut t = String::new();
                i += 1;
                loop {
                    match c.get(i) {
                        None => return Err(erro("unterminated string")),
                        Some('\\') if c.get(i + 1) == Some(&aspa) => {
                            t.push(aspa);
                            i += 2;
                        }
                        Some(x) if *x == aspa => {
                            i += 1;
                            break;
                        }
                        Some(x) => {
                            t.push(*x);
                            i += 1;
                        }
                    }
                }
                out.push(Token::Lit(Literal::Texto(t)));
            }
            d if d.is_ascii_digit() || (d == '-' && prox.is_some_and(|p| p.is_ascii_digit())) => {
                let ini = i;
                i += 1;
                while i < c.len() && (c[i].is_ascii_digit() || c[i] == '.') {
                    i += 1;
                }
                let txt: String = c[ini..i].iter().collect();
                let n = txt.parse::<f64>().map_err(|_| erro("invalid number"))?;
                out.push(Token::Lit(Literal::Numero(n)));
            }
            a if a.is_ascii_alphabetic() || a == '_' || a == '@' => {
                let ini = i;
                while i < c.len()
                    && (c[i].is_ascii_alphanumeric() || matches!(c[i], '_' | '.' | ':' | '@'))
                {
                    i += 1;
                }
                let palavra: String = c[ini..i].iter().collect();
                out.push(match palavra.as_str() {
                    "true" => Token::Lit(Literal::Bool(true)),
                    "false" => Token::Lit(Literal::Bool(false)),
                    "null" => Token::Lit(Literal::Nulo),
                    _ => Token::Ident(palavra),
                });
            }
            outro => return Err(erro(&format!("unexpected character '{outro}'"))),
        }
    }
    Ok(out)
}

struct Analisador {
    t: Vec<Token>,
    i: usize,
}

impl Analisador {
    fn espiar(&self) -> Option<&Token> {
        self.t.get(self.i)
    }

    fn ou(&mut self) -> Resultado<Expr> {
        let mut e = self.e()?;
        while self.espiar() == Some(&Token::Ou) {
            self.i += 1;
            e = Expr::Ou(Box::new(e), Box::new(self.e()?));
        }
        Ok(e)
    }

    fn e(&mut self) -> Resultado<Expr> {
        let mut e = self.atomo()?;
        while self.espiar() == Some(&Token::E) {
            self.i += 1;
            e = Expr::E(Box::new(e), Box::new(self.atomo()?));
        }
        Ok(e)
    }

    fn atomo(&mut self) -> Resultado<Expr> {
        if self.espiar() == Some(&Token::Abre) {
            self.i += 1;
            let e = self.ou()?;
            if self.espiar() != Some(&Token::Fecha) {
                return Err(Erro::invalido("Invalid filter: missing ')'"));
            }
            self.i += 1;
            return Ok(e);
        }
        let esq = self.operando()?;
        let op = match self.t.get(self.i) {
            Some(Token::Op(o)) => *o,
            _ => return Err(Erro::invalido("Invalid filter: expected an operator")),
        };
        self.i += 1;
        let dir = self.operando()?;
        Ok(Expr::Cmp { esq, op, dir })
    }

    fn operando(&mut self) -> Resultado<Operando> {
        let t = self.t.get(self.i).cloned();
        self.i += 1;
        match t {
            Some(Token::Ident(n)) if n.starts_with('@') => Ok(Operando::Macro(n)),
            Some(Token::Ident(n)) => Ok(Operando::Campo(n)),
            Some(Token::Lit(l)) => Ok(Operando::Valor(l)),
            _ => Err(Erro::invalido(
                "Invalid filter: expected a field or a value",
            )),
        }
    }
}

/// Analisa um filtro.
pub fn analisar(s: &str) -> Resultado<Expr> {
    if s.len() > MAX_FILTRO {
        return Err(Erro::invalido("Invalid filter: too long"));
    }
    let mut a = Analisador {
        t: tokens(s)?,
        i: 0,
    };
    let e = a.ou()?;
    if a.i != a.t.len() {
        return Err(Erro::invalido("Invalid filter: unexpected token"));
    }
    Ok(e)
}

// ── SQL ──────────────────────────────────────────────────────────────────

fn coluna(nome: &str, valido: &dyn Fn(&str) -> bool) -> Resultado<String> {
    if let Some((campo, modif)) = nome.split_once(':') {
        if campo.contains(['@', '.']) {
            return Err(Erro::invalido(format!(
                "Invalid filter: '{nome}' is not supported"
            )));
        }
        if !valido(campo) {
            return Err(Erro::invalido(format!(
                "Invalid filter: unknown field '{campo}'"
            )));
        }
        return match modif {
            "lower" => Ok(format!("LOWER(\"{campo}\")")),
            "length" => Ok(format!("LENGTH(\"{campo}\")")),
            "isset" => Ok(format!("\"{campo}\"")),
            _ => Err(Erro::invalido(format!(
                "Invalid filter: unknown modifier ':{modif}'"
            ))),
        };
    }
    if nome.contains(['@', '.', ':']) {
        return Err(Erro::invalido(format!(
            "Invalid filter: '{nome}' is not supported yet (macros, relations and modifiers come later)"
        )));
    }
    if !valido(nome) {
        return Err(Erro::invalido(format!(
            "Invalid filter: unknown field '{nome}'"
        )));
    }
    Ok(format!("\"{nome}\""))
}

fn civil_a_partir_de_secs(s: i64) -> (i64, u32, u32, u32, u32, u32) {
    let (dias, resto) = (s.div_euclid(86_400), s.rem_euclid(86_400));
    let (h, m, seg) = (resto / 3600, resto % 3600 / 60, resto % 60);
    let z = dias + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let dia = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let mes = (if mp < 10 { mp + 3 } else { mp - 9 }) as u32;
    let ano = yoe + era * 400 + if mes <= 2 { 1 } else { 0 };
    (ano, mes, dia, h as u32, m as u32, seg as u32)
}

fn dias_no_mes(ano: i64, mes: u32) -> u32 {
    match mes {
        4 | 6 | 9 | 11 => 30,
        2 => {
            let bissexto = (ano % 4 == 0 && ano % 100 != 0) || (ano % 400 == 0);
            if bissexto { 29 } else { 28 }
        }
        _ => 31,
    }
}

/// Valores que as macros de um filtro podem ler. Sem contexto, a macro vale
/// vazio — é o caso dos visitantes, como no PocketBase.
#[derive(Debug, Clone, Default)]
pub struct Contexto {
    /// Registro autenticado, já sem `password` e `tokenKey`.
    pub auth: Option<serde_json::Map<String, serde_json::Value>>,
}

impl Contexto {
    fn resolver(&self, nome: &str) -> Resultado<Literal> {
        if let Some(campo) = nome.strip_prefix("@request.auth.") {
            let Some(auth) = self.auth.as_ref() else {
                return Ok(Literal::Texto(String::new()));
            };
            return Ok(match auth.get(campo) {
                Some(serde_json::Value::String(s)) => Literal::Texto(s.clone()),
                Some(serde_json::Value::Bool(b)) => Literal::Bool(*b),
                Some(serde_json::Value::Number(n)) => Literal::Numero(n.as_f64().unwrap_or(0.0)),
                _ => Literal::Texto(String::new()),
            });
        }

        use std::time::{SystemTime, UNIX_EPOCH};
        let d = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default();
        let s = d.as_secs() as i64;
        let ms = d.subsec_millis();
        let (ano, mes, dia, h, m, seg, _, weekday) = crate::tempo_utc();

        match nome {
            "@now" => Ok(Literal::Texto(format!(
                "{ano:04}-{mes:02}-{dia:02} {h:02}:{m:02}:{seg:02}.{ms:03}Z"
            ))),
            "@yesterday" => {
                let (y_ano, y_mes, y_dia, y_h, y_m, y_seg) = civil_a_partir_de_secs(s - 86_400);
                Ok(Literal::Texto(format!(
                    "{y_ano:04}-{y_mes:02}-{y_dia:02} {y_h:02}:{y_m:02}:{y_seg:02}.{ms:03}Z"
                )))
            }
            "@tomorrow" => {
                let (t_ano, t_mes, t_dia, t_h, t_m, t_seg) = civil_a_partir_de_secs(s + 86_400);
                Ok(Literal::Texto(format!(
                    "{t_ano:04}-{t_mes:02}-{t_dia:02} {t_h:02}:{t_m:02}:{t_seg:02}.{ms:03}Z"
                )))
            }
            "@todayStart" => Ok(Literal::Texto(format!(
                "{ano:04}-{mes:02}-{dia:02} 00:00:00.000Z"
            ))),
            "@todayEnd" => Ok(Literal::Texto(format!(
                "{ano:04}-{mes:02}-{dia:02} 23:59:59.999Z"
            ))),
            "@monthStart" => Ok(Literal::Texto(format!(
                "{ano:04}-{mes:02}-01 00:00:00.000Z"
            ))),
            "@monthEnd" => {
                let ld = dias_no_mes(ano, mes);
                Ok(Literal::Texto(format!(
                    "{ano:04}-{mes:02}-{ld:02} 23:59:59.999Z"
                )))
            }
            "@yearStart" => Ok(Literal::Texto(format!("{ano:04}-01-01 00:00:00.000Z"))),
            "@yearEnd" => Ok(Literal::Texto(format!("{ano:04}-12-31 23:59:59.999Z"))),
            "@second" => Ok(Literal::Numero(seg as f64)),
            "@minute" => Ok(Literal::Numero(m as f64)),
            "@hour" => Ok(Literal::Numero(h as f64)),
            "@day" => Ok(Literal::Numero(dia as f64)),
            "@month" => Ok(Literal::Numero(mes as f64)),
            "@year" => Ok(Literal::Numero(ano as f64)),
            "@weekday" => Ok(Literal::Numero(weekday as f64)),
            _ => Err(Erro::invalido(format!(
                "Invalid filter: macro '{nome}' is not supported yet"
            ))),
        }
    }
}

/// Onde `@collection.colecao.campo` procura a outra coleção.
///
/// `externa(colecao, campo)` devolve o nome canônico da tabela, ou erro se a
/// coleção ou o campo não existem.
pub struct Escopo<'a> {
    pub tabela: &'a str,
    pub externa: &'a dyn Fn(&str, &str) -> Resultado<String>,
}

struct Escritor<'a> {
    dialeto: Dialeto,
    params: Vec<Literal>,
    valido: &'a dyn Fn(&str) -> bool,
    ctx: &'a Contexto,
    escopo: Option<&'a Escopo<'a>>,
    /// Dentro de um `EXISTS`, o campo local ganha o nome da tabela.
    qualificar: bool,
    /// `(nome canônico, alias)` da coleção do `EXISTS` em curso.
    alias: Option<(String, String)>,
}

impl Escritor<'_> {
    /// Sempre `?`: o executor do Postgres numera (`$1`, `$2`…) ao enviar, o que
    /// deixa juntar este SQL com outros pedaços sem renumerar.
    fn marcador(&mut self, l: Literal) -> String {
        self.params.push(l);
        "?".into()
    }

    fn operando(&mut self, o: &Operando) -> Resultado<String> {
        match o {
            Operando::Campo(n) => {
                let col = coluna(n, self.valido)?;
                if self.qualificar {
                    let tabela = self.escopo.map(|e| e.tabela).unwrap_or("");
                    if let Some((campo, modif)) = n.split_once(':') {
                        match modif {
                            "lower" => Ok(format!("LOWER(\"{tabela}\".\"{campo}\")")),
                            "length" => Ok(format!("LENGTH(\"{tabela}\".\"{campo}\")")),
                            _ => Ok(format!("\"{tabela}\".{col}")),
                        }
                    } else {
                        Ok(format!("\"{tabela}\".{col}"))
                    }
                } else {
                    Ok(col)
                }
            }
            Operando::Macro(n) if n.starts_with("@collection.") => self.coluna_externa(n),
            Operando::Macro(n) => {
                let l = self.ctx.resolver(n)?;
                Ok(self.marcador(l))
            }
            Operando::Valor(l) => Ok(self.marcador(l.clone())),
        }
    }

    fn coluna_externa(&self, nome: &str) -> Resultado<String> {
        let (citada, campo) = partir_colecao(nome)?;
        let (canon, alias) = self.alias.as_ref().ok_or_else(|| {
            Erro::invalido(format!(
                "Invalid filter: macro '{nome}' is not supported yet"
            ))
        })?;
        let escopo = self.escopo.ok_or_else(|| {
            Erro::invalido("Invalid filter: macro '@collection' is not supported yet")
        })?;
        let resolvido = (escopo.externa)(&citada, &campo)?;
        if !resolvido.eq_ignore_ascii_case(canon) {
            return Err(Erro::invalido(
                "Invalid filter: a comparison cannot reference two collections",
            ));
        }
        Ok(format!("\"{alias}\".\"{campo}\""))
    }

    /// SQL da expressão. Sem `@collection`, o caminho antigo (coluna nua).
    fn compilar(&mut self, e: &Expr) -> Resultado<String> {
        if !tem_colecao(e) {
            return self.expr(e);
        }
        if self.escopo.is_none() {
            return Err(Erro::invalido(
                "Invalid filter: macro '@collection' is not supported yet",
            ));
        }
        match e {
            Expr::Ou(a, b) => Ok(format!("({} OR {})", self.compilar(a)?, self.compilar(b)?)),
            _ => self.conjuncao(e),
        }
    }

    /// Um `&&`: cada coleção vira um `EXISTS` (a mesma linha cumpre todos os
    /// predicados dela). Um `||` dentro quebra o grupo — cada lado tem o seu.
    fn conjuncao(&mut self, e: &Expr) -> Resultado<String> {
        let mut partes = vec![];
        achatar(e, &mut partes);
        enum Slot<'b> {
            Local(&'b Expr),
            Grupo { termos: Vec<&'b Expr> },
        }
        let mut slots: Vec<Slot> = vec![];
        let mut indice: HashMap<String, usize> = HashMap::new();
        for parte in partes {
            if let Some(chave) = self.chave_colecao(parte)? {
                if let Some(&i) = indice.get(&chave) {
                    if let Slot::Grupo { termos } = &mut slots[i] {
                        termos.push(parte);
                    }
                } else {
                    indice.insert(chave, slots.len());
                    slots.push(Slot::Grupo {
                        termos: vec![parte],
                    });
                }
            } else {
                slots.push(Slot::Local(parte));
            }
        }
        let mut sqls = Vec::with_capacity(slots.len());
        for slot in slots {
            match slot {
                Slot::Local(p) => sqls.push(self.compilar(p)?),
                Slot::Grupo { termos } => sqls.push(self.exists(&termos)?),
            }
        }
        if sqls.len() == 1 {
            Ok(sqls.pop().unwrap_or_default())
        } else {
            Ok(format!("({})", sqls.join(" AND ")))
        }
    }

    fn chave_colecao(&self, e: &Expr) -> Resultado<Option<String>> {
        let Expr::Cmp { esq, dir, .. } = e else {
            return Ok(None);
        };
        let a = colecao_do_operando(esq)?;
        let b = colecao_do_operando(dir)?;
        let citada = match (a, b) {
            (None, None) => return Ok(None),
            (Some((c, _)), None) | (None, Some((c, _))) => c,
            (Some((c1, _)), Some((c2, _))) if c1.eq_ignore_ascii_case(&c2) => c1,
            _ => {
                return Err(Erro::invalido(
                    "Invalid filter: a comparison cannot reference two collections",
                ));
            }
        };
        let campo = campo_da_colecao(esq, &citada)
            .or_else(|| campo_da_colecao(dir, &citada))
            .unwrap_or_default();
        let escopo = self.escopo.ok_or_else(|| {
            Erro::invalido("Invalid filter: macro '@collection' is not supported yet")
        })?;
        let canon = (escopo.externa)(&citada, &campo)?;
        Ok(Some(canon.to_ascii_lowercase()))
    }

    fn exists(&mut self, termos: &[&Expr]) -> Resultado<String> {
        let Some(Expr::Cmp { esq, dir, .. }) = termos.first().copied() else {
            return Err(Erro::invalido("Invalid filter: empty @collection group"));
        };
        let (citada, campo) = colecao_do_operando(esq)?
            .or(colecao_do_operando(dir)?)
            .ok_or_else(|| Erro::invalido("Invalid filter: empty @collection group"))?;
        let escopo = self.escopo.ok_or_else(|| {
            Erro::invalido("Invalid filter: macro '@collection' is not supported yet")
        })?;
        let canon = (escopo.externa)(&citada, &campo)?;
        let alias = format!("_c_{canon}");
        let antigo_q = self.qualificar;
        let antigo_a = self.alias.clone();
        self.qualificar = true;
        self.alias = Some((canon.clone(), alias.clone()));
        let mut conds = Vec::with_capacity(termos.len());
        for t in termos {
            let Expr::Cmp { esq, op, dir } = t else {
                return Err(Erro::invalido("Invalid filter: empty @collection group"));
            };
            conds.push(self.comparacao(esq, *op, dir)?);
        }
        self.qualificar = antigo_q;
        self.alias = antigo_a;
        Ok(format!(
            "EXISTS (SELECT 1 FROM \"{canon}\" AS \"{alias}\" WHERE {})",
            conds.join(" AND ")
        ))
    }

    fn expr(&mut self, e: &Expr) -> Resultado<String> {
        Ok(match e {
            Expr::E(a, b) => format!("({} AND {})", self.expr(a)?, self.expr(b)?),
            Expr::Ou(a, b) => format!("({} OR {})", self.expr(a)?, self.expr(b)?),
            Expr::Cmp { esq, op, dir } => self.comparacao(esq, *op, dir)?,
        })
    }

    fn comparacao(&mut self, esq: &Operando, op: Op, dir: &Operando) -> Resultado<String> {
        if let Operando::Campo(n) = esq
            && let Some((campo, "isset")) = n.split_once(':')
        {
            if !(self.valido)(campo) {
                return Err(Erro::invalido(format!(
                    "Invalid filter: unknown field '{campo}'"
                )));
            }
            let bool_val = match dir {
                Operando::Valor(Literal::Bool(b)) => *b,
                Operando::Valor(Literal::Texto(s)) if s == "true" => true,
                Operando::Valor(Literal::Texto(s)) if s == "false" => false,
                _ => {
                    return Err(Erro::invalido(
                        "Invalid filter: ':isset' modifier expects a boolean",
                    ));
                }
            };
            let quer_definido = match op {
                Op::Igual => bool_val,
                Op::Diferente => !bool_val,
                _ => {
                    return Err(Erro::invalido(
                        "Invalid filter: ':isset' modifier only supports '=' and '!='",
                    ));
                }
            };
            let col = if self.qualificar {
                let tabela = self.escopo.map(|e| e.tabela).unwrap_or("");
                format!("\"{tabela}\".\"{campo}\"")
            } else {
                format!("\"{campo}\"")
            };
            return Ok(match self.dialeto {
                Dialeto::Sqlite => {
                    if quer_definido {
                        format!("({col} IS NOT NULL AND {col} != '')")
                    } else {
                        format!("({col} IS NULL OR {col} = '')")
                    }
                }
                Dialeto::Postgres => {
                    if quer_definido {
                        format!("({col} IS NOT NULL AND {col}::text != '')")
                    } else {
                        format!("({col} IS NULL OR {col}::text = '')")
                    }
                }
            });
        }

        // `campo = null` vale para vazio também, como no PocketBase.
        let nulo = |o: &Operando| matches!(o, Operando::Valor(Literal::Nulo));
        if matches!(op, Op::Igual | Op::Diferente) && (nulo(esq) || nulo(dir)) {
            let outro = if nulo(esq) { dir } else { esq };
            let col = self.operando(outro)?;
            // No Postgres a coluna pode ser numérica: compara como texto.
            let vazio = match self.dialeto {
                Dialeto::Sqlite => format!("({col} IS NULL OR {col} = '')"),
                Dialeto::Postgres => format!("({col} IS NULL OR {col}::text = '')"),
            };
            return Ok(if op == Op::Igual {
                vazio
            } else {
                format!("NOT {vazio}")
            });
        }
        if matches!(op, Op::Contem | Op::NaoContem) {
            let col = self.operando(esq)?;
            let padrao = match dir {
                Operando::Valor(Literal::Texto(t)) if t.contains('%') => t.clone(),
                Operando::Valor(Literal::Texto(t)) => {
                    format!(
                        "%{}%",
                        t.replace('\\', "\\\\")
                            .replace('%', "\\%")
                            .replace('_', "\\_")
                    )
                }
                Operando::Valor(Literal::Numero(n)) => format!("%{n}%"),
                _ => return Err(Erro::invalido("Invalid filter: '~' needs a text value")),
            };
            let m = self.marcador(Literal::Texto(padrao));
            let like = match self.dialeto {
                Dialeto::Sqlite => "LIKE",
                Dialeto::Postgres => "ILIKE",
            };
            let nao = if op == Op::NaoContem { "NOT " } else { "" };
            return Ok(format!("{col} {nao}{like} {m} ESCAPE '\\'"));
        }
        if matches!(op, Op::AlgumContem | Op::AlgumNaoContem) {
            let col = self.operando(esq)?;
            let padrao = match dir {
                Operando::Valor(Literal::Texto(t)) if t.contains('%') => t.clone(),
                Operando::Valor(Literal::Texto(t)) => {
                    format!(
                        "%{}%",
                        t.replace('\\', "\\\\")
                            .replace('%', "\\%")
                            .replace('_', "\\_")
                    )
                }
                Operando::Valor(Literal::Numero(n)) => format!("%{n}%"),
                _ => return Err(Erro::invalido("Invalid filter: '?~' needs a text value")),
            };
            let like = match self.dialeto {
                Dialeto::Sqlite => "LIKE",
                Dialeto::Postgres => "ILIKE",
            };
            let nao = if op == Op::AlgumNaoContem { "NOT " } else { "" };
            return Ok(match self.dialeto {
                Dialeto::Sqlite => {
                    let m1 = self.marcador(Literal::Texto(padrao.clone()));
                    let m2 = self.marcador(Literal::Texto(padrao));
                    format!(
                        "(CASE WHEN json_valid({col}) THEN EXISTS (SELECT 1 FROM json_each({col}) WHERE json_each.value {nao}{like} {m1} ESCAPE '\\') ELSE {col} {nao}{like} {m2} ESCAPE '\\' END)"
                    )
                }
                Dialeto::Postgres => {
                    let m = self.marcador(Literal::Texto(padrao));
                    format!("{col}::text {nao}{like} {m} ESCAPE '\\'")
                }
            });
        }
        if matches!(
            op,
            Op::AlgumIgual
                | Op::AlgumDiferente
                | Op::AlgumMaior
                | Op::AlgumMaiorIgual
                | Op::AlgumMenor
                | Op::AlgumMenorIgual
        ) {
            let simbolo = match op {
                Op::AlgumIgual => "=",
                Op::AlgumDiferente => "!=",
                Op::AlgumMaior => ">",
                Op::AlgumMaiorIgual => ">=",
                Op::AlgumMenor => "<",
                Op::AlgumMenorIgual => "<=",
                _ => unreachable!(),
            };
            let col = self.operando(esq)?;
            return Ok(match self.dialeto {
                Dialeto::Sqlite => {
                    let val1 = self.operando(dir)?;
                    let val2 = self.operando(dir)?;
                    format!(
                        "(CASE WHEN json_valid({col}) THEN EXISTS (SELECT 1 FROM json_each({col}) WHERE json_each.value {simbolo} {val1}) ELSE {col} {simbolo} {val2} END)"
                    )
                }
                Dialeto::Postgres => {
                    let val = self.operando(dir)?;
                    format!("{col}::text {simbolo} {val}")
                }
            });
        }
        let simbolo = match op {
            Op::Igual => "=",
            Op::Diferente => "!=",
            Op::Maior => ">",
            Op::MaiorIgual => ">=",
            Op::Menor => "<",
            Op::MenorIgual => "<=",
            _ => unreachable!(),
        };
        Ok(format!(
            "{} {simbolo} {}",
            self.operando(esq)?,
            self.operando(dir)?
        ))
    }
}

/// Converte um filtro em SQL. `valido` diz quais nomes de campo existem.
pub fn para_sql(filtro: &str, valido: &dyn Fn(&str) -> bool, dialeto: Dialeto) -> Resultado<Sql> {
    para_sql_com(filtro, valido, dialeto, &Contexto::default())
}

/// Como `para_sql`, resolvendo as macros contra o contexto da requisição.
pub fn para_sql_com(
    filtro: &str,
    valido: &dyn Fn(&str) -> bool,
    dialeto: Dialeto,
    ctx: &Contexto,
) -> Resultado<Sql> {
    compilar(filtro, valido, dialeto, ctx, None)
}

/// Como `para_sql_com`, e `@collection.colecao.campo` vira `EXISTS`.
pub fn para_sql_escopo(
    filtro: &str,
    valido: &dyn Fn(&str) -> bool,
    dialeto: Dialeto,
    ctx: &Contexto,
    escopo: &Escopo<'_>,
) -> Resultado<Sql> {
    compilar(filtro, valido, dialeto, ctx, Some(escopo))
}

fn compilar(
    filtro: &str,
    valido: &dyn Fn(&str) -> bool,
    dialeto: Dialeto,
    ctx: &Contexto,
    escopo: Option<&Escopo<'_>>,
) -> Resultado<Sql> {
    let e = analisar(filtro)?;
    let mut w = Escritor {
        dialeto,
        params: vec![],
        valido,
        ctx,
        escopo,
        qualificar: false,
        alias: None,
    };
    let texto = w.compilar(&e)?;
    Ok(Sql {
        texto,
        params: w.params,
    })
}

/// `@request.` e `@collection.` no filtro do cliente: só o superusuário.
pub fn macro_restrita(filtro: &str) -> bool {
    filtro.contains("@request.") || filtro.contains("@collection.")
}

/// Nomes de coleção citados em `@collection.nome.campo`, sem repetir.
pub fn colecoes_citadas(filtro: &str) -> Vec<String> {
    let mut out = vec![];
    let mut resto = filtro;
    while let Some(i) = resto.find("@collection.") {
        resto = &resto[i + "@collection.".len()..];
        let nome: String = resto
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
            .collect();
        if !nome.is_empty() && !out.iter().any(|n: &String| n.eq_ignore_ascii_case(&nome)) {
            out.push(nome);
        }
    }
    out
}

fn tem_colecao(e: &Expr) -> bool {
    match e {
        Expr::E(a, b) | Expr::Ou(a, b) => tem_colecao(a) || tem_colecao(b),
        Expr::Cmp { esq, dir, .. } => eh_colecao(esq) || eh_colecao(dir),
    }
}

fn eh_colecao(o: &Operando) -> bool {
    matches!(o, Operando::Macro(n) if n.starts_with("@collection."))
}

fn achatar<'a>(e: &'a Expr, out: &mut Vec<&'a Expr>) {
    if let Expr::E(a, b) = e {
        achatar(a, out);
        achatar(b, out);
    } else {
        out.push(e);
    }
}

/// `(colecao, campo)` se o operando é `@collection.colecao.campo`.
fn colecao_do_operando(o: &Operando) -> Resultado<Option<(String, String)>> {
    let Operando::Macro(n) = o else {
        return Ok(None);
    };
    if !n.starts_with("@collection.") {
        return Ok(None);
    }
    let (colecao, campo) = partir_colecao(n)?;
    Ok(Some((colecao, campo)))
}

fn partir_colecao(nome: &str) -> Resultado<(String, String)> {
    let resto = nome.strip_prefix("@collection.").unwrap_or("");
    let mut p = resto.split('.');
    let colecao = p.next().unwrap_or("");
    let campo = p.next().unwrap_or("");
    if colecao.is_empty()
        || campo.is_empty()
        || p.next().is_some()
        || !identificador(colecao)
        || !identificador(campo)
    {
        return Err(Erro::invalido(format!(
            "Invalid filter: '{nome}' must be @collection.name.field"
        )));
    }
    Ok((colecao.to_string(), campo.to_string()))
}

fn campo_da_colecao(o: &Operando, colecao: &str) -> Option<String> {
    let (c, campo) = colecao_do_operando(o).ok().flatten()?;
    c.eq_ignore_ascii_case(colecao).then_some(campo)
}

fn identificador(n: &str) -> bool {
    let mut cs = n.chars();
    matches!(cs.next(), Some(c) if c.is_ascii_alphabetic() || c == '_')
        && cs.all(|c| c.is_ascii_alphanumeric() || c == '_')
        && n.len() <= 100
}

/// `sort` do PocketBase (`-created,nome`, `@random`) em `ORDER BY`.
pub fn ordem(sort: &str, valido: &dyn Fn(&str) -> bool, dialeto: Dialeto) -> Resultado<String> {
    let mut partes = vec![];
    for item in sort.split(',').map(str::trim).filter(|s| !s.is_empty()) {
        let (desc, nome) = match item.strip_prefix('-') {
            Some(n) => (true, n),
            None => (false, item.strip_prefix('+').unwrap_or(item)),
        };
        if nome == "@random" {
            partes.push(match dialeto {
                Dialeto::Sqlite => "RANDOM()".to_string(),
                Dialeto::Postgres => "random()".to_string(),
            });
            continue;
        }
        let col = coluna(nome, valido)
            .map_err(|_| Erro::invalido(format!("Invalid sort field '{nome}'")))?;
        partes.push(format!("{col} {}", if desc { "DESC" } else { "ASC" }));
    }
    Ok(partes.join(", "))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn campos(n: &str) -> bool {
        matches!(n, "id" | "nome" | "preco" | "ativo" | "created")
    }

    fn sql(f: &str) -> Sql {
        para_sql(f, &campos, Dialeto::Sqlite).unwrap()
    }

    #[test]
    fn comparacoes_e_precedencia() {
        let s = sql("preco < 50 && nome ~ 'café' || ativo = true");
        assert_eq!(
            s.texto,
            "((\"preco\" < ? AND \"nome\" LIKE ? ESCAPE '\\') OR \"ativo\" = ?)"
        );
        assert_eq!(
            s.params,
            vec![
                Literal::Numero(50.0),
                Literal::Texto("%café%".into()),
                Literal::Bool(true)
            ]
        );
    }

    #[test]
    fn parenteses() {
        let s = sql("(preco >= 1 || preco <= 0) && nome != \"x\"");
        assert_eq!(
            s.texto,
            "((\"preco\" >= ? OR \"preco\" <= ?) AND \"nome\" != ?)"
        );
    }

    #[test]
    fn nulo_vale_para_vazio() {
        assert_eq!(
            sql("nome = null").texto,
            "(\"nome\" IS NULL OR \"nome\" = '')"
        );
        assert_eq!(
            sql("nome != null").texto,
            "NOT (\"nome\" IS NULL OR \"nome\" = '')"
        );
    }

    #[test]
    fn contem_escapa_curingas_e_respeita_porcento_explicito() {
        assert_eq!(
            sql("nome ~ '50%_off'").params,
            vec![Literal::Texto("50%_off".into())]
        );
        assert_eq!(
            sql("nome ~ 'a_b'").params,
            vec![Literal::Texto("%a\\_b%".into())]
        );
        assert!(sql("nome !~ 'x'").texto.contains("NOT LIKE"));
    }

    #[test]
    fn postgres_usa_ilike_e_compara_vazio_como_texto() {
        let s = para_sql("preco > 1 && nome ~ 'a'", &campos, Dialeto::Postgres).unwrap();
        assert_eq!(s.texto, "(\"preco\" > ? AND \"nome\" ILIKE ? ESCAPE '\\')");
        let s = para_sql("preco = null", &campos, Dialeto::Postgres).unwrap();
        assert_eq!(s.texto, "(\"preco\" IS NULL OR \"preco\"::text = '')");
    }

    #[test]
    fn recusa_injecao_e_campos_desconhecidos() {
        assert!(para_sql("senha = 'x'", &campos, Dialeto::Sqlite).is_err());
        assert!(para_sql("nome = 'x'; DROP TABLE t", &campos, Dialeto::Sqlite).is_err());
        assert!(para_sql("nome = 'x' OR 1=1", &campos, Dialeto::Sqlite).is_err());
        assert!(para_sql("@request.body.id = 'x'", &campos, Dialeto::Sqlite).is_err());
        assert!(para_sql("nome = 'sem fim", &campos, Dialeto::Sqlite).is_err());
        assert!(para_sql("(nome = 'x'", &campos, Dialeto::Sqlite).is_err());
        // Um valor com aspas e SQL dentro é só um parâmetro.
        let s = sql("nome = \"x' OR '1'='1\"");
        assert_eq!(s.params, vec![Literal::Texto("x' OR '1'='1".into())]);
    }

    #[test]
    fn macros_do_pedido() {
        // Visitante: a macro vira vazio e nunca entra no SQL como texto.
        let s = sql("@request.auth.id != ''");
        assert_eq!(s.texto, "? != ?");
        assert_eq!(
            s.params,
            vec![Literal::Texto(String::new()), Literal::Texto(String::new())]
        );

        let registro = serde_json::json!({"id": "abc123", "owner": "u1", "verified": true});
        let mapa = registro.as_object().unwrap();
        let ctx = Contexto {
            auth: Some(mapa.clone()),
        };
        let s = para_sql_com("@request.auth.id = nome", &campos, Dialeto::Sqlite, &ctx).unwrap();
        assert_eq!(s.texto, "? = \"nome\"");
        assert_eq!(s.params, vec![Literal::Texto("abc123".into())]);

        let s = para_sql_com(
            "@request.auth.verified = true",
            &campos,
            Dialeto::Sqlite,
            &ctx,
        )
        .unwrap();
        assert_eq!(s.params[0], Literal::Bool(true));
    }

    #[test]
    fn ordenacao() {
        assert_eq!(
            ordem("-created,nome", &campos, Dialeto::Sqlite).unwrap(),
            "\"created\" DESC, \"nome\" ASC"
        );
        assert_eq!(
            ordem("@random", &campos, Dialeto::Sqlite).unwrap(),
            "RANDOM()"
        );
        assert!(ordem("nome; DROP", &campos, Dialeto::Sqlite).is_err());
    }

    fn externas(colecao: &str, campo: &str) -> Resultado<String> {
        let ok = match colecao {
            "comentarios" => matches!(campo, "post" | "status" | "id"),
            "tags" => matches!(campo, "nome" | "post"),
            _ => false,
        };
        if !matches!(colecao, "comentarios" | "tags") {
            return Err(Erro::invalido(format!(
                "Invalid filter: unknown collection '{colecao}'"
            )));
        }
        if !ok {
            return Err(Erro::invalido(format!(
                "Invalid filter: unknown field '{campo}'"
            )));
        }
        Ok(colecao.into())
    }

    fn escopo() -> Escopo<'static> {
        Escopo {
            tabela: "posts",
            externa: &externas,
        }
    }

    #[test]
    fn collection_vira_exists_na_mesma_linha() {
        let s = para_sql_escopo(
            "@collection.comentarios.post = id && @collection.comentarios.status = \"ok\"",
            &campos,
            Dialeto::Sqlite,
            &Contexto::default(),
            &escopo(),
        )
        .unwrap();
        assert_eq!(
            s.texto,
            "EXISTS (SELECT 1 FROM \"comentarios\" AS \"_c_comentarios\" WHERE \"_c_comentarios\".\"post\" = \"posts\".\"id\" AND \"_c_comentarios\".\"status\" = ?)"
        );
        assert_eq!(s.params, vec![Literal::Texto("ok".into())]);
    }

    #[test]
    fn collection_no_ou_nao_exige_a_outra_colecao() {
        let s = para_sql_escopo(
            "nome = \"publico\" || (@collection.comentarios.post = id && @collection.comentarios.status = \"ok\")",
            &campos,
            Dialeto::Sqlite,
            &Contexto::default(),
            &escopo(),
        )
        .unwrap();
        assert_eq!(
            s.texto,
            "(\"nome\" = ? OR EXISTS (SELECT 1 FROM \"comentarios\" AS \"_c_comentarios\" WHERE \"_c_comentarios\".\"post\" = \"posts\".\"id\" AND \"_c_comentarios\".\"status\" = ?))"
        );
        assert_eq!(
            s.params,
            vec![
                Literal::Texto("publico".into()),
                Literal::Texto("ok".into())
            ]
        );
    }

    #[test]
    fn duas_colecoes_sao_dois_exists() {
        let s = para_sql_escopo(
            "@collection.tags.nome = \"rust\" && @collection.tags.post = id && @collection.comentarios.post = id",
            &campos,
            Dialeto::Sqlite,
            &Contexto::default(),
            &escopo(),
        )
        .unwrap();
        assert!(s.texto.contains("FROM \"tags\""));
        assert!(s.texto.contains("FROM \"comentarios\""));
        assert_eq!(s.texto.matches("EXISTS").count(), 2);
    }

    #[test]
    fn collection_desconhecida_falha() {
        let e = para_sql_escopo(
            "@collection.ausente.id = id",
            &campos,
            Dialeto::Sqlite,
            &Contexto::default(),
            &escopo(),
        );
        assert!(e.is_err());
        assert!(para_sql("@collection.comentarios.id = id", &campos, Dialeto::Sqlite).is_err());
    }

    #[test]
    fn operadores_algum_item_para_arrays_e_escalares() {
        let campos_ext = |c: &str| matches!(c, "id" | "nome" | "tags" | "idade");
        let s = para_sql("tags ?= 'rust'", &campos_ext, Dialeto::Sqlite).unwrap();
        assert!(s.texto.contains("json_valid(\"tags\")"));
        assert!(s.texto.contains("json_each.value = ?"));
        assert_eq!(
            s.params,
            vec![Literal::Texto("rust".into()), Literal::Texto("rust".into())]
        );

        let s_contem = para_sql("tags ?~ 'rus'", &campos_ext, Dialeto::Sqlite).unwrap();
        assert!(s_contem.texto.contains("json_each.value LIKE ?"));
        assert_eq!(
            s_contem.params,
            vec![
                Literal::Texto("%rus%".into()),
                Literal::Texto("%rus%".into())
            ]
        );

        let s_maior = para_sql("idade ?> 18", &campos_ext, Dialeto::Sqlite).unwrap();
        assert!(s_maior.texto.contains("json_each.value > ?"));
        assert_eq!(
            s_maior.params,
            vec![Literal::Numero(18.0), Literal::Numero(18.0)]
        );
    }

    #[test]
    fn macros_de_data_e_modificadores() {
        // 1. Modificadores :lower e :length
        let s_lower = sql("nome:lower = 'ana'");
        assert_eq!(s_lower.texto, "LOWER(\"nome\") = ?");
        assert_eq!(s_lower.params, vec![Literal::Texto("ana".into())]);

        let s_len = sql("nome:length > 5");
        assert_eq!(s_len.texto, "LENGTH(\"nome\") > ?");
        assert_eq!(s_len.params, vec![Literal::Numero(5.0)]);

        // 2. Modificador :isset
        let s_isset_t = sql("nome:isset = true");
        assert_eq!(s_isset_t.texto, "(\"nome\" IS NOT NULL AND \"nome\" != '')");

        let s_isset_f = sql("nome:isset = false");
        assert_eq!(s_isset_f.texto, "(\"nome\" IS NULL OR \"nome\" = '')");

        let s_isset_diff = sql("nome:isset != true");
        assert_eq!(s_isset_diff.texto, "(\"nome\" IS NULL OR \"nome\" = '')");

        // 3. Macros de data
        let s_now = sql("nome >= @now");
        assert_eq!(s_now.texto, "\"nome\" >= ?");
        assert!(matches!(s_now.params.first(), Some(Literal::Texto(s)) if s.ends_with('Z')));

        let s_start = sql("nome >= @todayStart");
        assert_eq!(s_start.texto, "\"nome\" >= ?");
        assert!(
            matches!(s_start.params.first(), Some(Literal::Texto(s)) if s.ends_with(" 00:00:00.000Z"))
        );

        let s_end = sql("nome <= @todayEnd");
        assert_eq!(s_end.texto, "\"nome\" <= ?");
        assert!(
            matches!(s_end.params.first(), Some(Literal::Texto(s)) if s.ends_with(" 23:59:59.999Z"))
        );

        let s_mstart = sql("nome >= @monthStart");
        assert!(
            matches!(s_mstart.params.first(), Some(Literal::Texto(s)) if s.ends_with("-01 00:00:00.000Z"))
        );

        let s_mend = sql("nome <= @monthEnd");
        assert!(
            matches!(s_mend.params.first(), Some(Literal::Texto(s)) if s.ends_with(" 23:59:59.999Z"))
        );

        let s_yr = sql("@year >= 2026");
        assert_eq!(s_yr.texto, "? >= ?");
        assert_eq!(
            s_yr.params,
            vec![Literal::Numero(2026.0), Literal::Numero(2026.0)]
        );
    }
}
