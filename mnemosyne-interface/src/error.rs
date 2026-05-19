use thiserror::Error;

#[derive(Debug, Error)]
pub enum InterfaceError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("serialization error: {0}")]
    Serde(#[from] serde_json::Error),
    #[error("inference error: {0}")]
    Inference(#[from] mnemosyne_inference_engine::InferenceError),
}
