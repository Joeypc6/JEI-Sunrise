//! JEI's search grammar over the item list: words match the name; `@mod` the mod (namespace), `#tag` an item tag,
//! `$text` the tooltip, `&id` the id; `-` before a term negates it, `"..."` keeps spaces, and `|` separates
//! alternatives. Names and tooltips are read once, from the game's own tooltips (`item_resource_tooltip`).

use crate::state::Jei;
use mod_api::*;

/// Fills `jei.names` from the tooltips, once per world.
pub(crate) fn ensure_names(jei: &mut Jei, host: &dyn ClientHost) {
    if jei.names.len() == jei.items.len() {
        return;
    }
    jei.names = jei
        .items
        .iter()
        .map(|(k, components)| {
            // A subtype's own tooltip (Potion of Swiftness, Sharpness V), so its name and effects are searchable.
            let stack = crate::api::stack_with(*k, 1, components).unwrap_or_else(|| ItemStack::of(*k));
            let lines = ItemResource::of(&stack).tooltip(host, false, false).unwrap_or_default();
            let mut text = lines.into_iter().filter_map(|e| match e {
                TooltipElement::Text(t, _) => Some(host.text_string(&t)),
                _ => None,
            });
            let mut name = text.next().unwrap_or_else(|| k.id(host).unwrap_or_default().to_owned()).to_lowercase();
            // A mod's name for its extra stack (`api::ExtraStack::name`): "cable facade - stone".
            if let Some(extra) = jei.extra_names.get(&(*k, components.clone())) {
                name = joined_name(&name, extra);
            }
            let tip = text.collect::<Vec<_>>().join(" ").to_lowercase();
            (name, tip)
        })
        .collect();
}

/// The name the search matches: the tooltip's and a mod's own, apart (no word runs across the two).
pub(crate) fn joined_name(tooltip: &str, extra: &str) -> String {
    if extra.is_empty() || tooltip == extra { tooltip.to_owned() } else { format!("{extra}\n{tooltip}") }
}

/// Whether one search term (no `-`) hits an entry: `@` its mod, `$` its tooltip, `&` its id, else its name. A `#`
/// term is the tag's (`tagged`).
pub(crate) fn term_hits(term: &str, id: &str, name: &str, tip: &str, tagged: bool) -> bool {
    if let Some(m) = term.strip_prefix('@') {
        id.split(':').next().unwrap_or("").contains(m)
    } else if term.starts_with('#') {
        tagged
    } else if let Some(s) = term.strip_prefix('$') {
        tip.contains(s)
    } else if let Some(s) = term.strip_prefix('&') {
        id.contains(s)
    } else {
        name.contains(term)
    }
}

fn terms(query: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut quoted = false;
    for ch in query.chars() {
        match ch {
            '"' => quoted = !quoted,
            ' ' if !quoted => {
                if !cur.is_empty() {
                    out.push(std::mem::take(&mut cur));
                }
            }
            _ => cur.push(ch),
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

fn tag_items(host: &dyn Host, name: &str) -> Vec<ItemKey> {
    let candidates =
        if name.contains(':') { vec![name.to_owned()] } else { vec![format!("minecraft:{name}"), format!("c:{name}")] };
    let tags = host.tags();
    let mut out = Vec::new();
    for c in candidates {
        if let Some(key) = Identifier::parse(&c).ok().and_then(|id| tags.key(&id))
            && let TagMembers::Members(items) = tags.items(key)
        {
            out.extend(items);
        }
    }
    out
}

/// The entries the query matches, in list order.
pub(crate) fn filter(jei: &Jei, host: &dyn Host, query: &str) -> Vec<(ItemKey, String)> {
    let query = query.trim().to_lowercase();
    if query.is_empty() {
        return jei.items.clone();
    }
    let groups: Vec<Vec<String>> = query.split('|').map(terms).filter(|g| !g.is_empty()).collect();
    // Tags are looked up once per term.
    let mut tag_sets: Vec<Vec<Vec<ItemKey>>> = Vec::new();
    for g in &groups {
        tag_sets.push(
            g.iter()
                .map(|t| {
                    let t = t.trim_start_matches('-');
                    t.strip_prefix('#').filter(|n| !n.is_empty()).map(|n| tag_items(host, n)).unwrap_or_default()
                })
                .collect(),
        );
    }
    jei.items
        .iter()
        .enumerate()
        .filter(|(i, (k, _))| {
            let id = k.id(host).unwrap_or_default();
            let (name, tip) = jei.names.get(*i).map(|(n, t)| (n.as_str(), t.as_str())).unwrap_or(("", ""));
            groups.iter().zip(&tag_sets).any(|(g, tags)| {
                g.iter().zip(tags).all(|(term, tag)| {
                    let (neg, t) = match term.strip_prefix('-') {
                        Some(rest) => (true, rest),
                        None => (false, term.as_str()),
                    };
                    // A prefix with nothing after it yet ("@", "#", "-", "$" while typing) leaves the list whole.
                    if matches!(t, "" | "@" | "#" | "$" | "&" | "%") {
                        return true;
                    }
                    let hit = term_hits(t, id, name, tip, t.starts_with('#') && tag.contains(k));
                    hit != neg
                })
            })
        })
        .map(|(_, e)| e.clone())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An extra stack is found by the name its mod gives (a facade by its block), as by its tooltip's.
    #[test]
    fn extra_names_are_searched() {
        let name = joined_name("cable facade", "cable facade - acacia log");
        assert!(term_hits("acacia", "ae2:facade", &name, "", false));
        assert!(term_hits("facade", "ae2:facade", &name, "", false));
        assert!(!term_hits("stone", "ae2:facade", &name, "", false));
        // No word across the two names.
        assert!(!term_hits("logcable", "ae2:facade", &name, "", false));
        assert_eq!(joined_name("stone", ""), "stone");
        assert!(term_hits("@ae2", "ae2:facade", &name, "", false));
        assert!(term_hits("&facade", "ae2:facade", &name, "", false));
        assert!(term_hits("$log", "ae2:facade", &name, "oak log", false));
        assert!(term_hits("#planks", "ae2:facade", &name, "", true));
    }
}
