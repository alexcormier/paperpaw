//! Any text as a translation catalog: parse it and look up every key.

#![no_main]

use libfuzzer_sys::fuzz_target;
use paperpaw_i18n::Catalog;

fuzz_target!(|data: &[u8]| {
    let text = String::from_utf8_lossy(data).into_owned();
    if let Ok(catalog) = Catalog::parse(text) {
        let _ = (catalog.language(), catalog.name(), catalog.plural_rule());
        for key in catalog.keys() {
            assert!(catalog.get(key).is_some());
        }
    }
});
