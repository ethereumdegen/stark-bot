//! Module registry — holds all available modules (dynamically loaded)

use super::loader;
use super::Module;
use std::collections::HashMap;

/// Registry of all available modules (dynamically loaded from ~/.starkbot/modules/)
pub struct ModuleRegistry {
    modules: HashMap<String, Box<dyn Module>>,
}

impl ModuleRegistry {
    /// Create a new registry with dynamically loaded modules
    /// from `~/.starkbot/modules/`.
    pub fn new() -> Self {
        let mut reg = Self {
            modules: HashMap::new(),
        };

        // Dynamic modules from ~/.starkbot/modules/
        let dynamic = loader::load_dynamic_modules();
        for module in dynamic {
            let name = module.name().to_string();
            log::info!("[MODULE] Registered dynamic module: {}", name);
            reg.modules.insert(name, Box::new(module));
        }

        reg
    }

    /// Get a module by name
    pub fn get(&self, name: &str) -> Option<&dyn Module> {
        self.modules.get(name).map(|m| m.as_ref())
    }

    /// List all available modules
    pub fn available_modules(&self) -> Vec<&dyn Module> {
        self.modules.values().map(|m| m.as_ref()).collect()
    }
}
