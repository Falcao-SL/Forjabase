//! Filtros e ordenação na sintaxe do PocketBase, convertidos em SQL com
//! parâmetros (marcador `?`). O texto do usuário nunca entra no SQL: nomes de campo são
//! conferidos contra a coleção e todo valor vira parâmetro.
//!
//! Suportado nesta versão: `=`, `!=`, `>`, `>=`, `<`, `<=`, `~` (contém),
//! `!~`, `&&`, `||`, parênteses, textos entre aspas simples ou duplas,
//! números, `true`, `false` e `null`. Macros (`@request`, `@collection`,
//! `@now`), operadores "algum item" (`?=`) e modificadores (`:isset`) ainda
//! não (FUNCIONALIDADES §4).

use crate::erro::{Erro, Resultado};

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

/// Valores que as macros de um filtro podem ler. Sem contexto, a macro vale
/// vazio — é o caso dos visitantes, como no PocketBase.
#[derive(Debug, Clone, Default)]
pub struct Contexto {
    /// Registro autenticado, já sem `password` e `tokenKey`.
    pub auth: Option<serde_json::Map<String, serde_json::Value>>,
}

impl Contexto {
    fn resolver(&self, nome: &str) -> Resultado<Literal> {
        let Some(campo) = nome.strip_prefix("@request.auth.") else {
            return Err(Erro::invalido(format!(
                "Invalid filter: macro '{nome}' is not supported yet"
            )));
        };
        let Some(auth) = self.auth.as_ref() else {
            return Ok(Literal::Texto(String::new()));
        };
        Ok(match auth.get(campo) {
            Some(serde_json::Value::String(s)) => Literal::Texto(s.clone()),
            Some(serde_json::Value::Bool(b)) => Literal::Bool(*b),
            Some(serde_json::Value::Number(n)) => Literal::Numero(n.as_f64().unwrap_or(0.0)),
            _ => Literal::Texto(String::new()),
        })
    }
}

struct Escritor<'a> {
    dialeto: Dialeto,
    params: Vec<Literal>,
    valido: &'a dyn Fn(&str) -> bool,
    ctx: &'a Contexto,
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
            Operando::Campo(n) => coluna(n, self.valido),
            Operando::Macro(n) => {
                let l = self.ctx.resolver(n)?;
                Ok(self.marcador(l))
            }
            Operando::Valor(l) => Ok(self.marcador(l.clone())),
        }
    }

    fn expr(&mut self, e: &Expr) -> Resultado<String> {
        Ok(match e {
            Expr::E(a, b) => format!("({} AND {})", self.expr(a)?, self.expr(b)?),
            Expr::Ou(a, b) => format!("({} OR {})", self.expr(a)?, self.expr(b)?),
            Expr::Cmp { esq, op, dir } => self.comparacao(esq, *op, dir)?,
        })
    }

    fn comparacao(&mut self, esq: &Operando, op: Op, dir: &Operando) -> Resultado<String> {
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
        let simbolo = match op {
            Op::Igual => "=",
            Op::Diferente => "!=",
            Op::Maior => ">",
            Op::MaiorIgual => ">=",
            Op::Menor => "<",
            Op::MenorIgual => "<=",
            Op::Contem | Op::NaoContem => unreachable!(),
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
    let e = analisar(filtro)?;
    let mut w = Escritor {
        dialeto,
        params: vec![],
        valido,
        ctx,
    };
    let texto = w.expr(&e)?;
    Ok(Sql {
        texto,
        params: w.params,
    })
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
}
