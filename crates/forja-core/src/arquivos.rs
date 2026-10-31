//! Campo `file` no disco local (onda 0.3).
//!
//! Os bytes ficam em `{pasta}/storage/{colecaoId}/{registroId}/{arquivo}`.
//! O nome guardado no registro não é o original: é a base normalizada, um
//! trecho aleatório e a extensão. Miniatura e S3 não entram nesta fatia.

use crate::erro::{Erro, Resultado};
use crate::id;
use crate::{Collection, Field, FieldType};
use serde::{Deserialize, Serialize};
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct S3Config {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub bucket: String,
    #[serde(default = "S3Config::region_padrao")]
    pub region: String,
    #[serde(default)]
    pub endpoint: String,
    #[serde(default)]
    pub access_key: String,
    #[serde(default)]
    pub secret: String,
    #[serde(default)]
    pub force_path_style: bool,
}

impl S3Config {
    fn region_padrao() -> String {
        "us-east-1".to_string()
    }
}

impl Default for S3Config {
    fn default() -> Self {
        Self {
            enabled: false,
            bucket: String::new(),
            region: Self::region_padrao(),
            endpoint: String::new(),
            access_key: String::new(),
            secret: String::new(),
            force_path_style: false,
        }
    }
}

/// Abstração de armazenamento de arquivos (local ou S3 compatível).
#[async_trait::async_trait]
pub trait ProvedorStorage: Send + Sync {
    async fn gravar(
        &self,
        colecao: &str,
        registro: &str,
        nome: &str,
        bytes: &[u8],
        tipo_mime: &str,
    ) -> Resultado<()>;

    async fn ler(&self, colecao: &str, registro: &str, nome: &str) -> Resultado<Vec<u8>>;

    async fn remover(&self, colecao: &str, registro: &str, nome: &str) -> Resultado<()>;

    async fn remover_dir(&self, colecao: &str, registro: &str) -> Resultado<()>;

    async fn gravar_thumb(
        &self,
        colecao: &str,
        registro: &str,
        arquivo: &str,
        thumb: &str,
        bytes: &[u8],
    ) -> Resultado<()>;

    async fn ler_thumb(
        &self,
        colecao: &str,
        registro: &str,
        arquivo: &str,
        thumb: &str,
    ) -> Option<Vec<u8>>;
}

#[derive(Debug, Clone)]
pub struct DiscoLocal {
    pub raiz: PathBuf,
}

impl DiscoLocal {
    pub fn new(raiz: impl Into<PathBuf>) -> Self {
        Self { raiz: raiz.into() }
    }
}

#[async_trait::async_trait]
impl ProvedorStorage for DiscoLocal {
    async fn gravar(
        &self,
        colecao: &str,
        registro: &str,
        nome: &str,
        bytes: &[u8],
        _tipo_mime: &str,
    ) -> Resultado<()> {
        let caminho = caminho(&self.raiz, colecao, registro, nome)?;
        if let Some(dir) = caminho.parent() {
            tokio::fs::create_dir_all(dir)
                .await
                .map_err(|e| Erro::Interno(format!("arquivo: {e}")))?;
        }
        tokio::fs::write(&caminho, bytes)
            .await
            .map_err(|e| Erro::Interno(format!("arquivo: {e}")))
    }

    async fn ler(&self, colecao: &str, registro: &str, nome: &str) -> Resultado<Vec<u8>> {
        let caminho = caminho(&self.raiz, colecao, registro, nome)?;
        tokio::fs::read(&caminho)
            .await
            .map_err(|_| Erro::NaoEncontrado("The requested resource wasn't found.".into()))
    }

    async fn remover(&self, colecao: &str, registro: &str, nome: &str) -> Resultado<()> {
        if let Ok(caminho) = caminho(&self.raiz, colecao, registro, nome) {
            let _ = tokio::fs::remove_file(caminho).await;
        }
        let pasta_thumbs =
            pasta_registro(&self.raiz, colecao, registro).join(format!("thumbs_{nome}"));
        let _ = tokio::fs::remove_dir_all(pasta_thumbs).await;
        Ok(())
    }

    async fn remover_dir(&self, colecao: &str, registro: &str) -> Resultado<()> {
        let pasta = pasta_registro(&self.raiz, colecao, registro);
        let _ = tokio::fs::remove_dir_all(pasta).await;
        Ok(())
    }

    async fn gravar_thumb(
        &self,
        colecao: &str,
        registro: &str,
        arquivo: &str,
        thumb: &str,
        bytes: &[u8],
    ) -> Resultado<()> {
        let caminho = caminho_thumb(&self.raiz, colecao, registro, arquivo, thumb)?;
        if let Some(dir) = caminho.parent() {
            tokio::fs::create_dir_all(dir)
                .await
                .map_err(|e| Erro::Interno(format!("thumb: {e}")))?;
        }
        tokio::fs::write(&caminho, bytes)
            .await
            .map_err(|e| Erro::Interno(format!("thumb: {e}")))
    }

    async fn ler_thumb(
        &self,
        colecao: &str,
        registro: &str,
        arquivo: &str,
        thumb: &str,
    ) -> Option<Vec<u8>> {
        let caminho = caminho_thumb(&self.raiz, colecao, registro, arquivo, thumb).ok()?;
        tokio::fs::read(&caminho).await.ok()
    }
}

#[allow(clippy::too_many_arguments)]
pub fn assinar_s3(
    secret: &str,
    access_key: &str,
    region: &str,
    metodo: &str,
    path: &str,
    query: &str,
    host: &str,
    payload: &[u8],
    data_hora: &str,
    data: &str,
) -> String {
    use hmac::{Hmac, Mac};
    use sha2::{Digest, Sha256};
    type HmacSha256 = Hmac<Sha256>;

    let mut hasher = Sha256::new();
    hasher.update(payload);
    let payload_hash = hex::encode(hasher.finalize());

    let signed_headers = "host;x-amz-content-sha256;x-amz-date";
    let canonical_headers = format!(
        "host:{host}\nx-amz-content-sha256:{payload_hash}\nx-amz-date:{data_hora}\n"
    );

    let canonical_request = format!(
        "{metodo}\n{path}\n{query}\n{canonical_headers}\n{signed_headers}\n{payload_hash}"
    );

    let mut cr_hasher = Sha256::new();
    cr_hasher.update(canonical_request.as_bytes());
    let cr_hash = hex::encode(cr_hasher.finalize());

    let scope = format!("{data}/{region}/s3/aws4_request");
    let string_to_sign = format!("AWS4-HMAC-SHA256\n{data_hora}\n{scope}\n{cr_hash}");

    let k_secret = format!("AWS4{secret}");
    let mut mac1 = HmacSha256::new_from_slice(k_secret.as_bytes()).unwrap();
    mac1.update(data.as_bytes());
    let k_date = mac1.finalize().into_bytes();

    let mut mac2 = HmacSha256::new_from_slice(&k_date).unwrap();
    mac2.update(region.as_bytes());
    let k_region = mac2.finalize().into_bytes();

    let mut mac3 = HmacSha256::new_from_slice(&k_region).unwrap();
    mac3.update(b"s3");
    let k_service = mac3.finalize().into_bytes();

    let mut mac4 = HmacSha256::new_from_slice(&k_service).unwrap();
    mac4.update(b"aws4_request");
    let k_signing = mac4.finalize().into_bytes();

    let mut mac5 = HmacSha256::new_from_slice(&k_signing).unwrap();
    mac5.update(string_to_sign.as_bytes());
    let signature = hex::encode(mac5.finalize().into_bytes());

    format!(
        "AWS4-HMAC-SHA256 Credential={access_key}/{scope}, SignedHeaders={signed_headers}, Signature={signature}"
    )
}

#[derive(Debug, Clone)]
pub struct S3Storage {
    pub config: S3Config,
    client: reqwest::Client,
}

impl S3Storage {
    pub fn new(config: S3Config) -> Self {
        Self {
            config,
            client: reqwest::Client::builder()
                .build()
                .unwrap_or_default(),
        }
    }

    pub fn is_path_style(&self) -> bool {
        self.config.force_path_style
            || self.config.endpoint.contains("localhost")
            || self.config.endpoint.contains("127.0.0.1")
    }

    pub fn bucket_url(&self) -> (String, String) {
        let endpoint = if self.config.endpoint.trim().is_empty() {
            if self.config.region == "us-east-1" {
                "https://s3.amazonaws.com".to_string()
            } else {
                format!("https://s3.{}.amazonaws.com", self.config.region)
            }
        } else {
            let ep = self.config.endpoint.trim();
            if ep.starts_with("http://") || ep.starts_with("https://") {
                ep.to_string()
            } else {
                format!("https://{ep}")
            }
        };

        let bucket = &self.config.bucket;
        if self.is_path_style() {
            let base = endpoint.trim_end_matches('/');
            let host = base
                .split("://")
                .nth(1)
                .unwrap_or(base)
                .split('/')
                .next()
                .unwrap_or(base)
                .to_string();
            let url = format!("{base}/{bucket}");
            (host, url)
        } else {
            let scheme = endpoint.split("://").next().unwrap_or("https");
            let host_part = endpoint
                .split("://")
                .nth(1)
                .unwrap_or(&endpoint)
                .trim_end_matches('/');
            let host = format!("{bucket}.{host_part}");
            let url = format!("{scheme}://{host}");
            (host, url)
        }
    }

    pub fn object_url(&self, key: &str) -> (String, String, String) {
        let (host, base) = self.bucket_url();
        let key_clean = key.trim_start_matches('/');
        let url = format!("{base}/{key_clean}");
        let path = if self.is_path_style() {
            format!("/{}/{}", self.config.bucket, key_clean)
        } else {
            format!("/{key_clean}")
        };
        (host, path, url)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn assinar(
        &self,
        metodo: &str,
        path: &str,
        query: &str,
        host: &str,
        payload: &[u8],
        data_hora: &str,
        data: &str,
    ) -> String {
        assinar_s3(
            &self.config.secret,
            &self.config.access_key,
            &self.config.region,
            metodo,
            path,
            query,
            host,
            payload,
            data_hora,
            data,
        )
    }

    pub async fn testar_conexao(&self) -> Resultado<()> {
        if self.config.bucket.trim().is_empty() {
            return Err(Erro::invalido("S3 bucket name cannot be blank."));
        }
        if self.config.access_key.trim().is_empty() {
            return Err(Erro::invalido("S3 access key cannot be blank."));
        }
        if self.config.secret.trim().is_empty() {
            return Err(Erro::invalido("S3 secret cannot be blank."));
        }

        let (ano, mes, dia, h, m, seg, _, _) = crate::tempo_utc();
        let data = format!("{ano:04}{mes:02}{dia:02}");
        let data_hora = format!("{ano:04}{mes:02}{dia:02}T{h:02}{m:02}{seg:02}Z");

        let (host, base) = self.bucket_url();
        let path = if self.config.force_path_style {
            format!("/{}", self.config.bucket)
        } else {
            "/".to_string()
        };
        let query = "max-keys=0";
        let auth = self.assinar("GET", &path, query, &host, &[], &data_hora, &data);

        let url = format!("{base}?{query}");
        let resp = self
            .client
            .get(&url)
            .header("host", &host)
            .header("x-amz-date", &data_hora)
            .header(
                "x-amz-content-sha256",
                "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
            )
            .header("Authorization", &auth)
            .send()
            .await
            .map_err(|e| Erro::invalido(format!("Failed to connect to S3 endpoint: {e}")))?;

        if resp.status().is_success() {
            Ok(())
        } else {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            Err(Erro::invalido(format!("S3 test failed ({status}): {body}")))
        }
    }
}

#[async_trait::async_trait]
impl ProvedorStorage for S3Storage {
    async fn gravar(
        &self,
        colecao: &str,
        registro: &str,
        nome: &str,
        bytes: &[u8],
        tipo_mime: &str,
    ) -> Resultado<()> {
        let key = format!("{colecao}/{registro}/{nome}");
        let (host, path, url) = self.object_url(&key);

        let (ano, mes, dia, h, m, seg, _, _) = crate::tempo_utc();
        let data = format!("{ano:04}{mes:02}{dia:02}");
        let data_hora = format!("{ano:04}{mes:02}{dia:02}T{h:02}{m:02}{seg:02}Z");

        let auth = self.assinar("PUT", &path, "", &host, bytes, &data_hora, &data);
        use sha2::{Digest, Sha256};
        let payload_hash = hex::encode(Sha256::digest(bytes));

        let mime = if tipo_mime.is_empty() {
            tipo_por_extensao(nome)
        } else {
            tipo_mime
        };

        let resp = self
            .client
            .put(&url)
            .header("host", &host)
            .header("x-amz-date", &data_hora)
            .header("x-amz-content-sha256", &payload_hash)
            .header("Authorization", &auth)
            .header("content-type", mime)
            .body(bytes.to_vec())
            .send()
            .await
            .map_err(|e| Erro::Interno(format!("S3 upload error: {e}")))?;

        if resp.status().is_success() {
            Ok(())
        } else {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            Err(Erro::Interno(format!("S3 upload failed ({status}): {body}")))
        }
    }

    async fn ler(&self, colecao: &str, registro: &str, nome: &str) -> Resultado<Vec<u8>> {
        let key = format!("{colecao}/{registro}/{nome}");
        let (host, path, url) = self.object_url(&key);

        let (ano, mes, dia, h, m, seg, _, _) = crate::tempo_utc();
        let data = format!("{ano:04}{mes:02}{dia:02}");
        let data_hora = format!("{ano:04}{mes:02}{dia:02}T{h:02}{m:02}{seg:02}Z");

        let auth = self.assinar("GET", &path, "", &host, &[], &data_hora, &data);

        let resp = self
            .client
            .get(&url)
            .header("host", &host)
            .header("x-amz-date", &data_hora)
            .header(
                "x-amz-content-sha256",
                "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
            )
            .header("Authorization", &auth)
            .send()
            .await
            .map_err(|_| Erro::NaoEncontrado("The requested resource wasn't found.".into()))?;

        if resp.status().is_success() {
            let b = resp
                .bytes()
                .await
                .map_err(|e| Erro::Interno(e.to_string()))?;
            Ok(b.to_vec())
        } else {
            Err(Erro::NaoEncontrado(
                "The requested resource wasn't found.".into(),
            ))
        }
    }

    async fn remover(&self, colecao: &str, registro: &str, nome: &str) -> Resultado<()> {
        let key = format!("{colecao}/{registro}/{nome}");
        let (host, path, url) = self.object_url(&key);

        let (ano, mes, dia, h, m, seg, _, _) = crate::tempo_utc();
        let data = format!("{ano:04}{mes:02}{dia:02}");
        let data_hora = format!("{ano:04}{mes:02}{dia:02}T{h:02}{m:02}{seg:02}Z");

        let auth = self.assinar("DELETE", &path, "", &host, &[], &data_hora, &data);

        let _ = self
            .client
            .delete(&url)
            .header("host", &host)
            .header("x-amz-date", &data_hora)
            .header(
                "x-amz-content-sha256",
                "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
            )
            .header("Authorization", &auth)
            .send()
            .await;

        Ok(())
    }

    async fn remover_dir(&self, _colecao: &str, _registro: &str) -> Resultado<()> {
        Ok(())
    }

    async fn gravar_thumb(
        &self,
        colecao: &str,
        registro: &str,
        arquivo: &str,
        thumb: &str,
        bytes: &[u8],
    ) -> Resultado<()> {
        let subnome = format!("thumbs_{arquivo}/{thumb}_{arquivo}");
        self.gravar(colecao, registro, &subnome, bytes, "image/jpeg")
            .await
    }

    async fn ler_thumb(
        &self,
        colecao: &str,
        registro: &str,
        arquivo: &str,
        thumb: &str,
    ) -> Option<Vec<u8>> {
        let subnome = format!("thumbs_{arquivo}/{thumb}_{arquivo}");
        self.ler(colecao, registro, &subnome).await.ok()
    }
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
    let pasta_thumbs = pasta_registro(raiz, colecao, registro).join(format!("thumbs_{nome}"));
    remover_dir(&pasta_thumbs);
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ThumbModo {
    Center,
    Top,
    Bottom,
    Fit,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ThumbSpec {
    pub w: u32,
    pub h: u32,
    pub modo: ThumbModo,
}

impl ThumbSpec {
    pub fn parse(s: &str) -> Option<ThumbSpec> {
        let s = s.trim();
        if s.is_empty() || s.len() > 20 {
            return None;
        }
        let (dims, modo) = if let Some(stripped) = s.strip_suffix('t') {
            (stripped, ThumbModo::Top)
        } else if let Some(stripped) = s.strip_suffix('b') {
            (stripped, ThumbModo::Bottom)
        } else if let Some(stripped) = s.strip_suffix('f') {
            (stripped, ThumbModo::Fit)
        } else {
            (s, ThumbModo::Center)
        };
        let (w_str, h_str) = dims.split_once('x')?;
        let w = w_str.parse::<u32>().ok()?;
        let h = h_str.parse::<u32>().ok()?;
        if w == 0 && h == 0 {
            return None;
        }
        if w > 4000 || h > 4000 {
            return None;
        }
        Some(ThumbSpec { w, h, modo })
    }
}

pub fn thumb_seguro(s: &str) -> bool {
    ThumbSpec::parse(s).is_some()
}

pub fn caminho_thumb(
    raiz: &Path,
    colecao: &str,
    registro: &str,
    arquivo: &str,
    thumb: &str,
) -> Resultado<PathBuf> {
    if !segmento_ok(colecao) || !id::valido(registro) || !nome_seguro(arquivo) || !thumb_seguro(thumb)
    {
        return Err(Erro::NaoEncontrado(
            "The requested resource wasn't found.".into(),
        ));
    }
    let nome_thumb = format!("{thumb}_{arquivo}");
    Ok(pasta_registro(raiz, colecao, registro)
        .join(format!("thumbs_{arquivo}"))
        .join(nome_thumb))
}

pub fn gerar_thumb(bytes: &[u8], spec: &ThumbSpec, ext: &str) -> Option<Vec<u8>> {
    use image::imageops::FilterType;
    let img = image::load_from_memory(bytes).ok()?;
    let (orig_w, orig_h) = (img.width(), img.height());
    if orig_w == 0 || orig_h == 0 {
        return None;
    }

    let processada = if spec.w == 0 {
        img.resize(orig_w.saturating_mul(spec.h) / orig_h.max(1), spec.h, FilterType::Triangle)
    } else if spec.h == 0 {
        img.resize(spec.w, orig_h.saturating_mul(spec.w) / orig_w.max(1), FilterType::Triangle)
    } else {
        match spec.modo {
            ThumbModo::Center => img.resize_to_fill(spec.w, spec.h, FilterType::Triangle),
            ThumbModo::Fit => img.resize(spec.w, spec.h, FilterType::Triangle),
            ThumbModo::Top => {
                let target_ratio = spec.w as f64 / spec.h as f64;
                let orig_ratio = orig_w as f64 / orig_h as f64;
                if orig_ratio > target_ratio {
                    let crop_w = ((orig_h as f64 * target_ratio).round() as u32).min(orig_w);
                    let x = (orig_w - crop_w) / 2;
                    img.crop_imm(x, 0, crop_w, orig_h)
                        .resize(spec.w, spec.h, FilterType::Triangle)
                } else {
                    let crop_h = ((orig_w as f64 / target_ratio).round() as u32).min(orig_h);
                    img.crop_imm(0, 0, orig_w, crop_h)
                        .resize(spec.w, spec.h, FilterType::Triangle)
                }
            }
            ThumbModo::Bottom => {
                let target_ratio = spec.w as f64 / spec.h as f64;
                let orig_ratio = orig_w as f64 / orig_h as f64;
                if orig_ratio > target_ratio {
                    let crop_w = ((orig_h as f64 * target_ratio).round() as u32).min(orig_w);
                    let x = (orig_w - crop_w) / 2;
                    img.crop_imm(x, 0, crop_w, orig_h)
                        .resize(spec.w, spec.h, FilterType::Triangle)
                } else {
                    let crop_h = ((orig_w as f64 / target_ratio).round() as u32).min(orig_h);
                    let y = orig_h.saturating_sub(crop_h);
                    img.crop_imm(0, y, orig_w, crop_h)
                        .resize(spec.w, spec.h, FilterType::Triangle)
                }
            }
        }
    };

    let formato = match ext.to_ascii_lowercase().as_str() {
        "jpg" | "jpeg" => image::ImageFormat::Jpeg,
        "png" => image::ImageFormat::Png,
        "webp" => image::ImageFormat::WebP,
        _ => image::ImageFormat::Png,
    };

    let mut buf = std::io::Cursor::new(Vec::new());
    processada.write_to(&mut buf, formato).ok()?;
    Some(buf.into_inner())
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

    #[test]
    fn thumb_spec_parse_e_geracao() {
        assert_eq!(
            ThumbSpec::parse("100x100"),
            Some(ThumbSpec {
                w: 100,
                h: 100,
                modo: ThumbModo::Center
            })
        );
        assert_eq!(
            ThumbSpec::parse("50x50t"),
            Some(ThumbSpec {
                w: 50,
                h: 50,
                modo: ThumbModo::Top
            })
        );
        assert_eq!(
            ThumbSpec::parse("0x80"),
            Some(ThumbSpec {
                w: 0,
                h: 80,
                modo: ThumbModo::Center
            })
        );
        assert_eq!(
            ThumbSpec::parse("200x0b"),
            Some(ThumbSpec {
                w: 200,
                h: 0,
                modo: ThumbModo::Bottom
            })
        );
        assert!(ThumbSpec::parse("invalido").is_none());
        assert!(ThumbSpec::parse("0x0").is_none());

        let img = image::RgbaImage::new(20, 10);
        let mut buf = std::io::Cursor::new(Vec::new());
        img.write_to(&mut buf, image::ImageFormat::Png).unwrap();
        let bytes = buf.into_inner();

        let spec = ThumbSpec::parse("10x10").unwrap();
        let thumb_bytes = gerar_thumb(&bytes, &spec, "png").expect("gera thumb png");
        let decoded = image::load_from_memory(&thumb_bytes).unwrap();
        assert_eq!(decoded.width(), 10);
        assert_eq!(decoded.height(), 10);
    }

    #[test]
    fn s3_config_e_assinatura() {
        let cfg = S3Config {
            enabled: true,
            bucket: "meu-bucket".to_string(),
            region: "sa-east-1".to_string(),
            endpoint: "https://s3.sa-east-1.amazonaws.com".to_string(),
            access_key: "AKIAEXAMPLE".to_string(),
            secret: "wJalrXUtnFEMI/K7MDENG+bPxRfiCYEXAMPLEKEY".to_string(),
            force_path_style: false,
        };
        let s3 = S3Storage::new(cfg);
        let (host, url) = s3.bucket_url();
        assert_eq!(host, "meu-bucket.s3.sa-east-1.amazonaws.com");
        assert_eq!(url, "https://meu-bucket.s3.sa-east-1.amazonaws.com");

        let (_, path, obj_url) = s3.object_url("posts/123/foto.png");
        assert_eq!(path, "/posts/123/foto.png");
        assert_eq!(
            obj_url,
            "https://meu-bucket.s3.sa-east-1.amazonaws.com/posts/123/foto.png"
        );

        let auth = assinar_s3(
            "wJalrXUtnFEMI/K7MDENG+bPxRfiCYEXAMPLEKEY",
            "AKIAEXAMPLE",
            "sa-east-1",
            "PUT",
            "/posts/123/foto.png",
            "",
            "meu-bucket.s3.sa-east-1.amazonaws.com",
            b"conteudo do arquivo",
            "20261031T120000Z",
            "20261031",
        );
        assert!(auth.starts_with(
            "AWS4-HMAC-SHA256 Credential=AKIAEXAMPLE/20261031/sa-east-1/s3/aws4_request"
        ));
        assert!(auth.contains("SignedHeaders=host;x-amz-content-sha256;x-amz-date"));
        assert!(auth.contains("Signature="));

        // Test path style (ex: MinIO)
        let mut cfg_path = s3.config.clone();
        cfg_path.force_path_style = true;
        cfg_path.endpoint = "http://localhost:9000".to_string();
        let s3_path = S3Storage::new(cfg_path);
        let (h, u) = s3_path.bucket_url();
        assert_eq!(h, "localhost:9000");
        assert_eq!(u, "http://localhost:9000/meu-bucket");
    }
}
