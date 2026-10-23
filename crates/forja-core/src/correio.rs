//! Envio de e-mail atrás de um trait.
//!
//! O padrão é o `Console`: a mensagem vai para o stderr, com o link completo —
//! suficiente para desenvolver e testar sem servidor de e-mail. O SMTP entra
//! depois, implementando o mesmo trait, sem mudar quem chama.

use std::sync::{Arc, Mutex};

#[derive(Debug, Clone, PartialEq)]
pub struct Mensagem {
    pub para: String,
    pub assunto: String,
    pub corpo: String,
}

pub trait Correio: Send + Sync {
    fn enviar(&self, mensagem: &Mensagem);
}

/// Escreve no stderr. É o padrão enquanto não há SMTP configurado.
#[derive(Debug, Default)]
pub struct Console;

impl Correio for Console {
    fn enviar(&self, m: &Mensagem) {
        eprintln!(
            "[email] para: {}\n[email] assunto: {}\n[email] corpo: {}",
            m.para, m.assunto, m.corpo
        );
    }
}

/// Guarda as mensagens em memória. Usado nos testes para ler o link.
#[derive(Debug, Default, Clone)]
pub struct Memoria(pub Arc<Mutex<Vec<Mensagem>>>);

impl Memoria {
    pub fn ultima(&self) -> Option<Mensagem> {
        self.0.lock().ok()?.last().cloned()
    }

    pub fn limpar(&self) {
        if let Ok(mut v) = self.0.lock() {
            v.clear();
        }
    }
}

impl Correio for Memoria {
    fn enviar(&self, m: &Mensagem) {
        if let Ok(mut v) = self.0.lock() {
            v.push(m.clone());
        }
    }
}
