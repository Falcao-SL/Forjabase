//! IDs de registro no formato do PocketBase: 15 caracteres `[a-z0-9]`.

use rand::RngExt;

const ALFABETO: &[u8] = b"abcdefghijklmnopqrstuvwxyz0123456789";

pub const TAMANHO: usize = 15;

pub fn novo() -> String {
    aleatorio(TAMANHO)
}

/// Texto aleatório do mesmo alfabeto, para segredos e chaves de token.
pub fn aleatorio(n: usize) -> String {
    let mut r = rand::rng();
    (0..n).map(|_| ALFABETO[r.random_range(0..ALFABETO.len())] as char).collect()
}

pub fn valido(id: &str) -> bool {
    id.len() == TAMANHO && id.bytes().all(|b| ALFABETO.contains(&b))
}

#[cfg(test)]
mod tests {
    #[test]
    fn formato() {
        let id = super::novo();
        assert!(super::valido(&id), "{id}");
        assert_ne!(id, super::novo());
        assert!(!super::valido("ABC"));
    }
}
