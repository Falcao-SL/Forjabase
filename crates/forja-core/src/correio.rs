//! Envio de e-mail e configurações de SMTP e modelos.
//!
//! O padrão é o `Console`: a mensagem vai para o stderr, com o link completo —
//! suficiente para desenvolver e testar sem servidor de e-mail.
//! Quando `SmtpConfig::enabled` é verdadeiro, `SmtpCorreio` entrega via SMTP
//! com suporte a TLS, STARTTLS e autenticação PLAIN / LOGIN.

use crate::Erro;
use lettre::transport::smtp::authentication::{Credentials, Mechanism};
use lettre::transport::smtp::client::Tls;
use lettre::transport::smtp::extension::ClientId;
use lettre::{AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// Mensagem de e-mail a ser enviada.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Mensagem {
    pub para: String,
    pub assunto: String,
    pub corpo: String,
    #[serde(default)]
    pub de_nome: Option<String>,
    #[serde(default)]
    pub de_email: Option<String>,
}

impl Mensagem {
    pub fn nova(para: impl Into<String>, assunto: impl Into<String>, corpo: impl Into<String>) -> Self {
        Self {
            para: para.into(),
            assunto: assunto.into(),
            corpo: corpo.into(),
            de_nome: None,
            de_email: None,
        }
    }

    pub fn com_remetente(
        para: impl Into<String>,
        assunto: impl Into<String>,
        corpo: impl Into<String>,
        de_nome: Option<String>,
        de_email: Option<String>,
    ) -> Self {
        Self {
            para: para.into(),
            assunto: assunto.into(),
            corpo: corpo.into(),
            de_nome,
            de_email,
        }
    }
}

/// Modelo editável de e-mail (verificação, redefinição de senha, troca de e-mail).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EmailTemplate {
    #[serde(default)]
    pub subject: String,
    #[serde(default)]
    pub body: String,
    #[serde(default)]
    pub action_url: String,
}

impl EmailTemplate {
    pub fn novo(
        subject: impl Into<String>,
        body: impl Into<String>,
        action_url: impl Into<String>,
    ) -> Self {
        Self {
            subject: subject.into(),
            body: body.into(),
            action_url: action_url.into(),
        }
    }

    /// Substitui `{APP_NAME}`, `{APP_URL}`, `{TOKEN}`, `{ACTION_URL}` e `{RECORD:campo}`.
    pub fn renderizar(
        &self,
        app_name: &str,
        app_url: &str,
        token: &str,
        campos_extras: &HashMap<String, String>,
    ) -> (String, String) {
        let url_base = app_url.trim_end_matches('/');
        let mut action_url = self.action_url.clone();
        action_url = action_url.replace("{TOKEN}", token);
        action_url = action_url.replace("{APP_NAME}", app_name);
        action_url = action_url.replace("{APP_URL}", url_base);

        let aplicar = |texto: &str| -> String {
            let mut s = texto.to_string();
            s = s.replace("{APP_NAME}", app_name);
            s = s.replace("{APP_URL}", url_base);
            s = s.replace("{TOKEN}", token);
            s = s.replace("{ACTION_URL}", &action_url);
            for (k, v) in campos_extras {
                s = s.replace(&format!("{{RECORD:{k}}}"), v);
            }
            s
        };

        (aplicar(&self.subject), aplicar(&self.body))
    }
}

/// Configurações do servidor SMTP, compatíveis com o PocketBase.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SmtpConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "SmtpConfig::host_padrao")]
    pub host: String,
    #[serde(default = "SmtpConfig::port_padrao")]
    pub port: u16,
    #[serde(default)]
    pub username: String,
    #[serde(default)]
    pub password: String,
    #[serde(default = "SmtpConfig::auth_method_padrao")]
    pub auth_method: String,
    #[serde(default = "SmtpConfig::tls_padrao")]
    pub tls: bool,
    #[serde(default)]
    pub local_name: String,
}

impl SmtpConfig {
    pub fn host_padrao() -> String {
        "smtp.example.com".into()
    }
    pub fn port_padrao() -> u16 {
        587
    }
    pub fn auth_method_padrao() -> String {
        "PLAIN".into()
    }
    pub fn tls_padrao() -> bool {
        true
    }
}

impl Default for SmtpConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            host: Self::host_padrao(),
            port: Self::port_padrao(),
            username: String::new(),
            password: String::new(),
            auth_method: Self::auth_method_padrao(),
            tls: Self::tls_padrao(),
            local_name: String::new(),
        }
    }
}

/// Trait para despachar e-mails de forma assíncrona.
#[async_trait::async_trait]
pub trait Correio: Send + Sync {
    async fn enviar(&self, mensagem: &Mensagem) -> Result<(), Erro>;
}

/// Escreve no stderr. É o padrão enquanto não há SMTP habilitado.
#[derive(Debug, Default)]
pub struct Console;

#[async_trait::async_trait]
impl Correio for Console {
    async fn enviar(&self, m: &Mensagem) -> Result<(), Erro> {
        eprintln!(
            "[email] de: {:?} <{:?}> | para: {}\n[email] assunto: {}\n[email] corpo: {}",
            m.de_nome, m.de_email, m.para, m.assunto, m.corpo
        );
        Ok(())
    }
}

/// Guarda as mensagens em memória. Usado nos testes para inspecionar e-mails disparados.
#[derive(Debug, Default, Clone)]
pub struct Memoria(pub Arc<Mutex<Vec<Mensagem>>>);

impl Memoria {
    pub fn ultima(&self) -> Option<Mensagem> {
        self.0.lock().ok()?.last().cloned()
    }

    pub fn todas(&self) -> Vec<Mensagem> {
        self.0.lock().ok().map(|v| v.clone()).unwrap_or_default()
    }

    pub fn limpar(&self) {
        if let Ok(mut v) = self.0.lock() {
            v.clear();
        }
    }
}

#[async_trait::async_trait]
impl Correio for Memoria {
    async fn enviar(&self, m: &Mensagem) -> Result<(), Erro> {
        if let Ok(mut v) = self.0.lock() {
            v.push(m.clone());
        }
        Ok(())
    }
}

/// Cliente SMTP que envia e-mails através da biblioteca `lettre`.
#[derive(Debug, Clone)]
pub struct SmtpCorreio {
    pub config: SmtpConfig,
}

impl SmtpCorreio {
    pub fn new(config: SmtpConfig) -> Self {
        Self { config }
    }

    pub fn construir_transporte(&self) -> Result<AsyncSmtpTransport<Tokio1Executor>, Erro> {
        if self.config.host.trim().is_empty() {
            return Err(Erro::invalido("SMTP host cannot be empty."));
        }

        let mut builder = if self.config.tls {
            if self.config.port == 465 {
                AsyncSmtpTransport::<Tokio1Executor>::relay(&self.config.host)
                    .map_err(|e| Erro::invalido(format!("Failed to build SMTP relay: {e}")))?
            } else {
                AsyncSmtpTransport::<Tokio1Executor>::starttls_relay(&self.config.host)
                    .map_err(|e| Erro::invalido(format!("Failed to build SMTP relay: {e}")))?
            }
        } else {
            AsyncSmtpTransport::<Tokio1Executor>::builder_dangerous(&self.config.host)
                .tls(Tls::None)
        };

        builder = builder.port(self.config.port);

        if !self.config.local_name.trim().is_empty() {
            builder = builder.hello_name(ClientId::Domain(self.config.local_name.clone()));
        }

        if !self.config.username.is_empty() || !self.config.password.is_empty() {
            builder = builder.credentials(Credentials::new(
                self.config.username.clone(),
                self.config.password.clone(),
            ));
        }

        let metodo = self.config.auth_method.to_uppercase();
        if metodo == "LOGIN" {
            builder = builder.authentication(vec![Mechanism::Login]);
        } else if metodo == "PLAIN" {
            builder = builder.authentication(vec![Mechanism::Plain]);
        }

        Ok(builder.build())
    }

    pub async fn testar_conexao(&self) -> Result<(), Erro> {
        let transport = self.construir_transporte()?;
        transport
            .test_connection()
            .await
            .map_err(|e| Erro::invalido(format!("Failed to connect to SMTP server: {e}")))?;
        Ok(())
    }

    pub async fn enviar_mensagem(&self, m: &Mensagem) -> Result<(), Erro> {
        let transport = self.construir_transporte()?;

        let from_header = match (&m.de_nome, &m.de_email) {
            (Some(nome), Some(email)) if !nome.trim().is_empty() => format!("{nome} <{email}>"),
            (_, Some(email)) => email.clone(),
            _ => "Support <support@example.com>".into(),
        };

        let msg = Message::builder()
            .from(
                from_header
                    .parse()
                    .map_err(|e| Erro::invalido(format!("Invalid from address: {e}")))?,
            )
            .to(m
                .para
                .parse()
                .map_err(|e| Erro::invalido(format!("Invalid to address: {e}")))?)
            .subject(&m.assunto)
            .header(lettre::message::header::ContentType::TEXT_HTML)
            .body(m.corpo.clone())
            .map_err(|e| Erro::invalido(format!("Failed to build email message: {e}")))?;

        transport
            .send(msg)
            .await
            .map_err(|e| Erro::invalido(format!("Failed to send email: {e}")))?;

        Ok(())
    }
}

#[async_trait::async_trait]
impl Correio for SmtpCorreio {
    async fn enviar(&self, m: &Mensagem) -> Result<(), Erro> {
        self.enviar_mensagem(m).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renderiza_template_com_placeholders() {
        let tpl = EmailTemplate::novo(
            "Verifique sua conta {APP_NAME}",
            "<p>Olá,</p><a href=\"{ACTION_URL}\">Clique aqui</a> para {RECORD:nome}.",
            "{APP_URL}/_/#/confirm/{TOKEN}",
        );

        let mut extras = HashMap::new();
        extras.insert("nome".into(), "Carlos".into());

        let (assunto, corpo) = tpl.renderizar(
            "Forjabase",
            "https://meuapp.com",
            "token123",
            &extras,
        );

        assert_eq!(assunto, "Verifique sua conta Forjabase");
        assert!(corpo.contains("https://meuapp.com/_/#/confirm/token123"));
        assert!(corpo.contains("para Carlos."));
    }
}
