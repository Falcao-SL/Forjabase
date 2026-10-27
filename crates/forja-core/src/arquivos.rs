//! Campo `file` no disco local (onda 0.3).
//!
//! Os bytes ficam em `{pasta}/storage/{colecaoId}/{registroId}/{arquivo}`.
//! O nome guardado no registro não é o original: é a base normalizada, um
//! trecho aleatório e a extensão. Miniatura e S3 não entram nesta fatia.

use crate::erro::{Erro, Resultado};
use crate::id;
use crate::{Collection, Field, FieldType};
use serde_json::{Map, Value, json};
use std::path::{Path, PathBuf};

/// Um arquivo que chegou no multipart, ainda sem nome final.
#[derive(Debug, Clone)]
pub struct Recebido {
    /// Nome do campo, sem o sufixo `+`.
    pub campo: String,
    /// `campo+`: acrescenta. Sem isso, os novos arquivos substituem os atuais.
    pub acrescentar: bool,
    pub nome_original: String,
    pub tipo: String,
    pub bytes: Vec<u8>,
}

/// O que gravar depois que o registro existir.
#[derive(Debug, Clone)]
pub struct Gravacao {
    pub nome: String,
    pub bytes: Vec<u8>,
}

#[derive(Debug)]
pub struct Resolvido {
    pub dados: Map<String, Value>,
    pub gravar: Vec<Gravacao>,
    /// Nomes que saíram do registro e devem sumir do disco.
    pub apagar: Vec<String>,
}

/// 5 MiB quando `maxSize` é 0 ou está ausente, como no PocketBase.
pub fn tamanho_maximo(f: &Field) -> u64 {
    let n = f.opcao_f64("maxSize").unwrap_or(0.0);
    if n <= 0.0 { 5 * 1024 * 1024 } else { n as u64 }
}

fn max_select(f: &Field) -> usize {
    let m = f.opcao_f64("maxSelect").unwrap_or(1.0);
    if m < 1.0 { 1 } else { m as usize }
}

fn tipos_mime(f: &Field) -> Vec<String> {
    f.options
        .get("mimeTypes")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

/// Extensão curta, só letras e dígitos.
fn extensao_ok(e: &str) -> bool {
    !e.is_empty() && e.len() <= 10 && e.bytes().all(|b| b.is_ascii_alphanumeric())
}

/// Nome que pode ir para o disco e para a URL. Sem pasta, sem `..`.
pub fn nome_seguro(nome: &str) -> bool {
    !nome.is_empty()
        && nome.len() <= 150
        && !nome.contains('/')
        && !nome.contains('\\')
        && !nome.contains("..")
        && nome
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'.' | b'-'))
}

/// `Minha Foto.PNG` → `minha_foto_a1b2c3d4e5.png`.
pub fn nome_armazenado(original: &str) -> String {
    let original = original.trim().trim_matches(|c| c == '/' || c == '\\');
    let original = original.rsplit(['/', '\\']).next().unwrap_or(original);
    let (base, ext) = match original.rsplit_once('.') {
        Some((b, e)) if !b.is_empty() && extensao_ok(e) => (b, Some(e.to_ascii_lowercase())),
        _ => (original, None),
    };
    let mut limpo = String::new();
    for ch in base.chars() {
        let c = ch.to_ascii_lowercase();
        if c.is_ascii_alphanumeric() {
            limpo.push(c);
        } else if !limpo.ends_with('_') {
            limpo.push('_');
        }
    }
    let limpo = limpo.trim_matches('_');
    let base: String = if limpo.is_empty() {
        "file".into()
    } else {
        limpo.chars().take(80).collect()
    };
    let mut nome = format!("{base}_{}", id::aleatorio(10));
    if let Some(e) = ext {
        nome.push('.');
        nome.push_str(&e);
    }
    nome
}

pub fn tipo_por_extensao(nome: &str) -> &'static str {
    match nome.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase()) {
        Some(e) if e == "jpg" || e == "jpeg" => "image/jpeg",
        Some(e) if e == "png" => "image/png",
        Some(e) if e == "gif" => "image/gif",
        Some(e) if e == "webp" => "image/webp",
        Some(e) if e == "svg" => "image/svg+xml",
        Some(e) if e == "pdf" => "application/pdf",
        Some(e) if e == "txt" => "text/plain",
        Some(e) if e == "json" => "application/json",
        Some(e) if e == "css" => "text/css",
        Some(e) if e == "js" => "text/javascript",
        Some(e) if e == "html" || e == "htm" => "text/html",
        Some(e) if e == "mp3" => "audio/mpeg",
        Some(e) if e == "mp4" => "video/mp4",
        Some(e) if e == "webm" => "video/webm",
        Some(e) if e == "wasm" => "application/wasm",
        _ => "application/octet-stream",
    }
}

fn tipo_efetivo(declarado: &str, nome_original: &str) -> String {
    let bruto = declarado.split(';').next().unwrap_or("").trim();
    if bruto.is_empty() || bruto.eq_ignore_ascii_case("application/octet-stream") {
        tipo_por_extensao(nome_original).to_string()
    } else {
        bruto.to_ascii_lowercase()
    }
}

fn segmento_ok(s: &str) -> bool {
    !s.is_empty() && s.len() <= 40 && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
}

pub fn pasta_registro(raiz: &Path, colecao: &str, registro: &str) -> PathBuf {
    raiz.join("storage").join(colecao).join(registro)
}

pub fn pasta_colecao(raiz: &Path, colecao: &str) -> PathBuf {
    raiz.join("storage").join(colecao)
}

/// Caminho final. Recusa segmento que saia da pasta `storage`.
pub fn caminho(raiz: &Path, colecao: &str, registro: &str, arquivo: &str) -> Resultado<PathBuf> {
    if !segmento_ok(colecao) || !id::valido(registro) || !nome_seguro(arquivo) {
        return Err(Erro::NaoEncontrado(
            "The requested resource wasn't found.".into(),
        ));
    }
    Ok(pasta_registro(raiz, colecao, registro).join(arquivo))
}

pub fn gravar(
    raiz: &Path,
    colecao: &str,
    registro: &str,
    nome: &str,
    bytes: &[u8],
) -> Resultado<()> {
    let caminho = caminho(raiz, colecao, registro, nome)?;
    if let Some(dir) = caminho.parent() {
        std::fs::create_dir_all(dir).map_err(|e| Erro::Interno(format!("arquivo: {e}")))?;
    }
    std::fs::write(&caminho, bytes).map_err(|e| Erro::Interno(format!("arquivo: {e}")))
}

pub fn remover_dir(caminho: &Path) {
    if caminho.exists() {
        let _ = std::fs::remove_dir_all(caminho);
    }
}

pub fn remover_arquivo(raiz: &Path, colecao: &str, registro: &str, nome: &str) {
    if let Ok(caminho) = caminho(raiz, colecao, registro, nome)
        && caminho.is_file()
    {
        let _ = std::fs::remove_file(caminho);
    }
}

fn nomes_de(v: Option<&Value>) -> Vec<String> {
    match v {
        None | Some(Value::Null) => vec![],
        Some(Value::String(s)) if s.is_empty() => vec![],
        Some(Value::String(s)) => vec![s.clone()],
        Some(Value::Array(a)) => a
            .iter()
            .filter_map(|x| x.as_str())
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .collect(),
        _ => vec![],
    }
}

fn valor_nomes(f: &Field, nomes: &[String]) -> Value {
    if f.multiplo() {
        json!(nomes)
    } else {
        json!(nomes.first().cloned().unwrap_or_default())
    }
}

fn erro(campos: &mut Map<String, Value>, nome: &str, codigo: &str, msg: String) {
    campos.insert(nome.into(), json!({ "code": codigo, "message": msg }));
}

/// Resolve o valor final de cada campo `file` antes da validação comum.
///
/// No criar, `atual` é `None`. Nome que não veio de um upload e não está no
/// registro atual é recusado: o cliente não escolhe o caminho no disco.
pub fn resolver(
    c: &Collection,
    dados: &Map<String, Value>,
    atual: Option<&Value>,
    recebidos: Vec<Recebido>,
) -> Result<Resolvido, Map<String, Value>> {
    let mut dados = dados.clone();
    let mut gravar = Vec::new();
    let mut apagar = Vec::new();
    let mut campos = Map::new();
    let arquivos: Vec<&Field> = c
        .fields
        .iter()
        .filter(|f| f.kind == FieldType::File)
        .collect();
    if arquivos.is_empty() {
        return Ok(Resolvido {
            dados,
            gravar,
            apagar,
        });
    }

    let mut remocoes: std::collections::HashMap<String, Vec<String>> =
        std::collections::HashMap::new();
    for f in &arquivos {
        let chave = format!("{}-", f.name);
        if let Some(v) = dados.remove(&chave) {
            remocoes.insert(f.name.clone(), nomes_de(Some(&v)));
        }
    }

    for f in &arquivos {
        let existentes = nomes_de(atual.and_then(|r| r.get(&f.name)));
        let mut finais = existentes.clone();
        if let Some(tirar) = remocoes.get(&f.name) {
            finais.retain(|n| !tirar.iter().any(|t| t == n));
        }
        let ups: Vec<&Recebido> = recebidos.iter().filter(|r| r.campo == f.name).collect();
        let presente = dados.contains_key(&f.name);
        let mut mexeu = presente || !ups.is_empty() || remocoes.contains_key(&f.name);

        if !ups.is_empty() {
            let acrescentar = ups.iter().all(|u| u.acrescentar);
            if !acrescentar {
                for n in finais.drain(..) {
                    apagar.push(n);
                }
            }
            for u in ups {
                if u.bytes.len() as u64 > tamanho_maximo(f) {
                    erro(
                        &mut campos,
                        &f.name,
                        "validation_file_size_limit",
                        format!("The file is too big (max {} bytes).", tamanho_maximo(f)),
                    );
                    continue;
                }
                let permitidos = tipos_mime(f);
                if !permitidos.is_empty() {
                    let tipo = tipo_efetivo(&u.tipo, &u.nome_original);
                    if !permitidos.iter().any(|p| p.eq_ignore_ascii_case(&tipo)) {
                        erro(
                            &mut campos,
                            &f.name,
                            "validation_invalid_mime_type",
                            format!("Invalid file type '{tipo}'."),
                        );
                        continue;
                    }
                }
                let nome = nome_armazenado(&u.nome_original);
                finais.push(nome.clone());
                gravar.push(Gravacao {
                    nome,
                    bytes: u.bytes.clone(),
                });
            }
            dados.remove(&f.name);
            mexeu = true;
        } else if presente {
            let pedidos = nomes_de(dados.get(&f.name));
            if pedidos.iter().any(|n| !nome_seguro(n))
                || pedidos.iter().any(|n| !existentes.iter().any(|e| e == n))
            {
                erro(
                    &mut campos,
                    &f.name,
                    "validation_invalid_value",
                    "Invalid file.".into(),
                );
            }
            for n in &existentes {
                if !pedidos.iter().any(|p| p == n) {
                    apagar.push(n.clone());
                }
            }
            finais = pedidos;
        }

        if !mexeu {
            continue;
        }
        let mut unicos = Vec::new();
        for n in finais {
            if !unicos.iter().any(|u| u == &n) {
                unicos.push(n);
            }
        }
        let max = max_select(f);
        if unicos.len() > max {
            erro(
                &mut campos,
                &f.name,
                "validation_too_many_values",
                format!("Select no more than {max}."),
            );
            unicos.truncate(max);
        }
        dados.insert(f.name.clone(), valor_nomes(f, &unicos));
    }

    if campos.is_empty() {
        Ok(Resolvido {
            dados,
            gravar,
            apagar,
        })
    } else {
        Err(campos)
    }
}

/// `Some(protegido)` se o nome está num campo `file` do registro.
pub fn arquivo_do_registro(c: &Collection, registro: &Value, arquivo: &str) -> Option<bool> {
    c.fields
        .iter()
        .filter(|f| f.kind == FieldType::File)
        .find_map(|f| {
            let nomes = nomes_de(registro.get(&f.name));
            if nomes.iter().any(|n| n == arquivo) {
                Some(f.opcao_bool("protected"))
            } else {
                None
            }
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nome_ganha_sufixo_e_nao_tem_espaco() {
        let n = nome_armazenado("Minha Foto.PNG");
        assert!(n.starts_with("minha_foto_"), "{n}");
        assert!(n.ends_with(".png"), "{n}");
        assert!(nome_seguro(&n), "{n}");
        assert!(nome_armazenado("").starts_with("file_"));
        assert!(!nome_seguro("../segredo"));
        assert!(!nome_seguro("a/b.png"));
    }
}
