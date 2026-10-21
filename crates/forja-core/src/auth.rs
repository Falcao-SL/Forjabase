//! Senhas (Argon2id) e tokens (JWT HS256).
//!
//! O token leva a `tokenKey` do usuário: trocar a senha troca a chave e
//! invalida todos os tokens antigos, como no PocketBase.

use crate::erro::{Erro, Resultado};
use argon2::{Argon2, PasswordHash, PasswordHasher, PasswordVerifier};
use jsonwebtoken::{Algorithm, DecodingKey, EncodingKey, Header, Validation, decode, encode};
use serde::{Deserialize, Serialize};

/// Id fixo da coleção de superusuários.
pub const COLECAO_SUPERUSERS: &str = "pbc_superusers";

/// Validade do token de superusuário: 1 dia (o padrão do PocketBase).
pub const VALIDADE_SUPERUSER: u64 = 86_400;

pub fn hash_senha(senha: &str) -> Resultado<String> {
    Argon2::default()
        .hash_password(senha.as_bytes())
        .map(|h| h.to_string())
        .map_err(|e| Erro::Interno(format!("hash: {e}")))
}

pub fn conferir_senha(hash: &str, senha: &str) -> bool {
    PasswordHash::new(hash)
        .map(|h| Argon2::default().verify_password(senha.as_bytes(), &h).is_ok())
        .unwrap_or(false)
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Claims {
    pub id: String,
    #[serde(rename = "type")]
    pub tipo: String,
    #[serde(rename = "collectionId")]
    pub colecao: String,
    pub refreshable: bool,
    /// `tokenKey` do usuário no momento da emissão.
    pub tk: String,
    pub exp: u64,
}

fn agora_s() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

pub fn emitir(segredo: &str, id: &str, colecao: &str, token_key: &str, validade: u64) -> Resultado<String> {
    let claims = Claims {
        id: id.into(),
        tipo: "auth".into(),
        colecao: colecao.into(),
        refreshable: true,
        tk: token_key.into(),
        exp: agora_s() + validade,
    };
    encode(&Header::new(Algorithm::HS256), &claims, &EncodingKey::from_secret(segredo.as_bytes()))
        .map_err(|e| Erro::Interno(format!("jwt: {e}")))
}

/// Confere assinatura e validade. A `tokenKey` é conferida por quem chama.
pub fn validar(segredo: &str, token: &str) -> Resultado<Claims> {
    let mut v = Validation::new(Algorithm::HS256);
    v.leeway = 0;
    decode::<Claims>(token, &DecodingKey::from_secret(segredo.as_bytes()), &v)
        .map(|d| d.claims)
        .map_err(|_| Erro::NaoAutorizado("The request requires valid record authorization token.".into()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn senha() {
        let h = hash_senha("correta-123").unwrap();
        assert!(h.starts_with("$argon2id$"), "{h}");
        assert!(conferir_senha(&h, "correta-123"));
        assert!(!conferir_senha(&h, "errada"));
        assert!(!conferir_senha("lixo", "correta-123"));
    }

    #[test]
    fn token() {
        let t = emitir("segredo", "abc", COLECAO_SUPERUSERS, "k1", 60).unwrap();
        let c = validar("segredo", &t).unwrap();
        assert_eq!((c.id.as_str(), c.tk.as_str()), ("abc", "k1"));
        assert!(validar("outro-segredo", &t).is_err());
        assert!(validar("segredo", "nao.e.token").is_err());
        let vencido = emitir("segredo", "abc", COLECAO_SUPERUSERS, "k1", 0).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(1100));
        assert!(validar("segredo", &vencido).is_err());
    }
}
