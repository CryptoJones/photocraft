//! Parser and lookup tables for translation catalogs (`*.tsv`, format documented in `ja.tsv`).

use std::collections::HashMap;

/// Owned strings; bundled catalogs are cached, external catalogs live for one snapshot.
#[derive(Debug, Default)]
pub struct Catalog {
    /// context-free strings: English source → translation (the hot path, looked up every frame)
    plain: HashMap<String, String>,
    /// strings with a disambiguating context, keyed `context \u{1} source`
    contextual: HashMap<String, String>,
    /// command-id keyed strings
    ids: HashMap<String, String>,
    /// plural messages: `one|other` → forms
    plurals: HashMap<String, Vec<String>>,
}

/// A catalog entry as read from the file: (context, source, translation).
pub type Entry = (String, String, String);

fn unescape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut it = s.chars();
    while let Some(c) = it.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match it.next() {
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some('\\') => out.push('\\'),
            Some(o) => {
                out.push('\\');
                out.push(o);
            }
            None => out.push('\\'),
        }
    }
    out
}

/// Read the entries of a catalog file. Malformed lines are returned as errors (and skipped), so
/// a bad translation never breaks the UI; the tests insist the bundled catalogs have none.
pub fn parse_entries(text: &str) -> (Vec<Entry>, Vec<String>) {
    let mut entries = Vec::new();
    let mut errors = Vec::new();
    for (n, line) in text.lines().enumerate() {
        if line.trim().is_empty() || line.starts_with('#') {
            continue;
        }
        let mut cols = line.split('\t');
        match (cols.next(), cols.next(), cols.next(), cols.next()) {
            (Some(ctx), Some(src), Some(tr), None) if !src.is_empty() && !tr.is_empty() => {
                entries.push((unescape(ctx), unescape(src), unescape(tr)));
            }
            _ => errors.push(format!("line {}: expected `context<TAB>source<TAB>translation`", n + 1)),
        }
    }
    (entries, errors)
}

impl Catalog {
    /// Reject a whole update rather than publishing a partly parsed catalog.
    pub fn parse_checked(text: &str, rule: super::config::PluralRule) -> Result<Self, String> {
        if text.len() > super::config::MAX_CATALOG_BYTES {
            return Err("catalog exceeds 2 MiB".into());
        }
        if text.lines().count() > 20_000 {
            return Err("catalog exceeds 20000 lines".into());
        }
        let (entries, errors) = parse_entries(text.trim_start_matches('\u{feff}'));
        if let Some(error) = errors.first() {
            return Err(error.clone());
        }
        if entries.len() > 10_000 {
            return Err("catalog exceeds 10000 entries".into());
        }
        let mut seen = std::collections::HashSet::new();
        for (context, source, translation) in &entries {
            if context.len() > 256 || source.len() > 8192 || translation.len() > 16384 {
                return Err("catalog entry is too long".into());
            }
            if !seen.insert((context, source)) {
                return Err(format!("duplicate key {context:?} {source:?}"));
            }
            let (source, forms): (&str, Vec<&str>) = if context == "@plural" {
                let (one, other) = source.split_once('|').ok_or_else(|| format!("plural source must be one|other: {source:?}"))?;
                let mut one_parameters = placeholders(one);
                let mut other_parameters = placeholders(other);
                one_parameters.sort_unstable();
                other_parameters.sort_unstable();
                if one.is_empty() || other.is_empty() || other.contains('|') || one_parameters != other_parameters {
                    return Err(format!("invalid plural source {source:?}"));
                }
                let forms: Vec<_> = translation.split('|').collect();
                if forms.len() != rule.forms() || forms.iter().any(|f| f.is_empty()) {
                    return Err(format!("{source:?}: expected {} nonempty plural forms", rule.forms()));
                }
                (other, forms)
            } else {
                if context.is_empty() && source.ends_with('…') != translation.ends_with('…') {
                    return Err(format!("{source:?}: trailing ellipsis differs"));
                }
                (source.as_str(), vec![translation.as_str()])
            };
            let mut expected = placeholders(source);
            expected.sort_unstable();
            for form in forms {
                let mut actual = placeholders(form);
                actual.sort_unstable();
                if expected != actual {
                    return Err(format!("{source:?}: placeholders differ (expected {expected:?}, got {actual:?})"));
                }
            }
        }
        Ok(Self::parse(text.trim_start_matches('\u{feff}')))
    }

    pub fn parse(text: &str) -> Catalog {
        let mut c = Catalog::default();
        for (ctx, src, tr) in parse_entries(text.trim_start_matches('\u{feff}')).0 {
            match ctx.as_str() {
                "" => {
                    c.plain.insert(src, tr);
                }
                "@id" => {
                    c.ids.insert(src, tr);
                }
                "@plural" => {
                    c.plurals.insert(src, tr.split('|').map(str::to_string).collect());
                }
                _ => {
                    c.contextual.insert(format!("{ctx}\u{1}{src}"), tr);
                }
            }
        }
        c
    }

    pub fn plain(&self, s: &str) -> Option<&str> {
        self.plain.get(s).map(String::as_str)
    }

    pub fn contextual(&self, ctx: &str, s: &str) -> Option<&str> {
        self.contextual.get(&format!("{ctx}\u{1}{s}")).map(String::as_str)
    }

    pub fn id(&self, id: &str) -> Option<&str> {
        self.ids.get(id).map(String::as_str)
    }

    /// The plural form `index` of the message whose English forms are `one|other`.
    pub fn plural(&self, one: &str, other: &str, index: usize) -> Option<&str> {
        let forms = self.plurals.get(&format!("{one}|{other}"))?;
        forms.get(index.min(forms.len().saturating_sub(1))).map(String::as_str)
    }
}

/// `{name}` placeholders of a template, in order of appearance.
pub fn placeholders(s: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut rest = s;
    while let Some(a) = rest.find('{') {
        let after = rest.get(a.saturating_add(1)..).unwrap_or("");
        match after.find('}') {
            Some(b) => {
                out.push(after.get(..b).unwrap_or(""));
                rest = after.get(b.saturating_add(1)..).unwrap_or("");
            }
            None => break,
        }
    }
    out
}
