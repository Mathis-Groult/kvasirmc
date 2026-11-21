//! A record of the Bedrock recipes `build` makes from every shipped crafting and stonecutting
//! recipe, made before the function was split into small ones
//! (`tests/golden/bedrock_recipes.txt`; rewrite it on purpose with `KVASIR_BLESS=1`).
//!
//! Consecutive recipes of one kind, block, size and output form a group: one line with the range
//! of network ids, the count, the output and an FNV-1a hash of the whole Debug text of the group.

use std::fmt::Write;

use pumpkin_protocol::bedrock::client::BedrockRecipe;

/// FNV-1a: a hash that does not change with the Rust version.
fn fnv(hash: u64, bytes: &[u8]) -> u64 {
    bytes.iter().fold(hash, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x0100_0000_01b3)
    })
}

const FNV_START: u64 = 0xcbf2_9ce4_8422_2325;

/// What groups consecutive recipes: kind, block, size and the first output.
fn group_key(recipe: &BedrockRecipe) -> String {
    match recipe {
        BedrockRecipe::Shaped(shaped) => {
            let out = &shaped.output[0];
            format!(
                "shaped {} {}x{} out id {} x{} aux {} block_state {}",
                shaped.block,
                shaped.width.0,
                shaped.height.0,
                out.id.0,
                out.stack_size,
                out.aux_value.0,
                out.block_runtime_id.0
            )
        }
        BedrockRecipe::Shapeless(shapeless) => {
            let out = &shapeless.output[0];
            format!(
                "shapeless {} out id {} x{} aux {} block_state {}",
                shapeless.block, out.id.0, out.stack_size, out.aux_value.0, out.block_runtime_id.0
            )
        }
    }
}

fn network_id(recipe: &BedrockRecipe) -> u32 {
    match recipe {
        BedrockRecipe::Shaped(shaped) => shaped.recipe_network_id.0,
        BedrockRecipe::Shapeless(shapeless) => shapeless.recipe_network_id.0,
    }
}

fn render(recipes: &[BedrockRecipe]) -> String {
    let mut text = String::new();
    let _ = writeln!(text, "recipes {}", recipes.len());
    let mut start = 0;
    while start < recipes.len() {
        let key = group_key(&recipes[start]);
        let mut end = start;
        let mut hash = FNV_START;
        while end < recipes.len() && group_key(&recipes[end]) == key {
            hash = fnv(hash, format!("{:?}", recipes[end]).as_bytes());
            end += 1;
        }
        let _ = writeln!(
            text,
            "ids {}..{} n={} {key} hash {hash:016x}",
            network_id(&recipes[start]),
            network_id(&recipes[end - 1]),
            end - start
        );
        start = end;
    }
    text
}

#[test]
fn static_recipes() -> Result<(), Box<dyn std::error::Error>> {
    let now = render(super::static_recipes());
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/golden")
        .join("bedrock_recipes.txt");
    if std::env::var_os("KVASIR_BLESS").is_some() {
        std::fs::create_dir_all(path.parent().ok_or("golden path has no parent")?)?;
        std::fs::write(&path, &now)?;
        return Ok(());
    }
    assert_eq!(std::fs::read_to_string(&path)?, now);
    Ok(())
}
