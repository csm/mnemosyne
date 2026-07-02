pub mod annotate;
pub mod eval;
pub mod lookup;
pub mod save;

pub use annotate::AnnotateTool;
pub use eval::EvalTool;
pub use lookup::LookupTool;
pub use save::SaveFunctionTool;

/// Deserialize tool arguments, mapping failures to a uniform error message.
pub(crate) fn parse_args<T: serde::de::DeserializeOwned>(
    args: serde_json::Value,
) -> Result<T, mnemosyne_mcp_core::CallToolResult> {
    serde_json::from_value(args)
        .map_err(|e| mnemosyne_mcp_core::CallToolResult::error(format!("invalid arguments: {e}")))
}
