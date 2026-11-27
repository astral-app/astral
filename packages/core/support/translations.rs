use std::collections::HashMap;
use std::sync::{LazyLock, Mutex, RwLock};

use crate::plugins::package::{best_locale, fill_placeholders};

type Table = HashMap<&'static str, &'static str>;

const ENGLISH: &str = "en-US";
const LOCALES: [(&str, &str); 32] = [
    ("bg", include_str!("../i18n/bg.toml")),
    ("cs", include_str!("../i18n/cs.toml")),
    ("da", include_str!("../i18n/da.toml")),
    ("de", include_str!("../i18n/de.toml")),
    ("el", include_str!("../i18n/el.toml")),
    ("en-GB", include_str!("../i18n/en-GB.toml")),
    ("en-US", include_str!("../i18n/en-US.toml")),
    ("es-419", include_str!("../i18n/es-419.toml")),
    ("es-ES", include_str!("../i18n/es-ES.toml")),
    ("fi", include_str!("../i18n/fi.toml")),
    ("fr", include_str!("../i18n/fr.toml")),
    ("hi", include_str!("../i18n/hi.toml")),
    ("hr", include_str!("../i18n/hr.toml")),
    ("hu", include_str!("../i18n/hu.toml")),
    ("id", include_str!("../i18n/id.toml")),
    ("it", include_str!("../i18n/it.toml")),
    ("ja", include_str!("../i18n/ja.toml")),
    ("ko", include_str!("../i18n/ko.toml")),
    ("lt", include_str!("../i18n/lt.toml")),
    ("nl", include_str!("../i18n/nl.toml")),
    ("no", include_str!("../i18n/no.toml")),
    ("pl", include_str!("../i18n/pl.toml")),
    ("pt-BR", include_str!("../i18n/pt-BR.toml")),
    ("ro", include_str!("../i18n/ro.toml")),
    ("ru", include_str!("../i18n/ru.toml")),
    ("sv-SE", include_str!("../i18n/sv-SE.toml")),
    ("th", include_str!("../i18n/th.toml")),
    ("tr", include_str!("../i18n/tr.toml")),
    ("uk", include_str!("../i18n/uk.toml")),
    ("vi", include_str!("../i18n/vi.toml")),
    ("zh-CN", include_str!("../i18n/zh-CN.toml")),
    ("zh-TW", include_str!("../i18n/zh-TW.toml")),
];

static ENGLISH_TABLE: LazyLock<&'static Table> = LazyLock::new(|| parse(source_of(ENGLISH).unwrap_or_default()));
static PARSED: Mutex<Vec<(&'static str, &'static Table)>> = Mutex::new(Vec::new());
static CHOSEN: RwLock<Option<&'static Table>> = RwLock::new(None);

fn source_of(locale: &str) -> Option<&'static str> {
    LOCALES.iter().find(|(name, _)| *name == locale).map(|(_, source)| *source)
}

fn parse(source: &'static str) -> &'static Table {
    let entries = source.parse::<toml::Table>().unwrap_or_default();
    let table: Table = entries
        .into_iter()
        .filter_map(|(key, value)| match value {
            toml::Value::String(text) if !text.is_empty() => Some((&*key.leak(), &*text.leak())),
            _ => None,
        })
        .collect();
    Box::leak(Box::new(table))
}

fn table_for(locale: &'static str) -> Option<&'static Table> {
    let mut parsed = PARSED.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some((_, table)) = parsed.iter().find(|(name, _)| *name == locale) {
        return Some(table);
    }
    let table = parse(source_of(locale)?);
    parsed.push((locale, table));
    Some(table)
}

pub fn locales() -> Vec<String> {
    LOCALES.iter().map(|(name, _)| (*name).to_owned()).collect()
}

pub fn choose(preferred: &[String]) -> Option<&'static str> {
    let locale = best_locale(&locales(), preferred)?;
    let name = LOCALES.iter().map(|(name, _)| *name).find(|name| *name == locale)?;
    *CHOSEN.write().unwrap_or_else(|poisoned| poisoned.into_inner()) = table_for(name);
    Some(name)
}

pub fn tr(key: &'static str) -> &'static str {
    let chosen = *CHOSEN.read().unwrap_or_else(|poisoned| poisoned.into_inner());
    chosen
        .and_then(|table| table.get(key))
        .or_else(|| ENGLISH_TABLE.get(key))
        .copied()
        .unwrap_or(key)
}

pub fn tr_with(key: &'static str, values: &[(&str, String)]) -> String {
    let values: Vec<(String, String)> = values.iter().map(|(name, value)| ((*name).to_owned(), value.clone())).collect();
    fill_placeholders(tr(key), &values)
}
