//! `[[Links]]` in note bodies, as in Obsidian: `[[Net revenue]]` or `[[Net revenue|revenue]]`.

/// Most links one revision may carry.
pub const MAX_LINKS: usize = 200;

/// What links resolve against: the title lowercased, whitespace collapsed. Case and spacing
/// never make two notes different.
pub fn title_key(title: &str) -> String {
    title
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

/// The link targets in `body`, deduplicated by key, in order of first appearance:
/// `(key, title as written)`.
pub fn extract(body: &str) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    let mut rest = body;
    while let Some(start) = rest.find("[[") {
        let after = &rest[start + 2..];
        let Some(end) = after.find("]]") else { break };
        let inner = &after[..end];
        // "[[a [[b]]": the innermost opening bracket starts the link.
        if let Some(nested) = inner.rfind("[[") {
            rest = &after[nested..];
            continue;
        }
        rest = &after[end + 2..];
        // A link never spans lines.
        if inner.contains('\n') {
            continue;
        }
        let target = inner.split('|').next().unwrap_or("").trim();
        if target.is_empty() || target.chars().count() > 120 {
            continue;
        }
        let key = title_key(target);
        if !out.iter().any(|(k, _)| *k == key) {
            out.push((key, target.split_whitespace().collect::<Vec<_>>().join(" ")));
            if out.len() == MAX_LINKS {
                break;
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_links_with_and_without_aliases() {
        let body = "Revenue from [[Order]]s that are [[Settled orders only|settled]], minus \
                    [[Refund]]s. See also [[ net  REVENUE ]] and [[order]].";
        let links = extract(body);
        let keys: Vec<&str> = links.iter().map(|(k, _)| k.as_str()).collect();
        assert_eq!(
            keys,
            ["order", "settled orders only", "refund", "net revenue"]
        );
        assert_eq!(links[3].1, "net REVENUE");
    }

    #[test]
    fn ignores_broken_and_empty_links() {
        assert!(extract("[[]] [[ | alias]] [[unclosed").is_empty());
        assert!(extract("[[across\nlines]]").is_empty());
        assert_eq!(extract("[[a [[b]]")[0].0, "b");
    }

    #[test]
    fn keys_ignore_case_and_spacing() {
        assert_eq!(title_key("  Net   Revenue "), "net revenue");
        assert_eq!(title_key("Umsatz Ä"), "umsatz ä");
    }
}
