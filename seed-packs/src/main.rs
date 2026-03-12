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
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
struct AgentSeed {
    capability: String,
    name: String,
    description: String,
    pack_hashes: Vec<String>,
}

fn capability_from_filename(stem: &str) -> &str {
    match stem {
        "general" => "general",
        "crypto" => "crypto",
        "image_gen" => "image_gen",
        "video_gen" => "video_gen",
        "discord_moderator" => "discord_moderator",
        "telegram_moderator" => "telegram_moderator",
        other => other,
    }
}

fn load_existing_hashes(config_path: &Path) -> HashMap<String, Vec<String>> {
    let mut map = HashMap::new();
    if let Ok(content) = std::fs::read_to_string(config_path) {
        for line in content.lines() {
            let line = line.trim();
            if line.starts_with("AgentSeed(") {
                let cap = extract_field(line, "capability");
                let hashes = extract_hashes(line);
                if !cap.is_empty() {
                    map.insert(cap, hashes);
                }
            }
        }
    }
    map
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

fn extract_hashes(line: &str) -> Vec<String> {
    let mut hashes = Vec::new();
    if let Some(start) = line.find("pack_hashes: [") {
        let rest = &line[start + 14..];
        if let Some(end) = rest.find(']') {
            let inner = &rest[..end];
            for part in inner.split(',') {
                let h = part.trim().trim_matches('"').to_string();
                if !h.is_empty() && !h.contains("...") {
                    hashes.push(h);
                }
            }
        }
    }
    hashes
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

    let existing = load_existing_hashes(&config_path);

    let mut entries: Vec<PathBuf> = std::fs::read_dir(&packs_dir)
        .expect("Cannot read packs directory")
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().map(|e| e == "json").unwrap_or(false))
        .collect();
    entries.sort();

    let mut results: Vec<AgentSeed> = Vec::new();

    for path in &entries {
        let stem = path.file_stem().unwrap().to_str().unwrap();
        let capability = capability_from_filename(stem).to_string();

        // --only filter
        if let Some(ref only) = args.only {
            if &capability != only {
                // Keep existing hash if present
                if let Some(hashes) = existing.get(&capability) {
                    if !hashes.is_empty() {
                        let content = std::fs::read_to_string(path).unwrap_or_default();
                        let pack_def: serde_json::Value = serde_json::from_str(&content).unwrap_or_default();
                        results.push(AgentSeed {
                            capability,
                            name: pack_def["pack"]["name"].as_str().unwrap_or(stem).to_string(),
                            description: pack_def["pack"]["description"].as_str().unwrap_or("").to_string(),
                            pack_hashes: hashes.clone(),
                        });
                    }
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

        // Skip if we already have real hashes (unless --force)
        if !args.force {
            if let Some(hashes) = existing.get(&capability) {
                if !hashes.is_empty() {
                    println!("[{}] SKIP — already has hash: {}", capability, hashes[0]);
                    results.push(AgentSeed {
                        capability,
                        name: pack_name,
                        description: pack_desc,
                        pack_hashes: hashes.clone(),
                    });
                    continue;
                }
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
                    pack_hashes: vec![],
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
                    pack_hashes: vec![result.content_hash],
                });
            }
            Err(e) => {
                eprintln!("[{}] FAILED: {}", capability, e);
                results.push(AgentSeed {
                    capability,
                    name: pack_name,
                    description: pack_desc,
                    pack_hashes: vec![],
                });
            }
        }
    }

    write_seed_config(&config_path, &results);

    println!();
    println!("Seed config written to {}", config_path.display());
    println!();
    println!("Hashes:");
    for r in &results {
        let hash = r.pack_hashes.first().map(|h| h.as_str()).unwrap_or("NONE");
        println!("  {} -> {}", r.capability, hash);
    }
}

fn write_seed_config(path: &Path, agents: &[AgentSeed]) {
    let mut ron = String::from("StarflaskSeed(\n    agents: [\n");

    for agent in agents {
        let hashes: Vec<String> = agent.pack_hashes.iter().map(|h| format!("\"{}\"", h)).collect();
        ron.push_str(&format!(
            "        AgentSeed(capability: \"{}\", name: \"{}\", description: \"{}\", pack_hashes: [{}]),\n",
            agent.capability,
            agent.name.replace('"', "\\\""),
            agent.description.replace('"', "\\\""),
            hashes.join(", "),
        ));
    }

    ron.push_str("    ],\n)\n");

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).ok();
    }
    std::fs::write(path, &ron)
        .unwrap_or_else(|e| panic!("Failed to write {}: {}", path.display(), e));
}
