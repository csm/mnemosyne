use crate::{embedded, Result};
use mnemosyne_execution_engine::ClojureRuntime;

/// Load all embedded core namespaces into `runtime`.
pub fn load_core(runtime: &mut ClojureRuntime) -> Result<()> {
    runtime.load_string(embedded::CORE_CLJ)?;
    runtime.load_string(embedded::TEMPLATES_CLJ)?;
    Ok(())
}

/// Load the `mnemosyne.shell` namespace into `runtime`.
///
/// Kept separate from [`load_core`] because it only loads on a runtime whose
/// `IoPolicy` grants file IO (the shell utilities are defined over the async
/// IO substrate and the `mnemosyne.shell.native` builtins).
pub fn load_shell(runtime: &mut ClojureRuntime) -> Result<()> {
    runtime.load_string(embedded::SHELL_CLJ)?;
    Ok(())
}
