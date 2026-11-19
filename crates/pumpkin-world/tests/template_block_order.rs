//! Card #95: an embedded structure template lists its blocks in the same order as the same NBT
//! file read from a datapack, which is vanilla's `StructureTemplate.buildInfoList` order (full
//! blocks, other blocks, block entities, each by y, x, z). Per-block random processors (block age,
//! lava, vines) draw in that order, so a different order builds a different structure.

use std::path::{Path, PathBuf};

use pumpkin_world::generation::structure::template::StructureTemplate;

fn nbt_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            nbt_files(&path, out);
        } else if path.extension().is_some_and(|ext| ext == "nbt") {
            out.push(path);
        }
    }
}

/// Each palette's blocks as `(pos, state name, properties, has block entity)`, in list order.
fn layout(template: &StructureTemplate) -> Vec<Vec<String>> {
    template
        .palettes
        .iter()
        .map(|palette| {
            palette
                .blocks()
                .iter()
                .map(|block| {
                    let mut properties: Vec<String> = block
                        .state
                        .properties
                        .iter()
                        .map(|property| format!("{property:?}"))
                        .collect();
                    properties.sort();
                    format!(
                        "{},{},{} {} {properties:?} {}",
                        block.pos.x,
                        block.pos.y,
                        block.pos.z,
                        block.state.name,
                        block.nbt.is_some()
                    )
                })
                .collect()
        })
        .collect()
}

#[test]
fn embedded_templates_list_blocks_in_datapack_order() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../assets/datapack/data/minecraft/structure");
    let mut files = Vec::new();
    nbt_files(&root, &mut files);
    files.sort();
    let mut compared = 0;
    let mut reordered = Vec::new();
    let mut different = Vec::new();
    for path in files {
        let Ok(relative) = path.strip_prefix(&root) else {
            continue;
        };
        let key = format!(
            "minecraft:{}",
            relative
                .with_extension("")
                .to_string_lossy()
                .replace('\\', "/")
        );
        let Some(embedded) = pumpkin_data::structure_template::get_structure_template(&key) else {
            continue;
        };
        let (Ok(bytes), Ok(fixed)) = (
            std::fs::read(&path),
            StructureTemplate::from_static(embedded),
        ) else {
            different.push(format!("{key}: unreadable"));
            continue;
        };
        let Ok(read) = StructureTemplate::from_nbt_bytes(&bytes) else {
            different.push(format!("{key}: datapack NBT unreadable"));
            continue;
        };
        compared += 1;
        let (from_nbt, from_static) = (layout(&read), layout(&fixed));
        if from_nbt != from_static {
            let mut a: Vec<_> = from_nbt.concat();
            let mut b: Vec<_> = from_static.concat();
            a.sort();
            b.sort();
            if a == b {
                reordered.push(key);
            } else {
                different.push(key);
            }
        }
    }
    assert!(compared > 100, "only {compared} templates compared");
    assert!(
        reordered.is_empty() && different.is_empty(),
        "{} of {compared} embedded templates list their blocks in another order than the datapack \
         NBT ({:?}..), {} differ in content ({:?}..)",
        reordered.len(),
        reordered.iter().take(5).collect::<Vec<_>>(),
        different.len(),
        different.iter().take(5).collect::<Vec<_>>(),
    );
}
