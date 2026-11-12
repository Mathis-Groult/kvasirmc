//! The packet mapping of Rust enums (one serialize, deserialize and to-WIT arm per variant).

use heck::{ToPascalCase, ToSnakeCase};
use syn::Fields;

use super::{MappingMode, convert_value, get_type_info};

pub(super) fn process_enum(
    e: syn::ItemEnum,
    state: &str,
    output: &mut String,
    attr_name: &str,
    variant_prefix: &str,
    rust_path_prefix: &str,
    mode: MappingMode,
) {
    if !matches!(e.vis, syn::Visibility::Public(_)) {
        return;
    }
    let enum_name = e.ident.to_string();
    let wit_case = if state == "play" {
        enum_name.to_pascal_case()
    } else {
        format!("{}{}", state, enum_name).to_pascal_case()
    };
    let enum_name_with_lt = if !e.generics.params.is_empty() {
        format!("{}<'_>", enum_name)
    } else {
        enum_name.clone()
    };
    let packet_ns = if rust_path_prefix.contains("java") {
        "java_packets"
    } else {
        "bedrock_packets"
    };
    let wit_ns = format!("crate::pumpkin::plugin::{}::{}", packet_ns, enum_name);

    match mode {
        MappingMode::Serialize => {
            output.push_str(&format!(
                "        {}::{}(data) => {{\n",
                variant_prefix, wit_case
            ));
            output.push_str("            let p = match data {\n");
            for v in &e.variants {
                if let Some(arm) = generate_serialize_arm(&enum_name, v, rust_path_prefix, &wit_ns)
                {
                    output.push_str(&arm);
                } else {
                    // variant isn't supported
                    return;
                }
            }
            output.push_str("            };\n");
            output.push_str("            let mut buf = Vec::new();\n");
            if attr_name == "java_packet" {
                output.push_str("            pumpkin_core::net::java::JavaClient::write_packet_for_version(&p, version, &mut buf).unwrap();\n");
            } else {
                output.push_str("            pumpkin_core::net::bedrock::BedrockClient::write_raw_packet(&p, &mut buf).unwrap();\n");
            }
            output.push_str("            Some(buf.into())\n");
            output.push_str("        }\n");
        }

        MappingMode::Deserialize => {
            if rust_path_prefix.contains("client") {
                return;
            }
            if rust_path_prefix.contains("java") {
                output.push_str(&format!(
                    "        id if id == {}::{}::to_id(version) => {{\n",
                    rust_path_prefix, enum_name
                ));
                output.push_str("            use pumpkin_protocol::ServerPacket;\n");
                output.push_str(&format!(
                    "            let p = <{}::{} as pumpkin_protocol::ServerPacket>::read(&mut payload, &version).ok()?;\n",
                    rust_path_prefix, enum_name
                ));
            } else {
                output.push_str(&format!(
                    "        id if id == <{}::{} as pumpkin_protocol::Packet>::PACKET_ID as i32 => {{\n",
                    rust_path_prefix, enum_name
                ));
                output.push_str("            use pumpkin_protocol::BServerPacket;\n");
                output.push_str(&format!(
                    "            let p = <{}::{} as pumpkin_protocol::BServerPacket>::read(&mut Cursor::new(payload)).ok()?;\n",
                    rust_path_prefix, enum_name
                ));
            }
            output.push_str(&format!("            Some(match p {{\n"));
            for v in &e.variants {
                if let Some(arm) = generate_deserialize_arm(
                    &rust_path_prefix,
                    &enum_name,
                    v,
                    variant_prefix,
                    &wit_case,
                    &wit_ns,
                ) {
                    output.push_str(&arm);
                } else {
                    return;
                }
            }
            output.push_str("            })\n");
            output.push_str("        }\n");
        }

        MappingMode::ToWit => {
            let side = if rust_path_prefix.contains("java") {
                "Java"
            } else {
                "Bedrock"
            };
            let wit_type = if rust_path_prefix.contains("java") {
                "ClientboundPacket"
            } else {
                "BClientboundPacket"
            };
            output.push_str(&format!(
                "impl ToWitClientbound{} for {}::{} {{\n",
                side, rust_path_prefix, enum_name_with_lt
            ));
            output.push_str(&format!("    fn to_wit(&self) -> {} {{\n", wit_type));
            output.push_str("        match self {\n");
            for v in &e.variants {
                if let Some(arm) = generate_to_wit_arm(v, variant_prefix, &wit_case, &wit_ns) {
                    output.push_str(&arm);
                } else {
                    return;
                }
            }
            output.push_str("        }\n");
            output.push_str("    }\n");
            output.push_str("}\n\n");
        }

        MappingMode::Downcast => {
            output.push_str(&format!(
                "    if let Some(p) = any.downcast_ref::<{}::{}>() {{\n",
                rust_path_prefix, enum_name
            ));
            output.push_str("        return Some(p.to_wit());\n");
            output.push_str("    }\n");
        }
    }
}

/// return the bindings of a variant : (pattern_destruct, vec of (binding_name, syn::Type))
fn variant_bindings(v: &syn::Variant) -> (String, Vec<(String, syn::Type)>) {
    match &v.fields {
        Fields::Unnamed(fields) => {
            let bindings: Vec<(String, syn::Type)> = fields
                .unnamed
                .iter()
                .map(|f| {
                    let (type_ident, _, _) = get_type_info(&f.ty);
                    (type_ident.to_snake_case(), f.ty.clone())
                })
                .collect();
            let pattern = format!(
                "({})",
                bindings
                    .iter()
                    .map(|(n, _)| n.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            );
            (pattern, bindings)
        }
        Fields::Named(fields) => {
            let bindings: Vec<(String, syn::Type)> = fields
                .named
                .iter()
                .map(|f| {
                    let name = f.ident.as_ref().unwrap().to_string();
                    (name, f.ty.clone())
                })
                .collect();
            let pattern = format!(
                "{{ {} }}",
                bindings
                    .iter()
                    .map(|(n, _)| n.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            );
            (pattern, bindings)
        }
        Fields::Unit => (String::new(), vec![]),
    }
}

/// Serialize : WitEnum::Variant(binding) => RustEnum::Variant(converted)
fn generate_serialize_arm(
    enum_name: &str,
    v: &syn::Variant,
    rust_path_prefix: &str,
    wit_ns: &str,
) -> Option<String> {
    let variant_name = v.ident.to_string();
    let (pattern, bindings) = variant_bindings(v);

    if bindings.is_empty() {
        return Some(format!(
            "                {}::{} => {}::{}::{},\n",
            wit_ns, variant_name, rust_path_prefix, enum_name, variant_name
        ));
    }

    let mut prep = String::new();
    let mut converted = Vec::new();
    for (binding, ty) in &bindings {
        let (type_ident, is_ref, is_slice) = get_type_info(ty);
        let (p, expr, ok) = convert_value(
            &type_ident,
            is_ref,
            is_slice,
            binding,
            Some(binding),
            MappingMode::Serialize,
            "                ",
        );
        if !ok {
            return None;
        }
        prep.push_str(&p);
        converted.push(expr);
    }

    Some(format!(
        "{}                {}::{}{} => {}::{}::{}({}),\n",
        prep,
        wit_ns,
        variant_name,
        pattern,
        rust_path_prefix,
        enum_name,
        variant_name,
        converted.join(", ")
    ))
}

/// Deserialize : RustEnum::Variant(binding) => WitVariantPrefix::WitCase(WitNs::Variant(converted))
fn generate_deserialize_arm(
    rust_path_prefix: &str,
    enum_name: &str,
    v: &syn::Variant,
    variant_prefix: &str,
    wit_case: &str,
    wit_ns: &str,
) -> Option<String> {
    let variant_name = v.ident.to_string();
    let (pattern, bindings) = variant_bindings(v);

    if bindings.is_empty() {
        return Some(format!(
            "                {}::{}::{} => {}::{}({}::{}),\n",
            rust_path_prefix,
            enum_name,
            variant_name,
            variant_prefix,
            wit_case,
            wit_ns,
            variant_name
        ));
    }

    let mut prep = String::new();
    let mut converted = Vec::new();
    for (binding, ty) in &bindings {
        let (type_ident, is_ref, is_slice) = get_type_info(ty);
        let (p, expr, ok) = convert_value(
            &type_ident,
            is_ref,
            is_slice,
            binding,
            Some(binding),
            MappingMode::Deserialize,
            "                ",
        );
        if !ok {
            return None;
        }
        prep.push_str(&p);
        converted.push(expr);
    }

    Some(format!(
        "{}                {}::{}::{}{} => {}::{}({}::{}({})),\n",
        prep,
        rust_path_prefix,
        enum_name,
        variant_name,
        pattern,
        variant_prefix,
        wit_case,
        wit_ns,
        variant_name,
        converted.join(", ")
    ))
}

/// ToWit : RustEnum::Variant(binding) => WitVariantPrefix::WitCase(WitNs::Variant(converted))
fn generate_to_wit_arm(
    v: &syn::Variant,
    variant_prefix: &str,
    wit_case: &str,
    wit_ns: &str,
) -> Option<String> {
    let variant_name = v.ident.to_string();
    let (pattern, bindings) = variant_bindings(v);

    if bindings.is_empty() {
        return Some(format!(
            "            Self::{} => {}::{}({}::{}),\n",
            variant_name, variant_prefix, wit_case, wit_ns, variant_name
        ));
    }

    let mut prep = String::new();
    let mut converted = Vec::new();
    for (binding, ty) in &bindings {
        let (type_ident, is_ref, is_slice) = get_type_info(ty);
        let (p, expr, ok) = convert_value(
            &type_ident,
            is_ref,
            is_slice,
            binding,
            Some(binding),
            MappingMode::ToWit,
            "            ",
        );
        if !ok {
            return None;
        }
        prep.push_str(&p);
        converted.push(expr);
    }

    Some(format!(
        "{}            Self::{}{} => {}::{}({}::{}({})),\n",
        prep,
        variant_name,
        pattern,
        variant_prefix,
        wit_case,
        wit_ns,
        variant_name,
        converted.join(", ")
    ))
}
