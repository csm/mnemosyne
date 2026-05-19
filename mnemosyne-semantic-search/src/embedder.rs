use candle_core::{DType, Device, IndexOp, Tensor};
use candle_nn::VarBuilder;
use candle_transformers::models::bert::{BertModel, Config as BertConfig};
use hf_hub::api::sync::Api;
use mnemosyne_code_search::IndexedFunction;
use tokenizers::Tokenizer;

use crate::error::SemanticSearchError;

/// Output dimension shared by all supported models (all are 768-d).
pub const DIMENSION: usize = 768;

/// Pooling strategy used to reduce the per-token sequence output to a single
/// sentence vector.
#[derive(Debug, Clone, Copy)]
pub enum Pooling {
    /// Take the CLS token (index 0). Recommended for BGE retrieval models.
    Cls,
    /// Masked mean over non-padding tokens. Recommended for Jina / Nomic.
    Mean,
}

/// Which embedding model to load from HuggingFace Hub.
///
/// All three produce 768-d vectors so they are drop-in replacements for
/// experimentation. The HuggingFace repo ID and pooling strategy differ.
#[derive(Debug, Clone, Copy, Default)]
pub enum EmbedModel {
    /// `BAAI/bge-base-en-v1.5` — standard BERT, CLS pooling, strong retrieval.
    /// Best default: fully supported by candle's BertModel out of the box.
    #[default]
    BgeBase,
    /// `jinaai/jina-embeddings-v2-base-code` — code-specific, mean pooling.
    /// Uses ALiBi positional encoding; candle's BertModel will approximate
    /// this with learned positions, which is adequate for shorter snippets.
    JinaCodeV2,
    /// `nomic-ai/nomic-embed-text-v1.5` — strong general-purpose, mean pooling.
    NomicText,
}

impl EmbedModel {
    pub fn hf_id(self) -> &'static str {
        match self {
            EmbedModel::BgeBase => "BAAI/bge-base-en-v1.5",
            EmbedModel::JinaCodeV2 => "jinaai/jina-embeddings-v2-base-code",
            EmbedModel::NomicText => "nomic-ai/nomic-embed-text-v1.5",
        }
    }

    fn pooling(self) -> Pooling {
        match self {
            EmbedModel::BgeBase => Pooling::Cls,
            EmbedModel::JinaCodeV2 => Pooling::Mean,
            EmbedModel::NomicText => Pooling::Mean,
        }
    }
}

pub struct Embedder {
    model: BertModel,
    tokenizer: Tokenizer,
    pooling: Pooling,
    device: Device,
}

impl Embedder {
    /// Download (or load from cache) the model and tokenizer, then initialise
    /// the candle BERT model on CPU.
    pub fn new(which: EmbedModel) -> Result<Self, SemanticSearchError> {
        let device = Device::Cpu;

        let api = Api::new().map_err(|e| SemanticSearchError::Embed(e.to_string()))?;
        let repo = api.model(which.hf_id().to_string());

        let tokenizer_path = repo
            .get("tokenizer.json")
            .map_err(|e| SemanticSearchError::Embed(e.to_string()))?;
        let config_path = repo
            .get("config.json")
            .map_err(|e| SemanticSearchError::Embed(e.to_string()))?;
        let weights_path = repo
            .get("model.safetensors")
            .map_err(|e| SemanticSearchError::Embed(e.to_string()))?;

        let tokenizer = Tokenizer::from_file(tokenizer_path)
            .map_err(|e| SemanticSearchError::Embed(e.to_string()))?;

        let config: BertConfig = serde_json::from_str(&std::fs::read_to_string(config_path)?)?;

        let vb = unsafe {
            VarBuilder::from_mmaped_safetensors(&[weights_path], DType::F32, &device)
                .map_err(|e| SemanticSearchError::Embed(e.to_string()))?
        };

        let model =
            BertModel::load(vb, &config).map_err(|e| SemanticSearchError::Embed(e.to_string()))?;

        Ok(Self {
            model,
            tokenizer,
            pooling: which.pooling(),
            device,
        })
    }

    /// Embed a batch of strings. Returns one L2-unnormalised vector per input.
    pub fn embed(&self, texts: Vec<String>) -> Result<Vec<Vec<f32>>, SemanticSearchError> {
        let encoded = self
            .tokenizer
            .encode_batch(texts, true)
            .map_err(|e| SemanticSearchError::Embed(e.to_string()))?;

        let input_ids: Vec<Vec<u32>> = encoded.iter().map(|e| e.get_ids().to_vec()).collect();
        let token_type_ids: Vec<Vec<u32>> =
            encoded.iter().map(|e| e.get_type_ids().to_vec()).collect();
        let attention_mask: Vec<Vec<u32>> = encoded
            .iter()
            .map(|e| e.get_attention_mask().to_vec())
            .collect();

        let ids = Tensor::new(input_ids, &self.device)
            .map_err(|e| SemanticSearchError::Embed(e.to_string()))?;
        let type_ids = Tensor::new(token_type_ids, &self.device)
            .map_err(|e| SemanticSearchError::Embed(e.to_string()))?;
        let mask = Tensor::new(attention_mask, &self.device)
            .map_err(|e| SemanticSearchError::Embed(e.to_string()))?;

        let output = self
            .model
            .forward(&ids, &type_ids, Some(&mask))
            .map_err(|e| SemanticSearchError::Embed(e.to_string()))?;
        // output: (batch, seq_len, hidden_size)

        let e2s = |e: candle_core::Error| SemanticSearchError::Embed(e.to_string());

        let embeddings = match self.pooling {
            Pooling::Cls => output.i((.., 0usize)).map_err(e2s)?,

            Pooling::Mean => {
                let mask_f = mask
                    .unsqueeze(2)
                    .and_then(|m| m.to_dtype(DType::F32))
                    .map_err(e2s)?;
                let sum = output
                    .to_dtype(DType::F32)
                    .and_then(|o| o.broadcast_mul(&mask_f))
                    .and_then(|o| o.sum(1))
                    .map_err(e2s)?;
                let count = mask_f.sum(1).map_err(e2s)?;
                sum.broadcast_div(&count).map_err(e2s)?
            }
        };

        embeddings.to_vec2::<f32>().map_err(e2s)
    }
}

/// Format an `IndexedFunction` as the text to be embedded.
///
/// The name + docstring prefix gives the model a natural-language handle on
/// the function's purpose; including the body lets intent-based queries match
/// on what the function actually does.
pub fn embed_text(f: &IndexedFunction) -> String {
    let doc = f.docstring.as_deref().unwrap_or("");
    let body = if f.body.len() > 2048 {
        &f.body[..2048]
    } else {
        &f.body
    };
    if doc.is_empty() {
        format!("{}\n{}", f.name, body)
    } else {
        format!("{}: {}\n{}", f.name, doc, body)
    }
}
