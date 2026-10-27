//! Caminhos de `expand` (`autor.cidade`, `comentarios_via_post`).
//! A gravação e a cascata ficam no `Banco`.

/// Níveis de `expand` aninhado. O sétimo é recusado.
pub const PROFUNDIDADE_MAX: usize = 6;

/// Agrupa `autor.cidade, comentarios_via_post, autor.pais` em
/// `[("autor", "cidade,pais"), ("comentarios_via_post", "")]`.
pub fn pedidos(caminho: &str) -> Vec<(String, String)> {
    let mut grupos: Vec<(String, Vec<String>)> = vec![];
    for parte in caminho.split(',') {
        let parte = parte.trim();
        if parte.is_empty() {
            continue;
        }
        let mut segs = parte.split('.').map(str::trim).filter(|s| !s.is_empty());
        let Some(primeiro) = segs.next() else {
            continue;
        };
        let resto = segs.collect::<Vec<_>>().join(".");
        if let Some((_, sufixos)) = grupos.iter_mut().find(|(n, _)| n == primeiro) {
            if !resto.is_empty() {
                sufixos.push(resto);
            }
        } else if resto.is_empty() {
            grupos.push((primeiro.to_string(), vec![]));
        } else {
            grupos.push((primeiro.to_string(), vec![resto]));
        }
    }
    grupos.into_iter().map(|(n, s)| (n, s.join(","))).collect()
}

/// `comentarios_via_post` → `("comentarios", "post")`.
/// O campo pode ter underscore (`posts_via_criado_por`); o separador é o
/// primeiro `_via_`, e coleção e campo não podem conter esse trecho.
pub fn partir_via(nome: &str) -> Option<(&str, &str)> {
    let (colecao, campo) = nome.split_once("_via_")?;
    if colecao.is_empty() || campo.is_empty() {
        None
    } else {
        Some((colecao, campo))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn agrupa_caminhos() {
        let p = pedidos("autor.cidade, comentarios_via_post , autor.pais");
        assert_eq!(
            p,
            vec![
                ("autor".into(), "cidade,pais".into()),
                ("comentarios_via_post".into(), String::new()),
            ]
        );
    }

    #[test]
    fn separa_relacao_reversa() {
        assert_eq!(
            partir_via("posts_via_criado_por"),
            Some(("posts", "criado_por"))
        );
        assert_eq!(partir_via("autor"), None);
        assert_eq!(partir_via("_via_post"), None);
    }
}
