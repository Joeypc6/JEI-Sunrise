//! The list's subtypes, as JEI lists them from the creative tabs: a potion, splash potion, lingering potion and tipped
//! arrow for each registered potion (`ClientHost::potions`, registry order), and an enchanted book for each
//! enchantment at each level (`ClientHost::enchantments`, registry order, level 1 up to its most). Each takes the
//! plain item's place in the list. Other items with variants in the creative tabs (goat horns, ominous bottles,
//! firework rockets, paintings) are still listed once.

use mod_api::*;

/// The items listed once per potion.
const POTION_ITEMS: [&str; 4] = ["minecraft:potion", "minecraft:splash_potion", "minecraft:lingering_potion", "minecraft:tipped_arrow"];
const BOOK: &str = "minecraft:enchanted_book";

/// `items` (every item, in list order) as the panel's entries: the item and its components' JSON (empty for none).
///
/// A potion item lists the potions the creative tabs show it with (`tabs`), as JEI lists the creative tabs' stacks:
/// no uncraftable potion or tipped arrow. Before the tabs load, every registered potion.
pub(crate) fn entries(host: &dyn ClientHost, items: &[ItemKey], tabs: &mod_api::CreativeModeTabs) -> Vec<(ItemKey, String)> {
    let all: Vec<String> = host.potions().into_iter().map(|p| p.id).collect();
    let shown = |item: ItemKey| -> Vec<String> {
        let listed: Vec<String> = tabs
            .tabs
            .iter()
            .flat_map(|t| t.display_items.iter())
            .filter(|s| s.item() == item)
            .filter_map(potion_of)
            .collect();
        let mut seen = std::collections::HashSet::new();
        let listed: Vec<String> = listed.into_iter().filter(|p| seen.insert(p.clone())).collect();
        if listed.is_empty() { all.clone() } else { all.iter().filter(|p| seen.contains(*p)).cloned().collect() }
    };
    let potions = all.clone();
    let books: Vec<(String, u32)> = host.enchantments().into_iter().map(|e| (e.id, e.max_level)).collect();
    let mut out = Vec::with_capacity(items.len() + potions.len() * POTION_ITEMS.len() + books.iter().map(|(_, n)| *n as usize).sum::<usize>());
    for k in items {
        let id = k.id(host);
        if POTION_ITEMS.contains(&id.as_str()) && !potions.is_empty() {
            out.extend(shown(*k).iter().map(|p| (*k, potion_json(p))));
        } else if id == BOOK && !books.is_empty() {
            out.extend(books.iter().flat_map(|(e, most)| (1..=*most).map(move |level| (*k, book_json(e, level)))));
        } else {
            out.push((*k, String::new()));
        }
    }
    out
}

/// The potion a stack holds: its `minecraft:potion_contents` component, written as the potion's id or as
/// `{potion: <id>, ...}`.
fn potion_of(stack: &mod_api::ItemStack) -> Option<String> {
    let contents = stack.components()?.as_nbt().get(&"minecraft:potion_contents".into())?.clone();
    match contents {
        Tag::String(id) => Some(id.to_string_lossy()),
        Tag::Compound(c) => match c.get(&"potion".into())? {
            Tag::String(id) => Some(id.to_string_lossy()),
            _ => None,
        },
        _ => None,
    }
}

/// `PotionContents.createItemStack`'s component: `{"minecraft:potion_contents":{"potion":"<id>"}}`.
pub(crate) fn potion_json(potion: &str) -> String {
    format!(r#"{{"minecraft:potion_contents":{{"potion":{}}}}}"#, mod_api::serde_json::Value::String(potion.to_owned()))
}

/// `EnchantmentHelper.createBook`'s component: `{"minecraft:stored_enchantments":{"<id>":<level>}}`.
pub(crate) fn book_json(enchantment: &str, level: u32) -> String {
    format!(r#"{{"minecraft:stored_enchantments":{{{}:{level}}}}}"#, mod_api::serde_json::Value::String(enchantment.to_owned()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn subtype_components_are_the_games_json() {
        assert_eq!(potion_json("minecraft:long_swiftness"), r#"{"minecraft:potion_contents":{"potion":"minecraft:long_swiftness"}}"#);
        assert_eq!(book_json("minecraft:sharpness", 5), r#"{"minecraft:stored_enchantments":{"minecraft:sharpness":5}}"#);
        // Both read back as the component values a subtype's focus compares.
        let v: mod_api::serde_json::Value = mod_api::serde_json::from_str(&book_json("minecraft:mending", 1)).unwrap();
        assert_eq!(v["minecraft:stored_enchantments"]["minecraft:mending"], 1);
        assert_eq!(crate::api::component_value(&potion_json("minecraft:water"), "minecraft:potion_contents").as_deref(), Some(r#"{"potion":"minecraft:water"}"#));
    }
}
