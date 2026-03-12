//! Starflask seed configuration — maps capabilities to Axoniac agent pack hashes.

use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct StarflaskSeed {
    pub agents: Vec<AgentSeed>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AgentSeed {
    pub capability: String,
    pub name: String,
    pub description: String,
    pub pack_hashes: Vec<String>,
}

impl StarflaskSeed {
    /// Load seed config from `config/starflask_seed.ron`.
    pub fn load() -> Option<Self> {
        let paths = [
            std::path::Path::new("./config/starflask_seed.ron"),
            std::path::Path::new("../config/starflask_seed.ron"),
        ];
        for path in &paths {
            if path.exists() {
                let contents = std::fs::read_to_string(path).ok()?;
                match ron::from_str::<StarflaskSeed>(&contents) {
                    Ok(seed) => return Some(seed),
                    Err(e) => {
                        log::error!("Failed to parse starflask_seed.ron: {}", e);
                        return None;
                    }
                }
            }
        }
        log::debug!("No starflask_seed.ron found");
        None
    }
}
