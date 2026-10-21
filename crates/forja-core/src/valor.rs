//! Valor de banco neutro: o que entra como parâmetro e sai de uma consulta,
//! igual nos três motores. Cada executor converte para o tipo do seu driver.

#[derive(Debug, Clone, PartialEq)]
pub enum Valor {
    Nulo,
    Int(i64),
    Real(f64),
    Texto(String),
    /// O SQLite e o Turso guardam como 0/1; o Postgres, como BOOLEAN.
    Bool(bool),
}

impl Valor {
    pub fn texto(&self) -> Option<&str> {
        match self {
            Valor::Texto(t) => Some(t),
            _ => None,
        }
    }

    /// Texto ou vazio: para colunas de sistema que nunca são nulas.
    pub fn texto_ou_vazio(&self) -> String {
        self.texto().unwrap_or_default().to_string()
    }

    pub fn int(&self) -> i64 {
        match self {
            Valor::Int(i) => *i,
            Valor::Real(x) => *x as i64,
            Valor::Bool(b) => *b as i64,
            Valor::Texto(t) => t.parse().unwrap_or(0),
            Valor::Nulo => 0,
        }
    }

    pub fn bool(&self) -> bool {
        match self {
            Valor::Bool(b) => *b,
            Valor::Int(i) => *i != 0,
            Valor::Real(x) => *x != 0.0,
            Valor::Texto(t) => matches!(t.as_str(), "1" | "true" | "t"),
            Valor::Nulo => false,
        }
    }
}

impl From<&str> for Valor {
    fn from(t: &str) -> Valor {
        Valor::Texto(t.into())
    }
}

impl From<String> for Valor {
    fn from(t: String) -> Valor {
        Valor::Texto(t)
    }
}

impl From<Option<String>> for Valor {
    fn from(t: Option<String>) -> Valor {
        t.map_or(Valor::Nulo, Valor::Texto)
    }
}

impl From<bool> for Valor {
    fn from(b: bool) -> Valor {
        Valor::Bool(b)
    }
}
