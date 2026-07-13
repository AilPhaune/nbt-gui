use std::{borrow::Cow, collections::HashMap, sync::Arc};

use fluent::FluentValue;
use fluent_templates::{ArcLoader, Loader, LoaderError};
use parking_lot::RwLock;
use unic_langid::LanguageIdentifier;

pub struct Translations {
    loader: ArcLoader,
    language: LanguageIdentifier,
    cache: Arc<RwLock<HashMap<&'static str, Arc<String>>>>,
}

impl Translations {
    pub fn load(language: LanguageIdentifier) -> Result<Self, LoaderError> {
        let loader = ArcLoader::builder("./assets/locales", language.clone()).build()?;
        Ok(Self {
            loader,
            language,
            cache: Arc::new(RwLock::new(HashMap::new())),
        })
    }
}

impl Translations {
    pub fn f(&self, key: &'static str, args: &HashMap<Cow<'static, str>, FluentValue>) -> String {
        self.loader
            .try_lookup_with_args(&self.language, key, args)
            .unwrap_or_else(|| key.to_string())
    }

    pub fn t(&self, key: &'static str) -> Arc<String> {
        let mut guard = self.cache.upgradable_read();
        if let Some(arc) = guard.get(key) {
            Arc::clone(arc)
        } else {
            let str = Arc::new(
                self.loader
                    .try_lookup(&self.language, key)
                    .unwrap_or_else(|| key.to_string()),
            );
            guard.with_upgraded(|m| {
                m.insert(key, Arc::clone(&str));
            });
            str
        }
    }
}
