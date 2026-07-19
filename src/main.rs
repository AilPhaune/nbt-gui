pub mod app;
pub mod document;
pub mod examples;
pub mod i18n;
pub mod mcregion;
pub mod ui;

use std::sync::Arc;

use anyhow::Context;
use app::NbtEditorApplication;
use egui::{FontData, FontDefinitions, FontFamily};
use iconflow::fonts;

use crate::i18n::Translations;

fn main() -> anyhow::Result<()> {
    let options = eframe::NativeOptions::default();

    let language_str = std::env::var("LANGUAGE")
        .or_else(|_| std::env::var("LC_ALL"))
        .or_else(|_| std::env::var("LC_MESSAGES"))
        .or_else(|_| std::env::var("LANG"))
        .map(|s| s.split(['.', '@']).next().unwrap().to_string())
        .unwrap_or_else(|_| String::from("en-US"));

    let lang_id = language_str
        .parse()
        .with_context(|| format!("Invalid language id: {}", language_str))?;

    let assets_dir = std::env::var("ASSETS_DIR").ok();

    let translations =
        Arc::new(Translations::load(lang_id, assets_dir).context("Failed to load translations")?);

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
    )?;

    Ok(())
}
