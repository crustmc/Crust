mod json;

pub use json::{deserialize_json, serialize_json};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Text {
    pub content: TextContent,
    pub extra: Vec<Text>,
    pub style: Style,
    pub insertion: Option<String>,
    pub click_event: Option<ClickEvent>,
    pub hover_event: Option<HoverEvent>,
}

impl Text {
    pub fn new<T: Into<TextContent>>(content: T) -> Self {
        Text {
            content: content.into(),
            extra: Vec::new(),
            style: Style::empty(),
            insertion: None,
            click_event: None,
            hover_event: None,
        }
    }

    pub fn add_extra<T: Into<Text>>(&mut self, extra: T) {
        self.extra.push(extra.into());
    }

    pub fn add_extras<T: Into<Text>, E: IntoIterator<Item=T>>(&mut self, extras: E) {
        self.extra.extend(extras.into_iter().map(Into::into));
    }

    pub fn accept<F: FnMut(&Text) -> bool>(&self, mut visitor: F) {
        let mut stack = Vec::new();
        stack.push(self);
        while let Some(text) = stack.pop() {
            if !visitor(text) {
                return;
            }
            for extra in &text.extra {
                stack.push(extra);
            }
        }
    }

    pub fn get_string(&self) -> String {
        let mut result = String::new();
        self.accept(|text| {
            if let Some(str) = text.content.get_string() {
                result.push_str(str);
            }
            true
        });
        result
    }
}

impl<T: Into<TextContent>> From<T> for Text {
    fn from(content: T) -> Self {
        Text::new(content)
    }
}

impl From<TextBuilder> for Text {
    fn from(builder: TextBuilder) -> Self {
        builder.build()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TextContent {
    Literal(String),
    Translation { key: String, fallback: Option<String>, with: Option<Vec<Text>> },
    Score { name: String, objective: String, value: Option<String> },
    EntitySelector { selector: String, separator: Option<Box<Text>> },
    Keybind(String),
    Nbt {
        nbt: String,
        interpret: Option<bool>,
        separator: Option<Box<Text>>,
        block: Option<String>,
        entity: Option<String>,
        storage: Option<String>,
    },
}

impl TextContent {
    pub fn literal(text: String) -> Self {
        TextContent::Literal(text)
    }

    pub fn translation(key: String, fallback: Option<String>, with: Option<Vec<Text>>) -> Self {
        TextContent::Translation { key, fallback, with }
    }

    pub fn score(name: String, objective: String, value: Option<String>) -> Self {
        TextContent::Score { name, objective, value }
    }

    pub fn entity_selector(selector: String, separator: Option<Text>) -> Self {
        TextContent::EntitySelector { selector, separator: separator.map(Box::new) }
    }

    pub fn keybind(key: String) -> Self {
        TextContent::Keybind(key)
    }

    pub fn nbt(nbt: String, interpret: Option<bool>, separator: Option<Text>,
               block: Option<String>, entity: Option<String>, storage: Option<String>) -> Self {
        TextContent::Nbt { nbt, interpret, separator: separator.map(Box::new), block, entity, storage }
    }

    pub fn get_string(&self) -> Option<&String> {
        match self {
            TextContent::Literal(text) => Some(text),
            TextContent::Translation { key, fallback, .. } => fallback.as_ref().or(Some(key)),
            _ => None,
        }
    }
}

impl From<String> for TextContent {
    fn from(text: String) -> Self {
        TextContent::Literal(text)
    }
}

impl From<&str> for TextContent {
    fn from(text: &str) -> Self {
        TextContent::Literal(text.to_string())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClickEvent {
    pub action: ClickAction,
    pub value: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClickAction {
    OpenUrl,
    OpenFile,
    RunCommand,
    SuggestCommand,
    ChangePage,
    CopyToClipboard,
    Unresolved(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HoverEvent {
    ShowText(Box<Text>),
    ShowItem { id: String, count: Option<i32>, tag: Option<String> },
    ShowEntity { id: String, entity_type: String, name: Option<String> },
    Unresolved { action: String, value: String },
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Style {
    pub color: Option<TextColor>,
    pub font: Option<String>,
    pub shadow_color: Option<u32>,
    pub bold: Option<bool>,
    pub italic: Option<bool>,
    pub underlined: Option<bool>,
    pub strikethrough: Option<bool>,
    pub obfuscated: Option<bool>,
}

impl Style {
    #[inline]
    pub fn empty() -> Self {
        Self::default()
    }

    pub fn is_empty(&self) -> bool {
        self.color.is_none() && self.shadow_color.is_none() && self.bold.is_none() &&
            self.italic.is_none() && self.underlined.is_none() && self.strikethrough.is_none() &&
            self.obfuscated.is_none()
    }

    pub fn with_color<C: Into<TextColor>>(mut self, color: C) -> Self {
        self.color = Some(color.into());
        self
    }

    pub fn without_color(mut self) -> Self {
        self.color = None;
        self
    }

    pub fn with_font(mut self, font: String) -> Self {
        self.font = Some(font);
        self
    }

    pub fn without_font(mut self) -> Self {
        self.font = None;
        self
    }

    pub fn with_shadow_color(mut self, color: u32) -> Self {
        self.shadow_color = Some(color);
        self
    }

    pub fn without_shadow_color(mut self) -> Self {
        self.shadow_color = None;
        self
    }

    pub fn with_bold(mut self, bold: bool) -> Self {
        self.bold = Some(bold);
        self
    }

    pub fn without_bold(mut self) -> Self {
        self.bold = None;
        self
    }

    pub fn with_italic(mut self, italic: bool) -> Self {
        self.italic = Some(italic);
        self
    }

    pub fn without_italic(mut self) -> Self {
        self.italic = None;
        self
    }

    pub fn with_underlined(mut self, underlined: bool) -> Self {
        self.underlined = Some(underlined);
        self
    }

    pub fn without_underlined(mut self) -> Self {
        self.underlined = None;
        self
    }

    pub fn with_strikethrough(mut self, strikethrough: bool) -> Self {
        self.strikethrough = Some(strikethrough);
        self
    }

    pub fn without_strikethrough(mut self) -> Self {
        self.strikethrough = None;
        self
    }

    pub fn with_obfuscated(mut self, obfuscated: bool) -> Self {
        self.obfuscated = Some(obfuscated);
        self
    }

    pub fn without_obfuscated(mut self) -> Self {
        self.obfuscated = None;
        self
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextColor {
    Hex(u32),
    Black,
    DarkBlue,
    DarkGreen,
    DarkAqua,
    DarkRed,
    DarkPurple,
    Gold,
    Gray,
    DarkGray,
    Blue,
    Green,
    Aqua,
    Red,
    LightPurple,
    Yellow,
    White,
}

impl TextColor {
    pub fn get_rgb(&self) -> u32 {
        match self {
            TextColor::Hex(color) => *color,
            TextColor::Black => 0x000000,
            TextColor::DarkBlue => 0x0000AA,
            TextColor::DarkGreen => 0x00AA00,
            TextColor::DarkAqua => 0x00AAAA,
            TextColor::DarkRed => 0xAA0000,
            TextColor::DarkPurple => 0xAA00AA,
            TextColor::Gold => 0xFFAA00,
            TextColor::Gray => 0xAAAAAA,
            TextColor::DarkGray => 0x555555,
            TextColor::Blue => 0x5555FF,
            TextColor::Green => 0x55FF55,
            TextColor::Aqua => 0x55FFFF,
            TextColor::Red => 0xFF5555,
            TextColor::LightPurple => 0xFF55FF,
            TextColor::Yellow => 0xFFFF55,
            TextColor::White => 0xFFFFFF,
        }
    }

    pub fn from_rgb(r: u8, g: u8, b: u8) -> Self {
        TextColor::Hex((r as u32) << 16 | (g as u32) << 8 | b as u32)
    }
}

impl From<u32> for TextColor {
    fn from(color: u32) -> Self {
        TextColor::Hex(color)
    }
}

impl From<i32> for TextColor {
    fn from(color: i32) -> Self {
        TextColor::Hex(color as u32)
    }
}

impl From<[u8; 3]> for TextColor {
    fn from(color: [u8; 3]) -> Self {
        TextColor::from_rgb(color[0], color[1], color[2])
    }
}

impl From<(u8, u8, u8)> for TextColor {
    fn from(color: (u8, u8, u8)) -> Self {
        TextColor::from_rgb(color.0, color.1, color.2)
    }
}

#[derive(Debug, Clone)]
pub struct TextBuilder {
    inner: Text,
}

impl TextBuilder {
    pub fn new<T: Into<TextContent>>(content: T) -> Self {
        Self {
            inner: Text::new(content),
        }
    }

    pub fn extra<T: Into<Text>>(mut self, extra: T) -> Self {
        self.inner.extra.push(extra.into());
        self
    }

    pub fn add_extra<T: Into<Text>>(&mut self, extra: T) -> &mut Self {
        self.inner.extra.push(extra.into());
        self
    }

    pub fn extras<T: Into<Text>, E: IntoIterator<Item=T>>(mut self, extras: E) -> Self {
        self.inner.extra.extend(extras.into_iter().map(Into::into));
        self
    }

    pub fn add_extras<T: Into<Text>, E: IntoIterator<Item=T>>(&mut self, extras: E) -> &mut Self {
        self.inner.extra.extend(extras.into_iter().map(Into::into));
        self
    }

    pub fn style(mut self, style: Style) -> Self {
        self.inner.style = style;
        self
    }

    pub fn insertion(mut self, insertion: String) -> Self {
        self.inner.insertion = Some(insertion);
        self
    }

    pub fn click_event(mut self, action: ClickAction, value: String) -> Self {
        self.inner.click_event = Some(ClickEvent { action, value });
        self
    }

    pub fn hover_event(mut self, event: HoverEvent) -> Self {
        self.inner.hover_event = Some(event);
        self
    }

    pub fn build(self) -> Text {
        self.inner
    }
}
