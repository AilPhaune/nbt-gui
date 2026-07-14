pub mod app;
pub mod examples;
pub mod i18n;
pub mod ui;

use std::sync::Arc;

use app::NbtEditorApplication;
use egui::{FontData, FontDefinitions, FontFamily};
use iconflow::fonts;

use crate::i18n::Translations;

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions::default();

    let translations = Arc::new(Translations::load(unic_langid::langid!("en-US")).unwrap());

    eframe::run_native(
        &translations.c().app_title,
        options,
        Box::new(|cc| {
            let mut definitions = FontDefinitions::default();
            for font in fonts() {
                definitions.font_data.insert(
                    font.family.to_string(),
                    Arc::new(FontData::from_static(font.bytes)),
                );
                definitions
                    .families
                    .entry(FontFamily::Name(font.family.into()))
                    .or_default()
                    .insert(0, font.family.to_string());
            }
            cc.egui_ctx.set_fonts(definitions);

            Ok(Box::new(
                NbtEditorApplication::new(Arc::clone(&translations)).unwrap(),
            ))
        }),
    )
}
