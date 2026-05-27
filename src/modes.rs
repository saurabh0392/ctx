//! Named context modes: bundle profile + inject + coaching + adaptive toggles.

use anyhow::{anyhow, Result};
use std::collections::HashMap;

use crate::config::{Config, ModeConfig};

pub fn list_modes(cfg: &Config) -> Vec<String> {
    let mut names: Vec<_> = cfg.modes.keys().cloned().collect();
    names.sort();
    names
}

pub fn show_mode(cfg: &Config, name: &str) -> Result<()> {
    let m = cfg
        .modes
        .get(name)
        .ok_or_else(|| anyhow!("unknown mode '{name}'"))?;
    print_mode(name, m);
    Ok(())
}

fn print_mode(name: &str, m: &ModeConfig) {
    println!("mode: {name}");
    println!("  profile: {}", m.profile);
    println!("  inject_enabled: {}", m.inject_enabled);
    println!("  coaching_enabled: {}", m.coaching_enabled);
    println!("  adaptive_prefix_enabled: {}", m.adaptive_prefix_enabled);
}

pub fn switch_mode(name: &str) -> Result<()> {
    let mut cfg = Config::load();
    let mode = cfg
        .modes
        .get(name)
        .cloned()
        .ok_or_else(|| anyhow!("unknown mode '{name}'"))?;
    cfg.active_mode = Some(name.to_string());
    cfg.active_profile = Some(mode.profile.clone());
    cfg.inject_enabled = mode.inject_enabled;
    cfg.coaching_enabled = mode.coaching_enabled;
    cfg.adaptive_prefix_enabled = mode.adaptive_prefix_enabled;
    cfg.save()?;
    crate::profiles::apply_profile(&mode.profile, true, true)?;
    println!("Switched to mode '{name}' (profile={})", mode.profile);
    Ok(())
}

pub fn save_current_as_mode(name: &str) -> Result<()> {
    let mut cfg = Config::load();
    let profile = cfg.active_profile.clone().unwrap_or_else(|| "all".to_string());
    cfg.modes.insert(
        name.to_string(),
        ModeConfig {
            profile,
            inject_enabled: cfg.inject_enabled,
            coaching_enabled: cfg.coaching_enabled,
            adaptive_prefix_enabled: cfg.adaptive_prefix_enabled,
        },
    );
    cfg.save()?;
    println!("Saved current settings as mode '{name}'");
    Ok(())
}

pub fn modes_map(cfg: &Config) -> HashMap<String, ModeConfig> {
    cfg.modes.clone()
}
