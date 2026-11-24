//! The API other mods use to add recipe categories, catalysts and information pages: files under
//! `assets/<namespace>/jei/`, read by a client reload listener (the fork's `add_client_reload_listeners`), so a mod
//! needs no code and no dependency on this crate. README.md documents the format; in short:
//!
//! - `jei/categories/<name>.json`: a category. `recipe_type` names the recipes it shows (read with
//!   `Host::recipes_of_type`, a mod's own type included); `recipes` adds inline ones. `slots`, `fluids` and `texts`
//!   pick the parts of each recipe's JSON by path (`ingredients[0]`, `result`, `ingredients[*]`). A file naming a
//!   vanilla category (`minecraft:smelting`) with only `catalysts` adds stations to it.
//! - `jei/info/<name>.json`: JEI's Information page for items: `{"items": [..], "text": ["key", ..]}`.

use crate::vanilla::Ctx;
use crate::view::{Card, Category, Role, Well};
use mod_api::serde_json::{self, Value};
use mod_api::*;
use std::sync::{Arc, Mutex};

/// The files as last read: (resource id, JSON). Replaced whole on each resource reload.
pub(crate) static FILES: Mutex<Vec<(String, Value)>> = Mutex::new(Vec::new());
/// Bumped on each reload, so the window's index is made again.
pub(crate) static VERSION: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

pub(crate) struct Listener;

impl ClientReloadListener for Listener {
    fn prepare(&self, context: &ClientReloadContext) -> Result<Box<dyn FnOnce() + Send + 'static>, ReloadResourceError> {
        let mut files = Vec::new();
        for id in context.list() {
            let Some(bytes) = context.read(id) else { continue };
            match serde_json::from_slice::<Value>(bytes) {
                Ok(v) => files.push((id.to_string(), v)),
                Err(e) => eprintln!("[jei] {id}: not JSON ({e}); skipped"),
            }
        }
        files.sort_by(|a, b| a.0.cmp(&b.0));
        Ok(Box::new(move || {
            *FILES.lock().unwrap_or_else(|e| e.into_inner()) = files;
            VERSION.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        }))
    }
}

pub(crate) fn listener() -> Arc<Listener> {
    Arc::new(Listener)
}

/// The id a file's category gets when it names none: `<ns>:<name>` from `<ns>:categories/<name>`.
fn file_name(id: &str) -> String {
    let (ns, path) = id.split_once(':').unwrap_or(("minecraft", id));
    format!("{ns}:{}", path.rsplit('/').next().unwrap_or(path))
}

/// Adds the providers' categories and cards to vanilla's. Information pages come last.
pub(crate) fn add(cx: &Ctx<'_>, categories: &mut Vec<Category>, cards: &mut Vec<Card>) {
    let files = FILES.lock().unwrap_or_else(|e| e.into_inner()).clone();
    for (id, v) in files.iter().filter(|(id, _)| id.contains(":categories/")) {
        let cat_id = v["id"].as_str().map(str::to_owned).unwrap_or_else(|| file_name(id));
        let catalysts: Vec<ItemKey> = strings(&v["catalysts"]).iter().filter_map(|c| cx.key(c)).collect();
        let index = match categories.iter().position(|c| c.id == cat_id) {
            Some(i) => {
                // A vanilla (or earlier) category: more stations, and its icon when it had none.
                let c = &mut categories[i];
                for k in catalysts {
                    if !c.catalysts.contains(&k) {
                        c.catalysts.push(k);
                    }
                }
                i
            }
            None => {
                let icon = v["icon"].as_str().and_then(|i| cx.key(i)).or_else(|| catalysts.first().copied());
                let title = v["title"].as_str().unwrap_or(&cat_id).to_owned();
                categories.push(Category { id: cat_id.clone(), title, icon, catalysts });
                categories.len() - 1
            }
        };
        if v["slots"].is_null() && v["fluids"].is_null() && v["texts"].is_null() {
            continue;
        }
        let mut recipes: Vec<(String, Value)> = Vec::new();
        for kind in strings(&v["recipe_type"]) {
            for r in cx.host.recipes_of_type(&kind).iter() {
                recipes.push((r.id.clone(), (*r.json).clone()));
            }
        }
        if let Some(list) = v["recipes"].as_array() {
            for (i, r) in list.iter().enumerate() {
                recipes.push((format!("{cat_id}#{i}"), r.clone()));
            }
        }
        for (rid, r) in recipes {
            if let Some(card) = card(cx, index, v, &rid, &r) {
                cards.push(card);
            }
        }
    }
    // Information (JEI's IngredientInfoRecipe): one card per item, its text wrapped when drawn.
    let mut info = None;
    for (_, v) in files.iter().filter(|(id, _)| id.contains(":info/")) {
        let index = *info.get_or_insert_with(|| {
            categories.push(Category {
                id: "jei:information".to_owned(),
                title: "jei.category.itemInformation".to_owned(),
                icon: cx.key("minecraft:book"),
                catalysts: Vec::new(),
            });
            categories.len() - 1
        });
        let text = strings(&v["text"]);
        for item in strings(&v["items"]).iter().filter_map(|i| cx.key(i)) {
            let mut c = Card::new(index, [160.0, 120.0], "");
            c.slot([72.0, 1.0], Role::Output, vec![(item, 1)]);
            for (i, line) in text.iter().enumerate() {
                // Wrapped and translated when drawn: a "§i" marker keeps the paragraph order.
                c.texts.push(([0.0, 24.0 + i as f32], format!("\u{1}{line}"), 0xFF40_4040));
            }
            cards.push(c);
        }
    }
}

/// The recipe types the providers' categories read (`recipe_type`), for the index to notice when their recipes
/// arrive. // jeiA
pub(crate) fn recipe_types() -> Vec<String> {
    let files = FILES.lock().unwrap_or_else(|e| e.into_inner());
    let mut out: Vec<String> = Vec::new();
    for (_, v) in files.iter().filter(|(id, _)| id.contains(":categories/")) {
        for k in strings(&v["recipe_type"]) {
            if !out.contains(&k) {
                out.push(k);
            }
        }
    }
    out
}

fn strings(v: &Value) -> Vec<String> {
    match v {
        Value::String(s) => vec![s.clone()],
        Value::Array(a) => a.iter().filter_map(Value::as_str).map(str::to_owned).collect(),
        _ => Vec::new(),
    }
}

/// The values a path picks from a recipe's JSON: `a.b[0]`, `a[*]` (every element), or `a` (an array spreads).
pub(crate) fn pick<'a>(v: &'a Value, path: &str) -> Vec<&'a Value> {
    let mut at = vec![v];
    for part in path.split('.').filter(|p| !p.is_empty()) {
        let (name, index) = match part.find('[') {
            Some(i) => (&part[..i], Some(part[i + 1..].trim_end_matches(']'))),
            None => (part, None),
        };
        let mut next = Vec::new();
        for v in at {
            let v = if name.is_empty() { v } else { &v[name] };
            match index {
                Some("*") => next.extend(v.as_array().into_iter().flatten()),
                Some(i) => {
                    if let Some(x) = i.parse::<usize>().ok().and_then(|i| v.get(i)) {
                        next.push(x)
                    }
                }
                None if !v.is_null() => next.push(v),
                None => {}
            }
        }
        at = next;
    }
    at
}

fn f(v: &Value, key: &str, default: f32) -> f32 {
    v[key].as_f64().map(|n| n as f32).unwrap_or(default)
}

fn role(v: &Value) -> Role {
    match v["role"].as_str() {
        Some("output") => Role::Output,
        Some("input") | None => Role::Input,
        _ => Role::Other,
    }
}

/// A fluid stack in the shapes recipe files use: `{"fluid"|"id": .., "amount": n}`, or an id.
pub(crate) fn fluid(v: &Value) -> Option<(String, u32)> {
    if let Some(id) = v.as_str() {
        return Some((id.to_owned(), 1000));
    }
    let id = v.get("fluid").or_else(|| v.get("id")).or_else(|| v.get("name")).and_then(Value::as_str)?;
    let amount = v.get("amount").and_then(Value::as_u64).unwrap_or(1000) as u32;
    let id = if id.contains(':') { id.to_owned() } else { format!("minecraft:{id}") };
    Some((id, amount))
}

/// One card from a category file and one recipe's JSON.
fn card(cx: &Ctx<'_>, index: usize, cat: &Value, id: &str, r: &Value) -> Option<Card> {
    let size = [
        cat["size"][0].as_f64().unwrap_or(120.0) as f32,
        cat["size"][1].as_f64().unwrap_or(40.0) as f32,
    ];
    let mut c = Card::new(index, size, id);
    if let Some(arrow) = cat["arrow"].as_array() {
        c.arrow = Some([arrow.first()?.as_f64()? as f32, arrow.get(1)?.as_f64()? as f32]);
    }
    c.shapeless = cat["shapeless"].as_bool().unwrap_or(false);
    if let Some(bg) = cat["background"].as_object() {
        let uv: Vec<f32> = bg.get("uv").and_then(Value::as_array).into_iter().flatten()
            .filter_map(Value::as_f64).map(|n| n as f32).collect();
        let tex: Vec<f32> = bg.get("texture_size").and_then(Value::as_array).into_iter().flatten()
            .filter_map(Value::as_f64).map(|n| n as f32).collect();
        if let Some(sprite) = bg.get("sprite").and_then(Value::as_str) {
            let uv = if uv.len() == 4 { [uv[0], uv[1], uv[2], uv[3]] } else { [0.0, 0.0, size[0], size[1]] };
            let tex = if tex.len() == 2 { [tex[0], tex[1]] } else { [256.0, 256.0] };
            c.background = Some((sprite.to_owned(), uv, tex));
        }
    }
    let mut any = false;
    for s in cat["slots"].as_array().into_iter().flatten() {
        let path = s["path"].as_str().unwrap_or("");
        let (x, y) = (f(s, "x", 0.0), f(s, "y", 0.0));
        let (columns, step) = (s["columns"].as_u64().unwrap_or(9).max(1) as usize, f(s, "step", 18.0));
        let output = role(s) == Role::Output;
        let picked = pick(r, path);
        // An array (or `[*]`) spreads over consecutive slots, `columns` to a row.
        let values: Vec<&Value> = if picked.len() == 1 && picked[0].is_array() && !s["one"].as_bool().unwrap_or(false) {
            picked[0].as_array().map(|a| a.iter().collect()).unwrap_or_default()
        } else {
            picked
        };
        let max = s["max"].as_u64().map(|m| m as usize).unwrap_or(usize::MAX);
        for (i, v) in values.into_iter().take(max).enumerate() {
            let stacks = if output { cx.result_json(v) } else { cx.ingredient_json(v) };
            let stacks = if stacks.is_empty() && output { cx.ingredient_json(v) } else { stacks };
            if stacks.is_empty() && s["keep_empty"].as_bool() != Some(true) {
                continue;
            }
            any |= !stacks.is_empty();
            let at = [x + (i % columns) as f32 * step, y + (i / columns) as f32 * step];
            let slot = c.slot(at, role(s), stacks);
            if let Some(chance) = s["chance"].as_str().and_then(|p| pick(v, p).first().and_then(|n| n.as_f64()))
                .or_else(|| v.get("chance").and_then(Value::as_f64))
            {
                slot.chance = Some(chance as f32);
            }
        }
    }
    for w in cat["fluids"].as_array().into_iter().flatten() {
        let path = w["path"].as_str().unwrap_or("");
        for (i, v) in pick(r, path).into_iter().enumerate() {
            let Some((fluid, amount)) = fluid(v) else { continue };
            any = true;
            let (x, y) = (f(w, "x", 0.0) + i as f32 * f(w, "step", 18.0), f(w, "y", 0.0));
            c.wells.push(Well {
                rect: [x, y, f(w, "w", 16.0), f(w, "h", 16.0)],
                role: role(w),
                fluid,
                amount,
                capacity: w["capacity"].as_u64().map(|c| c as u32).unwrap_or(amount).max(1),
                bare: false,
                overlay: None,
            });
        }
    }
    for t in cat["texts"].as_array().into_iter().flatten() {
        // `text` is a translation key with %s for each of `args` (paths into the recipe), or a literal.
        let args: Vec<String> = t["args"].as_array().into_iter().flatten().filter_map(Value::as_str)
            .map(|p| pick(r, p).first().map(|v| match v { Value::String(s) => s.clone(), other => other.to_string() }).unwrap_or_default())
            .collect();
        // `tr_args`: the args whose values are translation keys themselves (by index), translated on the client.
        let tr: Vec<u64> = t["tr_args"].as_array().into_iter().flatten().filter_map(Value::as_u64).collect();
        let args: Vec<String> = args.into_iter().enumerate().map(|(i, a)| if tr.contains(&(i as u64)) { format!("\u{1}{a}") } else { a }).collect();
        if t["needs"].as_str().is_some_and(|p| pick(r, p).is_empty()) {
            continue;
        }
        let colour = t["color"].as_u64().map(|c| c as u32).unwrap_or(0xFF40_4040);
        let text = format!("\u{2}{}\u{3}{}", t["text"].as_str().unwrap_or(""), args.join("\u{3}"));
        c.texts.push(([f(t, "x", 0.0), f(t, "y", 0.0)], text, colour));
    }
    any.then_some(c)
}

/// Adds what mods registered in code (`api.rs`): categories, catalysts, recipes and information.
pub(crate) fn add_api(cx: &Ctx<'_>, categories: &mut Vec<Category>, cards: &mut Vec<Card>) {
    use crate::api::{self, SlotRole};
    let (defs, catalysts, fns, info) =
        api::with(|r| (r.categories.clone(), r.catalysts.clone(), r.recipes.clone(), r.info.clone()));
    let role = |r: SlotRole| match r {
        SlotRole::Input => Role::Input,
        SlotRole::Output => Role::Output,
        SlotRole::Other => Role::Other,
    };
    for d in defs {
        let keys: Vec<ItemKey> = d.catalysts.iter().filter_map(|c| cx.key(c)).collect();
        match categories.iter_mut().find(|c| c.id == d.id) {
            Some(c) => c.catalysts.extend(keys.into_iter().filter(|k| !c.catalysts.contains(k)).collect::<Vec<_>>()),
            None => categories.push(Category { id: d.id.clone(), title: d.title.clone(), icon: cx.key(&d.icon), catalysts: keys }),
        }
    }
    for (cat, item) in catalysts {
        if let (Some(c), Some(k)) = (categories.iter_mut().find(|c| c.id == cat), cx.key(&item))
            && !c.catalysts.contains(&k)
        {
            c.catalysts.push(k);
        }
    }
    for (_, f) in fns {
        for (cat, def) in f(cx.host) {
            let Some(index) = categories.iter().position(|c| c.id == cat) else {
                eprintln!("[jei] recipe {} names category {cat}, which nobody registered", def.id);
                continue;
            };
            let mut c = Card::new(index, def.size, &def.id);
            c.arrow = def.arrow;
            c.shapeless = def.shapeless;
            c.background = def.background.map(|(s, uv)| (s, uv, [256.0, 256.0]));
            for s in def.slots {
                let mut stacks = Vec::new();
                let mut comps = Vec::new();
                for (spec, n) in &s.items {
                    let (id, json) = api::split_spec(spec);
                    if let Some(k) = cx.key(id) {
                        stacks.push((k, *n));
                        comps.push(json.to_owned());
                    }
                }
                let slot = c.slot([s.x, s.y], role(s.role), stacks);
                slot.chance = s.chance;
                slot.components = comps;
            }
            for (i, w) in def.fluids.into_iter().enumerate() {
                let look = def.bare_fluids.iter().find(|(n, _)| *n == i);
                c.wells.push(Well {
                    rect: w.rect,
                    role: role(w.role),
                    fluid: w.fluid,
                    amount: w.amount,
                    capacity: w.capacity,
                    bare: look.is_some(),
                    overlay: look.and_then(|(_, o)| o.clone()),
                });
            }
            c.icons = def.icons.iter().filter_map(|(id, at)| cx.key(id).map(|k| (k, *at))).collect();
            c.animated = def.animated;
            c.frameless = def.frameless;
            c.sprites_under = def.sprites_under;
            c.centered = def.centered.into_iter().map(|(at, t, colour)| (at, format!("\u{2}{t}\u{3}"), colour)).collect();
            for (at, text, colour) in def.texts {
                c.texts.push((at, format!("\u{2}{text}\u{3}"), colour));
            }
            c.hovers = def.hovers;
            c.sprites = def.sprites;
            cards.push(c);
        }
    }
    if !info.is_empty() {
        let index = match categories.iter().position(|c| c.id == "jei:information") {
            Some(i) => i,
            None => {
                categories.push(Category {
                    id: "jei:information".to_owned(),
                    title: "jei.category.itemInformation".to_owned(),
                    icon: cx.key("minecraft:book"),
                    catalysts: Vec::new(),
                });
                categories.len() - 1
            }
        };
        for (items, lines) in info {
            for item in items.iter().filter_map(|i| cx.key(i)) {
                let mut c = Card::new(index, [160.0, 120.0], "");
                c.slot([72.0, 1.0], Role::Output, vec![(item, 1)]);
                for (i, line) in lines.iter().enumerate() {
                    c.texts.push(([0.0, 24.0 + i as f32], format!("\u{1}{line}"), 0xFF40_4040));
                }
                cards.push(c);
            }
        }
    }
}
