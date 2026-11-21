//! A record of what `default_registry` registers, made before the function was split
//! (`tests/golden/block_registry.txt`; rewrite it on purpose with `KVASIR_BLESS=1`).
//!
//! Blocks that share one behaviour are listed together, numbered in the order of the first block
//! that has it, so the record does not depend on the order of the `register` calls. Fluids are
//! recorded the same way, by fluid id.

use std::collections::HashMap;
use std::fmt::Write;

use pumpkin_data::{Block, BlockId};

use super::registry::default_registry;

fn record() -> Result<String, std::fmt::Error> {
    let registry = default_registry();
    let mut out = String::new();

    let mut numbers: HashMap<usize, usize> = HashMap::new();
    let mut groups: Vec<Vec<&'static str>> = Vec::new();
    let mut without: Vec<&'static str> = Vec::new();
    let mut raw = 0u16;
    while let Some(id) = BlockId::new(raw) {
        raw += 1;
        let block = Block::from_id(id);
        match registry.get_pumpkin_block(id) {
            None => without.push(block.name),
            Some(behaviour) => {
                let key = std::sync::Arc::as_ptr(behaviour).cast::<()>() as usize;
                let next = groups.len();
                let number = *numbers.entry(key).or_insert(next);
                if number == groups.len() {
                    groups.push(Vec::new());
                }
                groups[number].push(block.name);
            }
        }
    }
    for (number, names) in groups.iter().enumerate() {
        writeln!(
            out,
            "behaviour {number} ({}): {}",
            names.len(),
            names.join(" ")
        )?;
    }
    writeln!(
        out,
        "no behaviour ({}): {}",
        without.len(),
        without.join(" ")
    )?;

    let mut fluid_numbers: HashMap<usize, usize> = HashMap::new();
    let mut fluid_groups: Vec<Vec<u16>> = Vec::new();
    for fluid_id in 0..64u16 {
        if let Some(fluid) = registry.get_pumpkin_fluid(fluid_id) {
            let key = std::sync::Arc::as_ptr(fluid).cast::<()>() as usize;
            let next = fluid_groups.len();
            let number = *fluid_numbers.entry(key).or_insert(next);
            if number == fluid_groups.len() {
                fluid_groups.push(Vec::new());
            }
            fluid_groups[number].push(fluid_id);
        }
    }
    for (number, ids) in fluid_groups.iter().enumerate() {
        writeln!(out, "fluid {number}: {ids:?}")?;
    }
    Ok(out)
}

#[test]
fn default_registry_registers_the_same_behaviours() -> Result<(), Box<dyn std::error::Error>> {
    let now = record()?;
    let path =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/block_registry.txt");
    if std::env::var_os("KVASIR_BLESS").is_some() {
        std::fs::write(&path, &now)?;
    } else {
        assert_eq!(std::fs::read_to_string(&path)?, now);
    }
    Ok(())
}
