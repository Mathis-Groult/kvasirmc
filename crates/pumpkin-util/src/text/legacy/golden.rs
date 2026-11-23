//! Characterisation of `TextComponent::from_legacy_string_with_code`: one line per input and code
//! symbol with the children it builds (text, colour, style flags). The golden file was written by
//! the code before the function was split; restructuring must not change it.
//! Rewrite it on purpose with `KVASIR_BLESS=1 cargo test -p pumpkin-util --lib legacy::golden`.

use std::fmt::Write;

use crate::text::{TextComponent, TextComponentBase, TextContent};

fn corpus() -> Vec<(&'static str, String)> {
    let mut cases: Vec<(&'static str, String)> = Vec::new();
    for code in "0123456789abcdefABCDEF".chars() {
        cases.push(("color", format!("§{code}text")));
    }
    for code in "lonmkrLONMKR".chars() {
        cases.push(("format", format!("§ctext §{code}after")));
    }
    for code in "lonmkr".chars() {
        cases.push(("format_first", format!("§{code}after")));
    }
    cases.extend(basic_cases());
    cases.extend(hex_cases());
    cases.extend(hex_prior_color_cases());
    cases
}

fn basic_cases() -> Vec<(&'static str, String)> {
    [
        ("empty", ""),
        ("plain", "plain text"),
        ("only_code_char", "§"),
        ("two_code_chars", "§§"),
        ("three_code_chars", "§§§a"),
        ("trailing_code_char", "text§"),
        ("trailing_code_after_color", "§ctext§"),
        ("leading_text", "lead§ctext"),
        ("leading_empty_then_code", "§ctext"),
        ("code_without_text", "§c"),
        ("codes_in_a_row", "§a§b§ctext"),
        ("styles_in_a_row", "§l§o§n§m§ktext"),
        ("style_then_color", "§lbold§ccolor resets the style"),
        ("color_then_style", "§cred§lbold stays red"),
        ("reset_after_style", "§c§lboth§rplain"),
        ("reset_in_a_row", "§c§l§rplain"),
        ("reset_no_text", "§ctext§r"),
        ("styles_survive_style", "§lbold§oboth§nthree§mfour§kfive"),
        ("color_resets_every_style", "§l§o§n§m§kx§ay"),
        ("reset_clears_every_style", "§l§o§n§m§kx§ry"),
        ("unknown_code", "§ztext after unknown"),
        ("unknown_keeps_state", "§c§lbold§ztext"),
        ("unknown_only", "§z"),
        ("space_code", "§ text"),
        ("digit_remainder", "§1234"),
        ("repeated_text", "a§cb§cc§rd"),
        ("non_ascii_text", "héllo §c日本語 ▽ 🙂"),
        ("non_ascii_code", "§étext"),
        ("non_ascii_code_wide", "§▽text"),
        ("non_ascii_emoji_code", "§🙂text"),
        ("non_ascii_leading", "日本語§c語"),
        ("ampersand_in_section_input", "&cplain §ared"),
        ("color_after_text_chain", "one§c two§l three§a four§r five"),
        ("sectionless", "&cplain"),
    ]
    .into_iter()
    .map(|(label, input)| (label, input.to_owned()))
    .collect()
}

fn hex_cases() -> Vec<(&'static str, String)> {
    [
        ("hex", "§x§f§c§0§0§0§0Test String ▽"),
        ("hex_upper_x", "§X§f§c§0§0§0§0Test"),
        ("hex_upper_digits", "§x§F§C§A§B§0§1Test"),
        ("hex_no_text", "§x§f§c§0§0§0§0"),
        ("hex_then_text_then_color", "§x§1§2§3§4§5§6hex§ared"),
        ("hex_remainder_on_x", "§xlead§1§2§3§4§5§6tail"),
        ("hex_remainder_on_digits", "§x1a§2b§3c§4d§5e§6ftail"),
        ("hex_remainder_on_last_only", "§x§1§2§3§4§5§6tail"),
        ("hex_remainder_on_first_only", "§x§1x§2§3§4§5§6"),
        ("hex_keeps_styles", "§l§ctext§x§1§2§3§4§5§6hex"),
        ("hex_then_style", "§x§1§2§3§4§5§6§lbold"),
        ("hex_all_styles", "§l§o§n§m§k§x§1§2§3§4§5§6all"),
        ("hex_after_named", "§cred§x§1§2§3§4§5§6hex"),
        ("hex_invalid_digits", "§x§g§g§g§g§g§gtext"),
        ("hex_invalid_keeps_color", "§c§x§g§g§g§g§g§gtext"),
        ("hex_invalid_keeps_no_color", "§x§g§1§2§3§4§5text"),
        ("hex_multibyte_digit", "§x§é§1§2§3§4§5text"),
        ("hex_truncated_none", "§x"),
        ("hex_truncated_text", "§xtext"),
        ("hex_truncated_one", "§x§1text"),
        ("hex_truncated_five", "§x§1§2§3§4§5"),
        ("hex_truncated_five_text", "§x§1§2§3§4§5tail"),
        ("hex_empty_part", "§x§1§§2§3§4§5§6text"),
        ("hex_empty_last_part", "§x§1§2§3§4§5§"),
        ("hex_empty_first_part", "§x§§1§2§3§4§5text"),
        ("hex_two_in_a_row", "§x§1§2§3§4§5§6a§x§a§b§c§d§e§fb"),
        ("hex_then_reset", "§x§1§2§3§4§5§6hex§rplain"),
        ("hex_inside_unknown", "§z§x§1§2§3§4§5§6hex"),
        ("hex_wide_digits", "§x§12§34§56§78§9a§bctext"),
        ("hex_wide_digits_remainders", "§xa§1bb§2cc§3dd§4ee§5ff§6gg"),
    ]
    .into_iter()
    .map(|(label, input)| (label, input.to_owned()))
    .collect()
}

fn hex_prior_color_cases() -> Vec<(&'static str, String)> {
    [
        ("hex_cut_off_keeps_color", "§cred§x§1§2text"),
        ("hex_cut_off_at_end_keeps_color", "§cred§x§1§2"),
        ("hex_empty_part_keeps_color", "§cred§x§1§§ztext"),
        ("hex_empty_first_part_keeps_color", "§cred§x§§ztext"),
        ("hex_cut_off_after_x_keeps_color", "§cred§xtext"),
        ("hex_cut_off_keeps_hex_color", "§x§1§2§3§4§5§6a§x§1§2text"),
    ]
    .into_iter()
    .map(|(label, input)| (label, input.to_owned()))
    .collect()
}

fn describe(base: &TextComponentBase) -> String {
    let text = match &*base.content {
        TextContent::Text { text } => format!("{text:?}"),
        other => format!("<{other:?}>"),
    };
    let style = &base.style;
    let flag = |value: Option<bool>, name: &'static str| match value {
        Some(true) => name,
        Some(false) => "!",
        None => "",
    };
    let mut out = format!("{text} {:?}", style.color);
    for (value, name) in [
        (style.bold, "B"),
        (style.italic, "I"),
        (style.underlined, "U"),
        (style.strikethrough, "S"),
        (style.obfuscated, "O"),
    ] {
        out.push_str(flag(value, name));
    }
    if !base.extra.is_empty() {
        let _ = write!(out, " +{} extra", base.extra.len());
    }
    out
}

fn line(label: &str, input: &str, symbol: char) -> String {
    let swapped: String = input
        .chars()
        .map(|c| match (symbol, c) {
            ('&', '§') => '&',
            _ => c,
        })
        .collect();
    let root = TextComponent::from_legacy_string_with_code(&swapped, symbol).0;
    let children: Vec<String> = root.extra.iter().map(describe).collect();
    format!(
        "{label}|{symbol}|{input:?}|root {}|{}",
        describe(&TextComponentBase {
            extra: vec![],
            ..root
        }),
        children.join(" ; ")
    )
}

fn render() -> String {
    let mut out = String::new();
    for (label, input) in corpus() {
        for symbol in ['§', '&'] {
            out.push_str(&line(label, &input, symbol));
            out.push('\n');
        }
    }
    out
}

#[test]
fn legacy_strings_parse_as_before() -> Result<(), std::io::Error> {
    let now = render();
    let path =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/legacy_parse.txt");
    if std::env::var_os("KVASIR_BLESS").is_some() {
        std::fs::write(&path, &now)?;
        return Ok(());
    }
    let golden = std::fs::read_to_string(&path)?;
    let differing: Vec<(&str, &str)> = golden
        .lines()
        .zip(now.lines())
        .filter(|(a, b)| a != b)
        .collect();
    assert_eq!(
        golden.lines().count(),
        now.lines().count(),
        "number of cases"
    );
    assert!(
        differing.is_empty(),
        "{} cases differ, first: {:?}",
        differing.len(),
        differing.first()
    );
    Ok(())
}
