//! seed-packs — Provisions Starkbot agent packs into Axoniac
//! and writes the resulting content hashes to config/starflask_seed.ron.
//!
//! This only affects Axoniac (global pack registry). No Starflask agents
//! are created or modified. Users later install these packs onto their
//! own agents via Starflask.
//!
//! Usage:
//!   AXONIAC_API_KEY=ax_... cargo run -p seed-packs                    # provision new only
//!   AXONIAC_API_KEY=ax_... cargo run -p seed-packs -- --force         # re-provision all
//!   AXONIAC_API_KEY=ax_... cargo run -p seed-packs -- --only crypto   # re-provision one

use axoniac::{Axoniac, ProvisionPackRequest};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
struct AgentSeed {
    capability: String,
    name: String,
    description: String,
    pack_hash: String,
}

fn capability_from_filename(stem: &str) -> &str {
    match stem {
        "general" => "general",
        "crypto" => "crypto",
        "image_gen" => "image_gen",
        "video_gen" => "video_gen",
        "video_gen_ltx_t2v" => "video_gen_ltx_t2v",
        "video_gen_ltx_i2v" => "video_gen_ltx_i2v",
        "discord_moderator" => "discord_moderator",
        "telegram_moderator" => "telegram_moderator",
        other => other,
    }
}

fn load_existing_entries(config_path: &Path) -> Vec<AgentSeed> {
    let mut entries = Vec::new();
    if let Ok(content) = std::fs::read_to_string(config_path) {
        for line in content.lines() {
            let line = line.trim();
            if line.starts_with("AgentSeed(") {
                let cap = extract_field(line, "capability");
                let hash = extract_field(line, "pack_hash");
                if !cap.is_empty() {
                    entries.push(AgentSeed {
                        capability: cap,
                        name: extract_field(line, "name"),
                        description: extract_field(line, "description"),
                        pack_hash: if hash.contains("...") { String::new() } else { hash },
                    });
                }
            }
        }
    }
    entries
}

fn extract_field(line: &str, field: &str) -> String {
    let pattern = format!("{}: \"", field);
    if let Some(start) = line.find(&pattern) {
        let rest = &line[start + pattern.len()..];
        if let Some(end) = rest.find('"') {
            return rest[..end].to_string();
        }
    }
    String::new()
}

fn find_packs_dir() -> PathBuf {
    for candidate in ["seed-packs/packs", "packs", "../seed-packs/packs"] {
        let p = PathBuf::from(candidate);
        if p.is_dir() {
            return p;
        }
    }
    panic!("Cannot find packs directory. Run from repo root or seed-packs/");
}

fn find_config_path() -> PathBuf {
    for candidate in ["config/starflask_seed.ron", "../config/starflask_seed.ron"] {
        let p = PathBuf::from(candidate);
        if p.exists() || p.parent().map(|d| d.is_dir()).unwrap_or(false) {
            return p;
        }
    }
    PathBuf::from("config/starflask_seed.ron")
}

struct Args {
    force: bool,
    only: Option<String>,
}

fn parse_args() -> Args {
    let args: Vec<String> = std::env::args().collect();
    let mut force = false;
    let mut only = None;

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--force" | "-f" => force = true,
            "--only" | "-o" => {
                i += 1;
                if i < args.len() {
                    only = Some(args[i].clone());
                } else {
                    eprintln!("--only requires a capability name");
                    std::process::exit(1);
                }
            }
            "--help" | "-h" => {
                println!("Usage: seed-packs [OPTIONS]");
                println!();
                println!("Options:");
                println!("  --force, -f              Re-provision all packs (ignore existing hashes)");
                println!("  --only, -o <capability>  Only provision this capability");
                println!();
                println!("Environment:");
                println!("  AXONIAC_API_KEY          Required. Your Axoniac API key (ax_...)");
                println!("  AXONIAC_BASE_URL         Optional. Custom Axoniac API URL");
                std::process::exit(0);
            }
            _ => {
                eprintln!("Unknown argument: {}", args[i]);
                std::process::exit(1);
            }
        }
        i += 1;
    }

    Args { force, only }
}

#[tokio::main]
async fn main() {
    let args = parse_args();

    let api_key = std::env::var("AXONIAC_API_KEY")
        .expect("AXONIAC_API_KEY env var required");
    let base_url = std::env::var("AXONIAC_BASE_URL").ok();

    let ax = Axoniac::new(&api_key, base_url.as_deref())
        .expect("Failed to create Axoniac client");

    let packs_dir = find_packs_dir();
    let config_path = find_config_path();

    println!("Packs directory: {}", packs_dir.display());
    println!("Config output:   {}", config_path.display());
    if args.force {
        println!("Mode:            --force (re-provision all)");
    }
    if let Some(ref only) = args.only {
        println!("Filter:          --only {}", only);
    }
    println!();

    let existing_entries = load_existing_entries(&config_path);
    let existing: HashMap<String, String> = existing_entries.iter()
        .filter(|e| !e.pack_hash.is_empty())
        .map(|e| (e.capability.clone(), e.pack_hash.clone()))
        .collect();

    let mut pack_files: Vec<PathBuf> = std::fs::read_dir(&packs_dir)
        .expect("Cannot read packs directory")
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().map(|e| e == "json").unwrap_or(false))
        .collect();
    pack_files.sort();

    let mut results: Vec<AgentSeed> = Vec::new();

    for path in &pack_files {
        let stem = path.file_stem().unwrap().to_str().unwrap();
        let capability = capability_from_filename(stem).to_string();

        // --only filter: keep existing entry for non-targeted capabilities
        if let Some(ref only) = args.only {
            if &capability != only {
                if let Some(hash) = existing.get(&capability) {
                    let content = std::fs::read_to_string(path).unwrap_or_default();
                    let pack_def: serde_json::Value = serde_json::from_str(&content).unwrap_or_default();
                    results.push(AgentSeed {
                        capability,
                        name: pack_def["pack"]["name"].as_str().unwrap_or(stem).to_string(),
                        description: pack_def["pack"]["description"].as_str().unwrap_or("").to_string(),
                        pack_hash: hash.clone(),
                    });
                }
                continue;
            }
        }

        let content = std::fs::read_to_string(path)
            .unwrap_or_else(|e| panic!("Failed to read {}: {}", path.display(), e));
        let pack_def: serde_json::Value = serde_json::from_str(&content)
            .unwrap_or_else(|e| panic!("Invalid JSON in {}: {}", path.display(), e));

        let pack_name = pack_def["pack"]["name"].as_str().unwrap_or(stem).to_string();
        let pack_desc = pack_def["pack"]["description"].as_str().unwrap_or("").to_string();

        // Skip if we already have a hash (unless --force)
        if !args.force {
            if let Some(hash) = existing.get(&capability) {
                println!("[{}] SKIP — already has hash: {}", capability, hash);
                results.push(AgentSeed {
                    capability,
                    name: pack_name,
                    description: pack_desc,
                    pack_hash: hash.clone(),
                });
                continue;
            }
        }

        println!("[{}] Provisioning into Axoniac...", capability);

        let provision_req: ProvisionPackRequest = match serde_json::from_value(pack_def) {
            Ok(r) => r,
            Err(e) => {
                eprintln!("[{}] FAILED to parse pack definition: {}", capability, e);
                results.push(AgentSeed {
                    capability,
                    name: pack_name,
                    description: pack_desc,
                    pack_hash: String::new(),
                });
                continue;
            }
        };

        match ax.provision_pack(provision_req).await {
            Ok(result) => {
                let status = if result.created { "CREATED" } else { "EXISTS" };
                println!("[{}] {} — hash={}", capability, status, result.content_hash);
                results.push(AgentSeed {
                    capability,
                    name: pack_name,
                    description: pack_desc,
                    pack_hash: result.content_hash,
                });
            }
            Err(e) => {
                eprintln!("[{}] FAILED: {}", capability, e);
                results.push(AgentSeed {
                    capability,
                    name: pack_name,
                    description: pack_desc,
                    pack_hash: String::new(),
                });
            }
        }
    }

    // Preserve existing config entries that have no pack file (e.g. externally provisioned)
    let seen: std::collections::HashSet<String> = results.iter().map(|r| r.capability.clone()).collect();
    for entry in &existing_entries {
        if !seen.contains(&entry.capability) && !entry.pack_hash.is_empty() {
            results.push(entry.clone());
        }
    }

    write_seed_config(&config_path, &results);

    println!();
    println!("Seed config written to {}", config_path.display());
    println!();
    println!("Hashes:");
    for r in &results {
        let hash = if r.pack_hash.is_empty() { "NONE" } else { &r.pack_hash };
        println!("  {} -> {}", r.capability, hash);
    }
}

fn write_seed_config(path: &Path, agents: &[AgentSeed]) {
    let mut ron = String::from("StarflaskSeed(\n    agents: [\n");

    for agent in agents {
        ron.push_str(&format!(
            "        AgentSeed(capability: \"{}\", name: \"{}\", description: \"{}\", pack_hash: \"{}\"),\n",
            agent.capability,
            agent.name.replace('"', "\\\""),
            agent.description.replace('"', "\\\""),
            agent.pack_hash,
        ));
    }

    ron.push_str("    ],\n)\n");

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).ok();
    }
    std::fs::write(path, &ron)
        .unwrap_or_else(|e| panic!("Failed to write {}: {}", path.display(), e));
}
