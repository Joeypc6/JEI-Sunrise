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
                TooltipElement::Text(t, _) => host.text_string(&t).ok(),
                _ => None,
            });
            let name = text.next().unwrap_or_else(|| k.id(host)).to_lowercase();
            let tip = text.collect::<Vec<_>>().join(" ").to_lowercase();
            (name, tip)
        })
        .collect();
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
            && let Some(items) = tags.items(key)
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
            let id = k.id(host);
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
                    let hit = if let Some(m) = t.strip_prefix('@') {
                        id.split(':').next().unwrap_or("").contains(m)
                    } else if t.starts_with('#') {
                        tag.contains(k)
                    } else if let Some(s) = t.strip_prefix('$') {
                        tip.contains(s)
                    } else if let Some(s) = t.strip_prefix('&') {
                        id.contains(s)
                    } else {
                        name.contains(t)
                    };
                    hit != neg
                })
            })
        })
        .map(|(_, e)| e.clone())
        .collect()
}
