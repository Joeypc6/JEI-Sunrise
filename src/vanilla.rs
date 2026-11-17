//! Vanilla's recipe categories, laid out as JEI lays them out (`CraftingRecipeCategory`, `AbstractCookingCategory`,
//! `StoneCuttingRecipeCategory`, `SmithingRecipeCategory`), from the recipes in force as the fork's API gives them
//! (`Host::recipes_of_type`). Shaped recipes are read from their file so each ingredient keeps its cell.

use crate::view::{Card, Category, Role};
use mod_api::serde_json::Value;
use mod_api::*;
use std::collections::HashMap;

pub(crate) const CRAFTING: &str = "minecraft:crafting";

/// The built-in categories: (id, title key, icon, recipe types).
pub(crate) const CATEGORIES: &[(&str, &str, &str, &[&str])] = &[
    (
        CRAFTING,
        "jei.category.crafting",
        "minecraft:crafting_table",
        &["minecraft:crafting_shaped", "minecraft:crafting_shapeless", "minecraft:crafting_transmute"],
    ),
    ("minecraft:smelting", "jei.category.smelting", "minecraft:furnace", &["minecraft:smelting"]),
    ("minecraft:blasting", "jei.category.blasting", "minecraft:blast_furnace", &["minecraft:blasting"]),
    ("minecraft:smoking", "jei.category.smoking", "minecraft:smoker", &["minecraft:smoking"]),
    (
        "minecraft:campfire_cooking",
        "jei.category.campfire",
        "minecraft:campfire",
        &["minecraft:campfire_cooking"],
    ),
    ("minecraft:stonecutting", "jei.category.stonecutter", "minecraft:stonecutter", &["minecraft:stonecutting"]),
    (
        "minecraft:smithing",
        "jei.category.smithing",
        "minecraft:smithing_table",
        &["minecraft:smithing_transform", "minecraft:smithing_trim"],
    ),
    ("minecraft:fuel", "jei.category.fuel", "minecraft:furnace", &[]),
];

/// More stations for a vanilla category: a campfire's soul twin, the crafter.
fn extra_catalysts(category: &str) -> &'static [&'static str] {
    match category {
        CRAFTING => &["minecraft:crafter"],
        "minecraft:campfire_cooking" => &["minecraft:soul_campfire"],
        _ => &[],
    }
}

pub(crate) struct Ctx<'a> {
    pub host: &'a dyn Host,
    pub ids: &'a HashMap<String, ItemKey>,
    pub fuels: &'a [ItemKey],
}

impl Ctx<'_> {
    pub(crate) fn key(&self, id: &str) -> Option<ItemKey> {
        let id = if id.contains(':') { id.to_owned() } else { format!("minecraft:{id}") };
        self.ids.get(&id).copied()
    }
    /// An ingredient's items, as its display lists them.
    pub(crate) fn ingredient(&self, ing: &Ingredient) -> Vec<(ItemKey, u32)> {
        match ing.item_keys(self.host) {
            Ok(keys) => keys.iter().map(|k| (*k, 1)).collect(),
            Err(_) => ing.order.iter().filter_map(|id| self.key(id)).map(|k| (k, 1)).collect(),
        }
    }
    /// An ingredient from its file's JSON (an id, `#tag`, `{item}`, `{tag}` or a list of them).
    pub(crate) fn ingredient_json(&self, v: &Value) -> Vec<(ItemKey, u32)> {
        if v.is_null() {
            return Vec::new();
        }
        // NeoForge's sized ingredient: {"ingredient": .., "count": n}, and a plain count beside the item.
        let count = v.get("count").and_then(Value::as_u64).unwrap_or(1).max(1) as u32;
        let inner = v.get("ingredient").unwrap_or(v);
        match self.host.recipe_ingredient(&inner.to_string()) {
            Ok(Some(ing)) => self.ingredient(&ing).into_iter().map(|(k, _)| (k, count)).collect(),
            _ => Vec::new(),
        }
    }
    /// A result from its file's JSON (`{id, count}` or a bare id).
    pub(crate) fn result_json(&self, v: &Value) -> Vec<(ItemKey, u32)> {
        if let Some(id) = v.as_str() {
            return self.key(id).map(|k| vec![(k, 1)]).unwrap_or_default();
        }
        let id = v.get("id").or_else(|| v.get("item")).and_then(Value::as_str);
        let count = v.get("count").and_then(Value::as_u64).unwrap_or(1).max(1) as u32;
        id.and_then(|id| self.key(id)).map(|k| vec![(k, count)]).unwrap_or_default()
    }
    fn stack(&self, s: &ItemStack) -> Vec<(ItemKey, u32)> {
        vec![(s.item(), s.count())]
    }
}

pub(crate) fn categories(cx: &Ctx<'_>) -> Vec<Category> {
    CATEGORIES
        .iter()
        .map(|(id, title, icon, _)| {
            let mut catalysts: Vec<ItemKey> = cx.key(icon).into_iter().collect();
            catalysts.extend(extra_catalysts(id).iter().filter_map(|c| cx.key(c)));
            Category { id: (*id).to_owned(), title: (*title).to_owned(), icon: cx.key(icon), catalysts }
        })
        .collect()
}

/// Every vanilla card. `displays` gives a crafting recipe's recipe-book display id, when the server sent it.
pub(crate) fn cards(cx: &Ctx<'_>) -> Vec<Card> {
    let mut out = Vec::new();
    for (index, (category, _, _, kinds)) in CATEGORIES.iter().enumerate() {
        for kind in kinds.iter() {
            for recipe in cx.host.recipes_of_type(kind).iter() {
                if let Some(card) = card(cx, index, category, recipe) {
                    out.push(card);
                }
            }
        }
        if *category == "minecraft:fuel" {
            for fuel in cx.fuels {
                let ticks = cx.host.item_burn_time(*fuel).unwrap_or(0);
                let mut c = Card::new(index, [82.0, 36.0], "");
                c.slot([1.0, 18.0], Role::Input, vec![(*fuel, 1)]);
                c.texts.push(([24.0, 4.0], format!("{} ticks", ticks), 0xFF80_8080));
                c.texts.push(([24.0, 14.0], format!("{:.1} items", ticks as f32 / 200.0), 0xFF80_8080));
                out.push(c);
            }
        }
    }
    out
}

const STEP: f32 = 18.0;

fn card(cx: &Ctx<'_>, index: usize, category: &str, r: &Recipe) -> Option<Card> {
    let json = &*r.json;
    match category {
        CRAFTING => {
            let mut c = Card::new(index, [116.0, 54.0], &r.id);
            c.arrow = Some([61.0, 19.0]);
            c.transfer = true;
            let result = if r.results.is_empty() { cx.result_json(&json["result"]) } else { cx.stack(&r.results[0]) };
            if r.kind == "minecraft:crafting_shaped" {
                let pattern: Vec<&str> =
                    json["pattern"].as_array()?.iter().filter_map(Value::as_str).collect();
                let width = pattern.iter().map(|row| row.chars().count()).max().unwrap_or(0);
                let mut keys: HashMap<char, Vec<(ItemKey, u32)>> = HashMap::new();
                if let Some(map) = json["key"].as_object() {
                    for (k, v) in map {
                        if let Some(ch) = k.chars().next() {
                            keys.insert(ch, cx.ingredient_json(v));
                        }
                    }
                }
                for (y, row) in pattern.iter().enumerate() {
                    for (x, ch) in row.chars().enumerate() {
                        let stacks = keys.get(&ch).cloned().unwrap_or_default();
                        let cell = (y * width + x) as u32;
                        let slot = c.slot([1.0 + x as f32 * STEP, 1.0 + y as f32 * STEP], Role::Input, stacks);
                        slot.cell = Some(cell);
                    }
                }
            } else if r.kind == "minecraft:crafting_transmute" {
                c.shapeless = true;
                c.slot([1.0, 1.0], Role::Input, cx.ingredient_json(&json["input"])).cell = Some(0);
                c.slot([1.0 + STEP, 1.0], Role::Input, cx.ingredient_json(&json["material"])).cell = Some(1);
            } else {
                c.shapeless = true;
                let list: Vec<Vec<(ItemKey, u32)>> = if r.ingredients.is_empty() {
                    json["ingredients"].as_array()?.iter().map(|v| cx.ingredient_json(v)).collect()
                } else {
                    r.ingredients.iter().map(|i| cx.ingredient(i)).collect()
                };
                let side = if list.len() <= 1 { 1 } else if list.len() <= 4 { 2 } else { 3 };
                for (i, stacks) in list.into_iter().enumerate().take(9) {
                    let (x, y) = (i % side, i / side);
                    c.slot([1.0 + x as f32 * STEP, 1.0 + y as f32 * STEP], Role::Input, stacks).cell = Some(i as u32);
                }
            }
            c.slot([95.0, 19.0], Role::Output, result);
            Some(c)
        }
        "minecraft:smelting" | "minecraft:blasting" | "minecraft:smoking" | "minecraft:campfire_cooking" => {
            let campfire = category == "minecraft:campfire_cooking";
            let mut c = Card::new(index, [82.0, if campfire { 44.0 } else { 54.0 }], &r.id);
            let input = r.ingredients.first().map(|i| cx.ingredient(i)).unwrap_or_else(|| cx.ingredient_json(&json["ingredient"]));
            let result = r.results.first().map(|s| cx.stack(s)).unwrap_or_else(|| cx.result_json(&json["result"]));
            c.slot([1.0, 1.0], Role::Input, input);
            if !campfire {
                c.slot([1.0, 37.0], Role::Other, cx.fuels.iter().map(|k| (*k, 1)).collect());
            }
            c.slot([61.0, 19.0], Role::Output, result);
            c.arrow = Some([24.0, 18.0]);
            let ticks = r.cooking_time.unwrap_or(200);
            let xp = r.experience.unwrap_or(0.0);
            // JEI writes the experience over the arrow and the time under it.
            if xp > 0.0 {
                c.texts.push(([24.0, if campfire { 0.0 } else { 6.0 }], format!("{xp} XP"), 0xFF80_8080));
            }
            let secs = format!("{}s", ticks as f32 / 20.0);
            c.texts.push(([24.0, if campfire { 36.0 } else { 38.0 }], secs, 0xFF80_8080));
            Some(c)
        }
        "minecraft:stonecutting" => {
            let mut c = Card::new(index, [82.0, 34.0], &r.id);
            let (input, result) = match &r.rule {
                Some(RecipeRule::Stonecutting { input, result }) => (cx.ingredient(input), cx.stack(result)),
                _ => (cx.ingredient_json(&json["ingredient"]), cx.result_json(&json["result"])),
            };
            c.slot([1.0, 9.0], Role::Input, input);
            c.slot([61.0, 9.0], Role::Output, result);
            c.arrow = Some([25.0, 8.0]);
            Some(c)
        }
        "minecraft:smithing" => {
            let mut c = Card::new(index, [108.0, 18.0], &r.id);
            let base = cx.ingredient_json(&json["base"]);
            c.slot([1.0, 1.0], Role::Input, cx.ingredient_json(&json["template"]));
            c.slot([19.0, 1.0], Role::Input, base.clone());
            c.slot([37.0, 1.0], Role::Input, cx.ingredient_json(&json["addition"]));
            let result = if r.kind == "minecraft:smithing_trim" { base } else { cx.result_json(&json["result"]) };
            c.slot([91.0, 1.0], Role::Output, result);
            c.arrow = Some([61.0, 0.0]);
            Some(c)
        }
        _ => None,
    }
}
