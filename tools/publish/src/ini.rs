//! The INI files of a plugin (`plugin.ini`, `store/store.ini`) read as the
//! site reads them: sections of `key = value` lines, `#` and `;` starting
//! a comment line.

/// A line of an INI file: its section, key, value and line number.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Line {
    pub section: String,
    pub key: String,
    pub value: String,
    pub line: usize,
}

/// The `key = value` lines of `text`, with their sections.
pub fn read(text: &str) -> Vec<Line> {
    let mut section = String::new();
    let mut lines = Vec::new();
    for (n, raw) in text.lines().enumerate() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
            continue;
        }
        if let Some(name) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            section = name.trim().to_string();
            continue;
        }
        if let Some((key, value)) = line.split_once('=') {
            lines.push(Line {
                section: section.clone(),
                key: key.trim().to_string(),
                value: value.trim().to_string(),
                line: n + 1,
            });
        }
    }
    lines
}

/// The value of `key` in `section` of `lines`, the last one where there
/// are several.
pub fn get<'a>(lines: &'a [Line], section: &str, key: &str) -> Option<&'a Line> {
    lines
        .iter()
        .rev()
        .find(|l| l.section == section && l.key == key)
}

/// The lines of `section`.
pub fn section<'a>(lines: &'a [Line], section: &'a str) -> impl Iterator<Item = &'a Line> + 'a {
    lines.iter().filter(move |l| l.section == section)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sections_keys_and_comments() {
        let lines = read("# note\n[plugin]\nid = a\n; x\n[store]\ngroup = tools \n");
        assert_eq!(get(&lines, "plugin", "id").unwrap().value, "a");
        assert_eq!(get(&lines, "store", "group").unwrap().line, 6);
        assert!(get(&lines, "store", "id").is_none());
    }
}
