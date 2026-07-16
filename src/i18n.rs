use std::{borrow::Cow, collections::HashMap, sync::Arc};

use fluent::FluentValue;
use fluent_templates::{ArcLoader, Loader, LoaderError};
use parking_lot::RwLock;
use unic_langid::LanguageIdentifier;

pub struct Translations {
    loader: ArcLoader,
    language: LanguageIdentifier,
    cache: Arc<RwLock<HashMap<&'static str, Arc<String>>>>,
    common: Arc<CommonTranslationCache>,
}

#[derive(Default)]
pub struct CommonTranslationCache {
    pub app_title: String,
    pub menu_file: String,
    pub menu_file_exit: String,
    pub menu_file_open_test_nbt: String,
    pub menu_file_open_test_nbt_simple: String,
    pub menu_file_open_test_nbt_huge: String,
    pub menu_file_open: String,
    pub menu_file_new: String,
    pub menu_file_save: String,
    pub menu_file_save_as: String,

    pub menu_preferences: String,
    pub menu_preferences_zoom: String,

    pub title_test_nbt: String,
    pub title_untitled: String,

    pub dialog_error: String,

    pub button_delete_text: String,
    pub button_cut_text: String,
    pub button_copy_text: String,
    pub button_paste_text: String,
    pub button_paste_value_text: String,
    pub button_paste_key_value_text: String,
    pub button_paste_above_text: String,
    pub button_paste_below_text: String,
    pub button_confirm_text: String,
    pub button_create_empty_root: String,

    pub root_nbt_empty_text: String,
    pub unnamed_root_nbt_text_hint: String,
    pub editable_key_empty_text: String,
    pub editable_value_empty_text: String,
    pub empty_list_text: String,
    pub compound_simple_value_change_type: String,
    pub compound_array_change_type: String,
    pub compound_simple_value_try_parse: String,
    pub nbt_list_change_type: String,
    pub nbt_list_try_change_type: String,
    pub dt_conv_warn: String,
    pub type_conv_abort_on_fail_text: String,
    pub type_conv_default_on_fail_text: String,
    pub button_list_to_compound_conv: String,

    pub type_hint_empty_list: String,
    pub type_hint_i8: String,
    pub type_hint_u8: String,
    pub type_hint_i16: String,
    pub type_hint_i32: String,
    pub type_hint_i64: String,
    pub type_hint_f32: String,
    pub type_hint_f64: String,
    pub type_hint_str: String,
    pub type_hint_list: String,
    pub type_hint_compound: String,
    pub type_hint_byte_array: String,
    pub type_hint_int_array: String,
    pub type_hint_long_array: String,
    pub type_hint_list_i8: String,
    pub type_hint_list_byte: String,
    pub type_hint_list_byte_arrays: String,
    pub type_hint_list_compounds: String,
    pub type_hint_list_f64: String,
    pub type_hint_list_f32: String,
    pub type_hint_list_i32: String,
    pub type_hint_list_int_arrays: String,
    pub type_hint_list_lists: String,
    pub type_hint_list_i64: String,
    pub type_hint_list_long_arrays: String,
    pub type_hint_list_i16: String,
    pub type_hint_list_strs: String,
}

impl CommonTranslationCache {
    fn new(loader: &mut Translations) -> Self {
        Self {
            app_title: loader.t("app-title").to_string(),
            menu_file: loader.t("menu-file").to_string(),
            menu_file_exit: loader.t("menu-file-exit").to_string(),
            menu_file_open_test_nbt: loader.t("menu-file-open-test-nbt").to_string(),
            menu_file_open_test_nbt_simple: loader.t("menu-file-open-test-nbt-simple").to_string(),
            menu_file_open_test_nbt_huge: loader.t("menu-file-open-test-nbt-huge").to_string(),
            menu_file_open: loader.t("menu-file-open").to_string(),
            menu_file_new: loader.t("menu-file-new").to_string(),
            menu_file_save: loader.t("menu-file-save").to_string(),
            menu_file_save_as: loader.t("menu-file-save-as").to_string(),

            menu_preferences: loader.t("menu-preferences").to_string(),
            menu_preferences_zoom: loader.t("menu-preferences-zoom").to_string(),

            title_test_nbt: loader.t("title-test-nbt").to_string(),
            title_untitled: loader.t("title-untitled").to_string(),

            dialog_error: loader.t("dialog-error").to_string(),

            button_delete_text: loader.t("button-delete-text").to_string(),
            button_cut_text: loader.t("button-cut-text").to_string(),
            button_copy_text: loader.t("button-copy-text").to_string(),
            button_paste_text: loader.t("button-paste-text").to_string(),
            button_paste_value_text: loader.t("button-paste-value-text").to_string(),
            button_paste_key_value_text: loader.t("button-paste-key-value-text").to_string(),
            button_paste_above_text: loader.t("button-paste-above-text").to_string(),
            button_paste_below_text: loader.t("button-paste-below-text").to_string(),
            button_confirm_text: loader.t("button-confirm-text").to_string(),
            button_create_empty_root: loader.t("button-create-empty-root").to_string(),

            root_nbt_empty_text: loader.t("root-nbt-empty-text").to_string(),
            unnamed_root_nbt_text_hint: loader.t("unnamed-root-nbt-text-hint").to_string(),
            editable_key_empty_text: loader.t("editable-key-empty-text").to_string(),
            editable_value_empty_text: loader.t("editable-value-empty-text").to_string(),
            empty_list_text: loader.t("empty-list-text").to_string(),
            compound_simple_value_change_type: loader
                .t("compound-simple-value-change-type")
                .to_string(),
            compound_array_change_type: loader.t("compound-array-change-type").to_string(),
            compound_simple_value_try_parse: loader
                .t("compound-simple-value-try-parse")
                .to_string(),
            nbt_list_change_type: loader.t("nbt-list-change-type").to_string(),
            nbt_list_try_change_type: loader.t("nbt-list-try-change-type").to_string(),
            dt_conv_warn: loader.t("dt-conv-warn").to_string(),
            type_conv_abort_on_fail_text: loader.t("type-conv-abort-on-fail-text").to_string(),
            type_conv_default_on_fail_text: loader.t("type-conv-default-on-fail-text").to_string(),
            button_list_to_compound_conv: loader.t("button-list-to-compound-conv").to_string(),

            type_hint_empty_list: loader.t("type-hint_empty-list").to_string(),
            type_hint_i8: loader.t("type-hint-i8").to_string(),
            type_hint_u8: loader.t("type-hint-u8").to_string(),
            type_hint_i16: loader.t("type-hint-i16").to_string(),
            type_hint_i32: loader.t("type-hint-i32").to_string(),
            type_hint_i64: loader.t("type-hint-i64").to_string(),
            type_hint_f32: loader.t("type-hint-f32").to_string(),
            type_hint_f64: loader.t("type-hint-f64").to_string(),
            type_hint_str: loader.t("type-hint-str").to_string(),
            type_hint_list: loader.t("type-hint-list").to_string(),
            type_hint_compound: loader.t("type-hint-compound").to_string(),
            type_hint_byte_array: loader.t("type-hint-byte-array").to_string(),
            type_hint_int_array: loader.t("type-hint-int-array").to_string(),
            type_hint_long_array: loader.t("type-hint-long-array").to_string(),
            type_hint_list_i8: loader.t("type-hint-list-i8").to_string(),
            type_hint_list_byte: loader.t("type-hint-list-byte").to_string(),
            type_hint_list_byte_arrays: loader.t("type-hint-list-byte-arrays").to_string(),
            type_hint_list_compounds: loader.t("type-hint-list-compounds").to_string(),
            type_hint_list_f64: loader.t("type-hint-list-f64").to_string(),
            type_hint_list_f32: loader.t("type-hint-list-f32").to_string(),
            type_hint_list_i32: loader.t("type-hint-list-i32").to_string(),
            type_hint_list_int_arrays: loader.t("type-hint-list-int-arrays").to_string(),
            type_hint_list_lists: loader.t("type-hint-list-lists").to_string(),
            type_hint_list_i64: loader.t("type-hint-list-i64").to_string(),
            type_hint_list_long_arrays: loader.t("type-hint-list-long-arrays").to_string(),
            type_hint_list_i16: loader.t("type-hint-list-i16").to_string(),
            type_hint_list_strs: loader.t("type-hint-list-strs").to_string(),
        }
    }
}

impl Translations {
    pub fn load(language: LanguageIdentifier) -> Result<Self, LoaderError> {
        let loader = ArcLoader::builder("./assets/locales", language.clone()).build()?;
        let mut translations = Self {
            common: Arc::new(Default::default()),
            loader,
            language,
            cache: Arc::new(RwLock::new(HashMap::new())),
        };
        translations.common = Arc::new(CommonTranslationCache::new(&mut translations));
        Ok(translations)
    }
}

impl Translations {
    pub fn f(&self, key: &'static str, args: &HashMap<Cow<'static, str>, FluentValue>) -> String {
        self.loader
            .try_lookup_with_args(&self.language, key, args)
            .unwrap_or_else(|| key.to_string())
    }

    pub fn c(&self) -> &Arc<CommonTranslationCache> {
        &self.common
    }

    pub fn t(&self, key: &'static str) -> Arc<String> {
        if let Some(arc) = self.cache.read().get(key) {
            return Arc::clone(arc);
        }

        let str = Arc::new(
            self.loader
                .try_lookup(&self.language, key)
                .unwrap_or_else(|| key.to_string()),
        );

        self.cache.write().insert(key, Arc::clone(&str));
        str
    }
}
