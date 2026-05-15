use crate::{embedded, Result};
use execution_engine::ClojureRuntime;

/// Load all embedded core namespaces into `runtime`.
pub fn load_core(runtime: &mut ClojureRuntime) -> Result<()> {
    runtime.load_string(embedded::CORE_CLJ)?;
    runtime.load_string(embedded::TEMPLATES_CLJ)?;
    Ok(())
}
