use super::*;

fn textured_profile() -> ProfileImpl {
    ProfileImpl {
        name: Some("Notch".to_string()),
        properties: vec![ProfileProperty {
            name: "textures".to_string(),
            value: "eyJ0ZXh0dXJlcyI6e319".to_string(),
            signature: None,
        }],
        ..Default::default()
    }
}

/// The wire form vanilla expects for `textured_profile`.
fn textured_profile_bytes() -> Vec<u8> {
    let mut bytes = vec![
        0, // partial profile tag
        1, 5, // name present, 5 bytes
    ];
    bytes.extend_from_slice(b"Notch");
    bytes.push(0); // no id
    bytes.push(1); // one property
    bytes.push(8); // property name, 8 bytes
    bytes.extend_from_slice(b"textures");
    bytes.push(20); // property value, 20 bytes
    bytes.extend_from_slice(b"eyJ0ZXh0dXJlcyI6e319");
    bytes.push(0); // no signature
    bytes.extend_from_slice(&[0, 0, 0, 0]); // no texture, cape, elytra, model
    bytes
}

#[test]
fn partial_profile_is_written_with_a_false_tag() {
    let mut encoded = Vec::new();
    textured_profile().serialize(&mut encoded).unwrap();
    assert_eq!(encoded, textured_profile_bytes());
}

#[test]
fn partial_profile_is_read_as_name_then_id() {
    let decoded = ProfileImpl::deserialize(&mut textured_profile_bytes().as_slice()).unwrap();
    assert_eq!(decoded, textured_profile());
}

#[test]
fn full_profile_is_read_as_id_then_name() {
    let id = uuid::Uuid::from_u128(0x0000_0001_0000_0002_0000_0003_0000_0004);
    let mut encoded = vec![1];
    encoded.extend_from_slice(id.as_bytes());
    encoded.push(5);
    encoded.extend_from_slice(b"Notch");
    encoded.extend_from_slice(&[0, 0, 0, 0, 0]);

    let decoded = ProfileImpl::deserialize(&mut encoded.as_slice()).unwrap();
    assert_eq!(decoded.name.as_deref(), Some("Notch"));
    assert_eq!(decoded.id, Some([1, 2, 3, 4]));
}

#[test]
fn variant_components_use_vanilla_network_ids() {
    let mut encoded = Vec::new();
    SalmonSizeImpl {
        value: Cow::Borrowed("large"),
    }
    .serialize(&mut encoded)
    .unwrap();
    TropicalFishPatternImpl {
        value: Cow::Borrowed("stripey"),
    }
    .serialize(&mut encoded)
    .unwrap();
    SheepColorImpl {
        value: Cow::Borrowed("red"),
    }
    .serialize(&mut encoded)
    .unwrap();
    // Holders send the registry id plus one.
    PaintingVariantImpl {
        value: Cow::Borrowed("minecraft:alban"),
    }
    .serialize(&mut encoded)
    .unwrap();
    assert_eq!(encoded, [2, 0x81, 0x02, 14, 1]);

    let clamped = SalmonSizeImpl::deserialize(&mut [9u8].as_slice()).unwrap();
    assert_eq!(clamped.value, "large");

    let min_holder = [0x80, 0x80, 0x80, 0x80, 0x08];
    assert!(PaintingVariantImpl::deserialize(&mut min_holder.as_slice()).is_err());
}

#[test]
fn all_default_item_components_serialize_and_deserialize() {
    for id in 0..=2000 {
        let Some(item) = pumpkin_data::item::Item::from_id(id) else {
            continue;
        };
        for (comp, val) in item.components {
            if *comp == DataComponent::EntityData {
                // Static Item constants use nbt: None because NbtCompound is not const-constructible.
                continue;
            }
            let mut buf = Vec::new();
            serialize(*comp, *val, &mut buf).unwrap_or_else(|e| {
                panic!("Failed to serialize {comp:?} on {}: {e}", item.registry_key)
            });
            let mut slice = buf.as_slice();
            let _decoded = deserialize(*comp, &mut slice).unwrap_or_else(|e| {
                panic!(
                    "Failed to deserialize {comp:?} on {}: {e}",
                    item.registry_key
                )
            });
            assert!(
                slice.is_empty(),
                "Trailing bytes after deserializing {comp:?} on {}: {} bytes left",
                item.registry_key,
                slice.len()
            );
        }
    }
}

#[test]
fn oversized_tool_rule_count_is_rejected_without_preallocating() {
    for count in [
        [0xFF, 0xFF, 0xFF, 0xFF, 0x07],
        [0xFF, 0xFF, 0xFF, 0xFF, 0x0F],
    ] {
        assert!(ToolImpl::deserialize(&mut count.as_slice()).is_err());
    }
}
