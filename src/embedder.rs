//! Local session embeddings: 384-dimensional float vectors for cosine similarity.
//!
//! With the `onnx` feature enabled and model files present at `~/.ctx/models/`,
//! uses all-MiniLM-L6-v2 via ONNX Runtime for semantic similarity.
//! Falls back to a deterministic 384-d hash projection when model files are absent
//! or the `onnx` feature is not compiled in.

use anyhow::Result;
use ndarray::Array1;
use rusqlite::{Connection, OptionalExtension};

pub const EMBED_DIM: usize = 384;

pub fn compose_embed_text(first_message: &str, working_directory: &str, profile: &str) -> String {
    let fm = first_message.trim();
    let wd = working_directory.trim();
    let pr = profile.trim();
    format!("[profile: {pr}] [dir: {wd}] {fm}")
}

fn normalize_vec(mut v: Vec<f32>) -> Vec<f32> {
    let n: f32 = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    if n < 1e-8 {
        return vec![0.0; v.len()];
    }
    for x in &mut v {
        *x /= n;
    }
    v
}

/// Deterministic 384-d embedding. Stable across runs for the same text.
pub fn embed_text_hash(text: &str) -> Vec<f32> {
    let mut v = vec![0f32; EMBED_DIM];
    let lower = text.to_lowercase();
    for w in lower.split_whitespace() {
        let h = fnv1a64(w.as_bytes());
        let i = (h % EMBED_DIM as u64) as usize;
        v[i] += 1.0;
    }
    for tri in lower.as_bytes().windows(3) {
        let h = fnv1a64(tri);
        let i = (h % EMBED_DIM as u64) as usize;
        v[i] += 0.25;
    }
    normalize_vec(v)
}

fn fnv1a64(data: &[u8]) -> u64 {
    const FNV_OFFSET: u64 = 14695981039346656037;
    const FNV_PRIME: u64 = 1099511628211;
    let mut h = FNV_OFFSET;
    for &b in data {
        h ^= b as u64;
        h = h.wrapping_mul(FNV_PRIME);
    }
    h
}

fn cosine_sim(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() {
        return 0.0;
    }
    let aa = Array1::from_vec(a.to_vec());
    let bb = Array1::from_vec(b.to_vec());
    aa.dot(&bb)
}

fn blob_to_vec(blob: &[u8]) -> Option<Vec<f32>> {
    if blob.len() != EMBED_DIM * 4 {
        return None;
    }
    let mut v = Vec::with_capacity(EMBED_DIM);
    for chunk in blob.chunks_exact(4) {
        v.push(f32::from_le_bytes(chunk.try_into().ok()?));
    }
    Some(v)
}

fn vec_to_blob(v: &[f32]) -> Vec<u8> {
    let mut b = Vec::with_capacity(v.len() * 4);
    for x in v {
        b.extend_from_slice(&x.to_le_bytes());
    }
    b
}

// ---------------------------------------------------------------------------
// ONNX MiniLM path (feature-gated)
// ---------------------------------------------------------------------------

#[cfg(feature = "onnx")]
mod onnx_impl {
    use super::*;
    use std::sync::Mutex;

    static SESSION: std::sync::LazyLock<Mutex<Option<OnnxEmbedder>>> =
        std::sync::LazyLock::new(|| Mutex::new(OnnxEmbedder::try_load().ok()));

    struct OnnxEmbedder {
        session: ort::session::Session,
        tokenizer: tokenizers::Tokenizer,
    }

    impl OnnxEmbedder {
        fn try_load() -> Result<Self> {
            let model_path = crate::config::minilm_onnx_path();
            let tok_path = crate::config::minilm_tokenizer_path();
            if !model_path.exists() || !tok_path.exists() {
                anyhow::bail!("model files not found");
            }
            let session = ort::session::Session::builder()
                .map_err(|e| anyhow::anyhow!("{e}"))?
                .with_intra_threads(1)
                .map_err(|e| anyhow::anyhow!("{e}"))?
                .commit_from_file(&model_path)
                .map_err(|e| anyhow::anyhow!("{e}"))?;
            let tokenizer = tokenizers::Tokenizer::from_file(&tok_path)
                .map_err(|e| anyhow::anyhow!("{e}"))?;
            Ok(Self { session, tokenizer })
        }

        fn embed(&mut self, text: &str) -> Result<Vec<f32>> {
            let encoding = self
                .tokenizer
                .encode(text, true)
                .map_err(|e| anyhow::anyhow!("{e}"))?;

            let ids: Vec<i64> = encoding.get_ids().iter().map(|&id| id as i64).collect();
            let mask: Vec<i64> = encoding
                .get_attention_mask()
                .iter()
                .map(|&m| m as i64)
                .collect();
            let type_ids: Vec<i64> = encoding
                .get_type_ids()
                .iter()
                .map(|&t| t as i64)
                .collect();
            let seq_len = ids.len();

            let ids_tensor = ort::value::Tensor::from_array(([1usize, seq_len], ids))
                .map_err(|e| anyhow::anyhow!("{e}"))?;
            let mask_tensor = ort::value::Tensor::from_array(([1usize, seq_len], mask))
                .map_err(|e| anyhow::anyhow!("{e}"))?;
            let type_tensor = ort::value::Tensor::from_array(([1usize, seq_len], type_ids))
                .map_err(|e| anyhow::anyhow!("{e}"))?;

            let inputs = ort::inputs![
                "input_ids" => ids_tensor,
                "attention_mask" => mask_tensor,
                "token_type_ids" => type_tensor
            ];
            let outputs = self.session.run(inputs)
                .map_err(|e| anyhow::anyhow!("{e}"))?;

            let (shape, data) = outputs[0]
                .try_extract_tensor::<f32>()
                .map_err(|e| anyhow::anyhow!("{e}"))?;
            // shape derefs to &[i64], expected [1, seq_len, 384]
            let tokens = if shape.len() == 3 { shape[1] as usize } else { 1 };
            let embed_dim = if shape.len() == 3 { shape[2] as usize } else { EMBED_DIM };
            let mut pooled = vec![0f32; embed_dim.min(EMBED_DIM)];
            for t in 0..tokens {
                for d in 0..pooled.len() {
                    let idx = t * embed_dim + d;
                    if idx < data.len() {
                        pooled[d] += data[idx];
                    }
                }
            }
            if tokens > 0 {
                let div = tokens as f32;
                for x in &mut pooled {
                    *x /= div;
                }
            }
            Ok(normalize_vec(pooled))
        }
    }

    pub fn embed_text_onnx(text: &str) -> Option<Vec<f32>> {
        let mut guard = SESSION.lock().ok()?;
        let embedder = guard.as_mut()?;
        embedder.embed(text).ok()
    }

    pub fn onnx_available() -> bool {
        SESSION.lock().map(|g| g.is_some()).unwrap_or(false)
    }
}

#[cfg(not(feature = "onnx"))]
mod onnx_impl {
    pub fn embed_text_onnx(_text: &str) -> Option<Vec<f32>> {
        None
    }

    pub fn onnx_available() -> bool {
        false
    }
}

pub fn onnx_available() -> bool {
    onnx_impl::onnx_available()
}

/// Primary embedding entry point.
/// Tries ONNX MiniLM first, falls back to hash.
pub fn embed_text(text: &str) -> Result<Vec<f32>> {
    if let Some(v) = onnx_impl::embed_text_onnx(text) {
        return Ok(v);
    }
    Ok(embed_text_hash(text))
}

// ---------------------------------------------------------------------------
// DB operations (unchanged)
// ---------------------------------------------------------------------------

pub fn embed_sessions_incremental(conn: &Connection) -> Result<usize> {
    if !crate::config::Config::load().embeddings_enabled() {
        return Ok(0);
    }
    crate::db::ensure_schema(conn)?;
    let mut stmt = conn.prepare(
        "SELECT s.id, s.embed_text FROM sessions s
         LEFT JOIN session_embeddings e ON e.session_id = s.id
         WHERE e.session_id IS NULL AND s.embed_text IS NOT NULL AND LENGTH(TRIM(s.embed_text)) > 0
         LIMIT 200",
    )?;
    let rows: Vec<(i64, String)> = stmt
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<Result<_, _>>()?;

    let mut n = 0usize;
    for (sid, etext) in rows {
        let vec = match embed_text(&etext) {
            Ok(v) => v,
            Err(_) => continue,
        };
        crate::db::set_session_embedding_blob(conn, sid, &vec_to_blob(&vec))?;
        n += 1;
    }
    Ok(n)
}

/// Re-embed all sessions (used when switching from hash to ONNX).
pub fn reembed_all_sessions(conn: &Connection) -> Result<usize> {
    if !crate::config::Config::load().embeddings_enabled() {
        return Ok(0);
    }
    crate::db::ensure_schema(conn)?;
    conn.execute("DELETE FROM session_embeddings", [])?;
    let mut stmt = conn.prepare(
        "SELECT id, embed_text FROM sessions WHERE embed_text IS NOT NULL AND LENGTH(TRIM(embed_text)) > 0",
    )?;
    let rows: Vec<(i64, String)> = stmt
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<Result<_, _>>()?;

    let mut n = 0usize;
    for (sid, etext) in rows {
        let vec = match embed_text(&etext) {
            Ok(v) => v,
            Err(_) => continue,
        };
        crate::db::set_session_embedding_blob(conn, sid, &vec_to_blob(&vec))?;
        n += 1;
    }
    Ok(n)
}

pub fn similar_sessions_by_query(
    conn: &Connection,
    query_embedding: &[f32],
    top_k: usize,
    exclude_session_pk: Option<i64>,
) -> Result<Vec<(i64, f32)>> {
    crate::db::ensure_schema(conn)?;
    let rows = crate::db::list_embedding_rows(conn)?;
    let mut scored: Vec<(i64, f32)> = Vec::new();
    for (sid, blob) in rows {
        if Some(sid) == exclude_session_pk {
            continue;
        }
        if let Some(v) = blob_to_vec(&blob) {
            scored.push((sid, cosine_sim(query_embedding, &v)));
        }
    }
    scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    scored.truncate(top_k);
    Ok(scored)
}

pub fn similar_sessions(conn: &Connection, session_pk: i64, top_k: usize) -> Result<Vec<(i64, f32)>> {
    crate::db::ensure_schema(conn)?;
    let self_blob: Option<Vec<u8>> = conn
        .query_row(
            "SELECT embedding FROM session_embeddings WHERE session_id = ?1",
            rusqlite::params![session_pk],
            |r| r.get(0),
        )
        .optional()?;
    let Some(blob) = self_blob else {
        return Ok(vec![]);
    };
    let self_v = match blob_to_vec(&blob) {
        Some(v) => v,
        None => return Ok(vec![]),
    };
    similar_sessions_by_query(conn, &self_v, top_k, Some(session_pk))
}

pub fn session_pk_for_external(conn: &Connection, external_key: &str) -> Result<Option<i64>> {
    crate::db::ensure_schema(conn)?;
    let r = conn.query_row(
        "SELECT id FROM sessions WHERE external_key = ?1",
        rusqlite::params![external_key],
        |row| row.get::<_, i64>(0),
    );
    match r {
        Ok(id) => Ok(Some(id)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(e.into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_embedding_is_unit_length() {
        let v = embed_text_hash("hello world");
        let n: f32 = v.iter().map(|x| x * x).sum::<f32>().sqrt();
        assert!((n - 1.0).abs() < 1e-3);
        assert_eq!(v.len(), EMBED_DIM);
    }

    #[test]
    fn same_text_same_embedding() {
        let a = embed_text_hash("fix the bug in ctx");
        let b = embed_text_hash("fix the bug in ctx");
        assert_eq!(a, b);
    }

    #[test]
    fn compose_embed_text_includes_parts() {
        let t = compose_embed_text("do the thing", "/proj/foo", "minimal");
        assert!(t.contains("minimal"));
        assert!(t.contains("/proj/foo"));
        assert!(t.contains("do the thing"));
    }

    #[test]
    fn embed_text_returns_hash_without_onnx() {
        let v = embed_text("test input").unwrap();
        assert_eq!(v.len(), EMBED_DIM);
        let n: f32 = v.iter().map(|x| x * x).sum::<f32>().sqrt();
        assert!((n - 1.0).abs() < 1e-3);
    }
}
