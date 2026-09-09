use zed_extension_api as zed;

struct AstroProbeExtension;

impl zed::Extension for AstroProbeExtension {
    fn new() -> Self {
        Self
    }

    fn language_server_command(
        &mut self,
        _language_server_id: &zed::LanguageServerId,
        worktree: &zed::Worktree,
    ) -> Result<zed::Command, String> {
        // Find Node.js path managed by Zed
        let node_binary = zed::node_binary_path()?;

        // Retrieve location of extension bundle directory on host via current WASI directory
        let extension_dir = std::env::current_dir()
            .map_err(|e| format!("Failed to get extension directory: {}", e))?;

        // Locate the bundled JS adapter
        let adapter_path = extension_dir
            .join("lsp-adapter.js")
            .to_string_lossy()
            .to_string();

        // Run the LSP Adapter under node, passing the workspace root path as an argument
        Ok(zed::Command {
            command: node_binary,
            args: vec![adapter_path, worktree.root_path().to_string()],
            env: {
                let mut env = vec![];
                if let Ok(val) = std::env::var("PATH") {
                    env.push(("PATH".to_string(), val));
                }
                if let Ok(val) = std::env::var("ASTRO_PROBE_URL") {
                    env.push(("ASTRO_PROBE_URL".to_string(), val));
                }
                env
            },
        })
    }
}

zed::register_extension!(AstroProbeExtension);
