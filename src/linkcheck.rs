//! Dead links against the vault on disk (brain-lib's brokenLinksIn and quoteDeadLinks): what the
//! doctor's `links` check fails on, and what text copied into the vault (a commit body, a flushed
//! summary) gets backticks around so it does not become a dead link.
use crate::vault;
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

pub struct LinkCheck {
    names: HashSet<String>,
    files: HashSet<String>,
    path_of: HashMap<String, PathBuf>,
    allowed: HashSet<String>,
    headings: RefCell<HashMap<String, HashSet<String>>>,
}

impl LinkCheck {
    /// Built once from a walk of the vault: note names, file names, and the forward links.
    pub fn new(root: &Path, files: &[vault::File]) -> LinkCheck {
        let mut names = HashSet::new();
        let mut path_of = HashMap::new();
        let mut all = HashSet::new();
        for f in files {
            let base = f.rel.rsplit('/').next().unwrap_or(&f.rel);
            all.insert(base.to_lowercase());
            if f.is_note() {
                let n = vault::name_of(&f.rel).to_lowercase();
                names.insert(n.clone());
                path_of.insert(n, root.join(&f.rel));
            }
        }
        LinkCheck {
            names,
            files: all,
            path_of,
            allowed: vault::forward_links(root),
            headings: RefCell::new(HashMap::new()),
        }
    }

    pub fn allowed(&self) -> usize {
        self.allowed.len()
    }

    fn has_heading(&self, note: &str, h: &str) -> bool {
        let mut cache = self.headings.borrow_mut();
        let set = cache.entry(note.to_string()).or_insert_with(|| {
            self.path_of
                .get(note)
                .and_then(|p| std::fs::read_to_string(p).ok())
                .map(|t| vault::headings_of(&t.replace("\r\n", "\n")))
                .unwrap_or_default()
        });
        set.contains(&h.to_lowercase())
    }

    /// Targets in `text` that resolve to no note, no file and no forward link, then `Note#Heading`
    /// for a heading link whose note has no such heading.
    pub fn broken(&self, text: &str) -> Vec<String> {
        let links = vault::links_of(text);
        let mut out: Vec<String> = links
            .iter()
            .filter(|l| {
                let k = l.name.to_lowercase();
                !self.names.contains(&k) && !self.files.contains(&k) && !self.allowed.contains(&k)
            })
            .map(|l| l.name.clone())
            .collect();
        for l in &links {
            let Some(h) = &l.heading else { continue };
            let k = l.name.to_lowercase();
            if self.path_of.contains_key(&k) && !self.has_heading(&k, h) {
                out.push(format!("{}#{h}", l.name));
            }
        }
        out
    }

    /// Backticks around every `[[link]]` in `body` that resolves to nothing; live links stay.
    pub fn quote_dead(&self, body: &str) -> String {
        if !body.contains("[[") {
            return body.to_string();
        }
        let dead: HashSet<String> = self.broken(body).iter().map(|t| t.to_lowercase()).collect();
        if dead.is_empty() {
            return body.to_string();
        }
        let b = body.as_bytes();
        let mut out = String::with_capacity(body.len() + 8);
        let mut i = 0;
        while let Some(k) = body[i..].find("[[") {
            let start = i + k;
            let inner = start + 2;
            // The link runs to the first `]]` on its line; a name stops at ] | # or a newline.
            let line_end = body[inner..].find('\n').map_or(body.len(), |e| inner + e);
            let Some(close) = body[inner..line_end].find("]]").map(|c| inner + c) else {
                out.push_str(&body[i..inner]);
                i = inner;
                continue;
            };
            let seg = &body[inner..close];
            let name_end = seg.find([']', '|', '#']).unwrap_or(seg.len());
            if name_end == 0 || seg[..close - inner].contains(']') {
                out.push_str(&body[i..inner]);
                i = inner;
                continue;
            }
            let name = seg[..name_end].trim();
            let name = name
                .rsplit(['/', '\\'])
                .next()
                .unwrap_or(name)
                .to_lowercase();
            let heading = seg[name_end..]
                .strip_prefix('#')
                .map(|h| h.split('|').next().unwrap_or("").trim().to_lowercase())
                .filter(|h| !h.is_empty() && !h.contains(['#', '^']));
            let quoted_before = start > 0 && b[start - 1] == b'`';
            let quoted_after = b.get(close + 2) == Some(&b'`');
            let is_dead = dead.contains(&name)
                || heading
                    .as_ref()
                    .is_some_and(|h| dead.contains(&format!("{name}#{h}")));
            out.push_str(&body[i..start]);
            if is_dead && !quoted_before && !quoted_after {
                out.push('`');
                out.push_str(&body[start..close + 2]);
                out.push('`');
            } else {
                out.push_str(&body[start..close + 2]);
            }
            i = close + 2;
        }
        out.push_str(&body[i..]);
        out
    }
}
