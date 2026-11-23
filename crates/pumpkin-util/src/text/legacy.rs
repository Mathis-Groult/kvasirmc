use std::fmt::Write;

use crate::text::color::Color;
use crate::text::{TextComponent, TextContent};
use crate::translation::{Locale, get_translation_text};

#[cfg(test)]
mod golden;

impl TextComponent {
    /// Parses a legacy Minecraft formatted string (using section signs '§' by default) into a text component.
    ///
    /// Legacy formatting uses formatting codes after a color symbol:
    /// - Colors: 0-9, a-f
    /// - Styles: l (bold), o (italic), n (underline), m (strikethrough), k (obfuscated)
    /// - Reset: r
    /// - RGB hex colors: §x§R§R§G§G§B§B
    ///
    /// # Arguments
    /// - `input` – The legacy formatted string.
    ///
    /// # Returns
    /// A `TextComponent` with the parsed formatting applied.
    #[must_use]
    pub fn from_legacy_string(input: &str) -> Self {
        Self::from_legacy_string_with_code(input, '§')
    }

    /// Parses a legacy formatted string using a custom color code symbol (e.g. '&' or '§').
    ///
    /// # Arguments
    /// - `input` – The legacy formatted string.
    /// - `code_symbol` – The delimiter character (e.g. `'§'` or `'&'`).
    ///
    /// # Returns
    /// A `TextComponent` with the parsed formatting applied.
    #[must_use]
    pub fn from_legacy_string_with_code(input: &str, code_symbol: char) -> Self {
        let mut root = Self::text("");
        let mut parts = input.split(code_symbol);

        if let Some(first) = parts.next()
            && !first.is_empty()
        {
            root = root.add_child(Self::text(first.to_string()));
        }

        let mut style = LegacyStyle::default();

        while let Some(part) = parts.next() {
            if part.is_empty() {
                continue;
            }

            let code = part.chars().next().unwrap_or(' ').to_ascii_lowercase();
            let mut text = part[code.len_utf8()..].to_string();

            if code == 'x' {
                let (update, trailing) = read_hex_run(&mut parts);
                if let ColorUpdate::Set(color) = update {
                    style.color = color;
                }
                text.push_str(&trailing);
            } else {
                style.apply(code);
            }

            if !text.is_empty() {
                root = root.add_child(style.text(text));
            }
        }

        root
    }

    /// Serializes this component into a Java legacy formatted string using `§`.
    ///
    /// # Arguments
    /// - `locale` – The locale to use for resolving translation components.
    ///
    /// # Returns
    /// A legacy Minecraft formatted string with `§` color and style codes.
    #[must_use]
    pub fn to_legacy_string(&self, locale: Locale) -> String {
        self.to_legacy_string_for_version(&crate::version::JavaMinecraftVersion::V_26_3, locale)
    }

    /// Serializes this component into a Java legacy formatted string for a specific Minecraft version.
    ///
    /// # Arguments
    /// - `version` – The Minecraft version to format for.
    /// - `locale` – The locale to use for resolving translation components.
    ///
    /// # Returns
    /// A legacy Minecraft formatted string with `§` color and style codes.
    #[must_use]
    pub fn to_legacy_string_for_version(
        &self,
        version: &crate::version::JavaMinecraftVersion,
        locale: Locale,
    ) -> String {
        self.to_legacy_string_with_code_for_version(version, '§', locale)
    }

    /// Serializes this component into a legacy formatted string using a custom color code symbol.
    ///
    /// # Arguments
    /// - `code_symbol` – The formatting character symbol (e.g., `'§'` or `'&'`).
    /// - `locale` – The locale to use for resolving translation components.
    ///
    /// # Returns
    /// A legacy formatted string with color and style codes using `code_symbol`.
    #[must_use]
    pub fn to_legacy_string_with_code(&self, code_symbol: char, locale: Locale) -> String {
        self.to_legacy_string_with_code_for_version(
            &crate::version::JavaMinecraftVersion::V_26_3,
            code_symbol,
            locale,
        )
    }

    /// Serializes this component into a legacy formatted string for a specific Minecraft version.
    ///
    /// # Arguments
    /// - `version` – The Minecraft version to format for.
    /// - `code_symbol` – The formatting character symbol (e.g., `'§'` or `'&'`).
    /// - `locale` – The locale to use for resolving translation components.
    ///
    /// # Returns
    /// A legacy formatted string with color and style codes using `code_symbol`.
    #[must_use]
    pub fn to_legacy_string_with_code_for_version(
        &self,
        version: &crate::version::JavaMinecraftVersion,
        code_symbol: char,
        locale: Locale,
    ) -> String {
        let mut text = String::new();

        // 1. Color formatting
        if let Some(color) = &self.0.style.color {
            match color {
                Color::Named(named) => {
                    let _ = write!(text, "{code_symbol}{}", named.to_legacy_char());
                }
                Color::Rgb(rgb) => {
                    if *version >= crate::version::JavaMinecraftVersion::V_1_16 {
                        let hex = format!("{:02x}{:02x}{:02x}", rgb.red, rgb.green, rgb.blue);
                        let _ = write!(text, "{code_symbol}x");
                        for ch in hex.chars() {
                            let _ = write!(text, "{code_symbol}{ch}");
                        }
                    } else {
                        let named = rgb.to_nearest_named();
                        let _ = write!(text, "{code_symbol}{}", named.to_legacy_char());
                    }
                }
                Color::Reset => {
                    let _ = write!(text, "{code_symbol}r");
                }
            }
        }

        // 2. Style formatting
        if self.0.style.bold == Some(true) {
            let _ = write!(text, "{code_symbol}l");
        }
        if self.0.style.italic == Some(true) {
            let _ = write!(text, "{code_symbol}o");
        }
        if self.0.style.underlined == Some(true) {
            let _ = write!(text, "{code_symbol}n");
        }
        if self.0.style.strikethrough == Some(true) {
            let _ = write!(text, "{code_symbol}m");
        }
        if self.0.style.obfuscated == Some(true) {
            let _ = write!(text, "{code_symbol}k");
        }

        // 3. Resolve Content
        match &*self.0.content {
            TextContent::Text { text: t } => text.push_str(t),
            TextContent::Translate {
                translate,
                bedrock_translate: _,
                with,
            } => {
                text.push_str(&get_translation_text(
                    format!("minecraft:{translate}"),
                    locale,
                    with.clone(),
                ));
            }
            TextContent::EntityNames { selector, .. } => text.push_str(selector),
            TextContent::Keybind { keybind } => text.push_str(keybind),
            TextContent::Custom { key, with, .. } => {
                text.push_str(&get_translation_text(key.clone(), locale, with.clone()));
            }
            TextContent::PlayerSprite { profile, .. } => {
                if let Some(name) = profile.0.get_string("name") {
                    text.push_str(name);
                }
            }
        }

        // 4. Recursively append extra components
        for child in &self.0.extra {
            text.push_str(&Self(child.clone()).to_legacy_string_with_code_for_version(
                version,
                code_symbol,
                locale,
            ));
            let _ = write!(text, "{code_symbol}r");
        }

        text
    }
}

/// The colour and styles that the legacy codes read so far have switched on.
#[derive(Default)]
struct LegacyStyle {
    color: Option<Color>,
    bold: bool,
    italic: bool,
    underlined: bool,
    strikethrough: bool,
    obfuscated: bool,
}

impl LegacyStyle {
    /// Applies one code character: a colour code replaces the whole style, `r` clears it, the
    /// format codes add to it and any other character changes nothing.
    fn apply(&mut self, code: char) {
        match code {
            '0'..='9' | 'a'..='f' => {
                *self = Self {
                    color: Color::from_legacy_code(code),
                    ..Self::default()
                };
            }
            'l' => self.bold = true,
            'o' => self.italic = true,
            'n' => self.underlined = true,
            'm' => self.strikethrough = true,
            'k' => self.obfuscated = true,
            'r' => *self = Self::default(),
            _ => {}
        }
    }

    /// A text component carrying `text` in this style.
    fn text(&self, text: String) -> TextComponent {
        let mut child = TextComponent::text(text);
        if let Some(color) = self.color {
            child = child.color(color);
        }
        for (on, apply) in [
            (
                self.bold,
                TextComponent::bold as fn(TextComponent) -> TextComponent,
            ),
            (self.italic, TextComponent::italic),
            (self.underlined, TextComponent::underlined),
            (self.strikethrough, TextComponent::strikethrough),
            (self.obfuscated, TextComponent::obfuscated),
        ] {
            if on {
                child = apply(child);
            }
        }
        child
    }
}

/// What a `§x` run does to the current colour.
enum ColorUpdate {
    /// The run was cut off, or its digits are not text: the colour stays.
    Keep,
    /// The six digits were read: the colour becomes this (`None` when they are not hex digits).
    Set(Option<Color>),
}

/// Reads the six `§R` digit parts that follow `§x`. Returns the colour change and the text that
/// followed each digit, which belongs to the next component.
fn read_hex_run(parts: &mut std::str::Split<'_, char>) -> (ColorUpdate, String) {
    let mut digits = Vec::with_capacity(6);
    let mut trailing = String::new();
    for _ in 0..6 {
        let Some(c) = parts.next().and_then(|part| {
            let c = part.chars().next()?;
            trailing.push_str(&part[c.len_utf8()..]);
            Some(c)
        }) else {
            return (ColorUpdate::Keep, trailing);
        };
        digits.push(c as u8);
    }
    let update = std::str::from_utf8(&digits).map_or(ColorUpdate::Keep, |hex| {
        ColorUpdate::Set(Color::from_hex_str(hex))
    });
    (update, trailing)
}

#[cfg(test)]
mod test {
    use crate::text::TextComponent;
    use crate::text::color::{Color, NamedColor};
    use crate::translation::Locale;

    #[test]
    fn from_legacy_string_hex() {
        let comp = TextComponent::from_legacy_string("§x§f§c§0§0§0§0Test String ▽");
        assert_eq!(comp.0.extra.len(), 1);
        if let crate::text::TextContent::Text { text } = &*comp.0.extra[0].content {
            assert_eq!(text, "Test String ▽");
        } else {
            panic!("Expected Text content");
        }
        assert_eq!(comp.0.extra[0].style.color, Color::from_hex_str("fc0000"));
    }

    #[test]
    fn from_legacy_string_alt_code() {
        let comp = TextComponent::from_legacy_string_with_code("&cRed &lBold", '&');
        assert_eq!(comp.0.extra.len(), 2);
        assert_eq!(
            comp.0.extra[0].style.color,
            Some(Color::Named(NamedColor::Red))
        );
        assert_eq!(comp.0.extra[1].style.bold, Some(true));
    }

    #[test]
    fn to_legacy_string() {
        let comp = TextComponent::text("Hello ").add_child(
            TextComponent::text("World")
                .color_named(NamedColor::Red)
                .bold(),
        );
        let legacy = comp.to_legacy_string(Locale::EnUs);
        assert_eq!(legacy, "Hello §c§lWorld§r");
    }
}
