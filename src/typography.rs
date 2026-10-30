//! Embedded Korean UI typography, shared by the editor and the coach overlay.
use eframe::egui::{self, FontData, FontDefinitions, FontFamily, FontId, TextStyle};

const REGULAR: &str = "Pretendard Regular";
const SEMIBOLD: &str = "Pretendard SemiBold";

/// Install the bundled, unmodified Pretendard 1.3.9 static fonts.
///
/// The executable does not need fonts installed on the user's system. The
/// original egui fonts and (when present) Malgun remain glyph fallbacks.
pub fn configure(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();
    fonts.font_data.insert(
        REGULAR.into(),
        FontData::from_static(include_bytes!("../assets/fonts/Pretendard-Regular.otf")).into(),
    );
    fonts.font_data.insert(
        SEMIBOLD.into(),
        FontData::from_static(include_bytes!("../assets/fonts/Pretendard-SemiBold.otf")).into(),
    );

    let mut fallback = fonts
        .families
        .get(&FontFamily::Proportional)
        .cloned()
        .unwrap_or_default();
    if let Ok(bytes) = std::fs::read("C:/Windows/Fonts/malgun.ttf") {
        fonts
            .font_data
            .insert("Malgun fallback".into(), FontData::from_owned(bytes).into());
        fallback.push("Malgun fallback".into());
    }
    let mut regular = vec![REGULAR.into()];
    regular.extend(fallback.iter().cloned());
    fonts.families.insert(FontFamily::Proportional, regular);

    let mut semibold = vec![SEMIBOLD.into(), REGULAR.into()];
    semibold.extend(fallback);
    fonts
        .families
        .insert(FontFamily::Name(SEMIBOLD.into()), semibold);
    fonts
        .families
        .entry(FontFamily::Monospace)
        .or_default()
        .push(REGULAR.into());
    ctx.set_fonts(fonts);

    ctx.style_mut(|style| {
        style
            .text_styles
            .insert(TextStyle::Body, FontId::proportional(14.0));
        style.text_styles.insert(TextStyle::Button, title(13.0));
        style
            .text_styles
            .insert(TextStyle::Small, FontId::proportional(12.0));
        style.text_styles.insert(TextStyle::Heading, title(22.0));
    });
}

/// Real semibold outlines for titles, labels and key numbers (not synthetic bold).
pub fn title(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name(SEMIBOLD.into()))
}
