//! The client half of the sources: the server's index arrives on `jei:sources` in parts, and becomes JEI categories
//! and cards (World Gen, Block Drops, Mob Drops, Chest Loot, Archaeology, Fishing, Piglin Bartering, Shearing,
//! Villager Trades, Other Sources, How To Get It), so R and U find them as they find recipes. The pages follow the
//! Horizon JEI kit's (Just Enough Resources' layout).

use super::index::{self, DIMENSIONS, Drop, Group, Index, Source, WORLD};
use super::math::{percent, title_case, trim_count};
use crate::vanilla::Ctx;
use crate::view::{Card, Category, Graph, Role};
use mod_api::serde_json::Value;
use mod_api::*;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::{Arc, Mutex};

const TEXT: u32 = 0xFF40_4040;
const STEP: f32 = 24.0;

/// The index, once every part of a generation arrived.
static INDEX: Mutex<Option<Arc<Index>>> = Mutex::new(None);
/// Bumped when a new index arrives: the cards are made again.
pub(crate) static VERSION: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
/// Parts of the generation arriving: generation, total, parts by number.
static PARTS: Mutex<Option<(u64, usize, BTreeMap<usize, Vec<u8>>)>> = Mutex::new(None);
/// When the client last asked.
static ASKED: Mutex<u64> = Mutex::new(0);

/// Asks the server for the index every five seconds until it has one (from `render_frame_pre`).
pub(crate) fn request(host: &mut dyn ClientHost, millis: u64) {
    if INDEX.lock().unwrap_or_else(|e| e.into_inner()).is_some() {
        return;
    }
    let mut asked = ASKED.lock().unwrap_or_else(|e| e.into_inner());
    if *asked != 0 && millis < *asked + 5000 {
        return;
    }
    *asked = millis.max(1);
    let _ = host.send_payload_to_server(&PayloadId(super::server::CHANNEL.into()), PayloadBytes(b"?".to_vec()));
}

/// A new world: the index is asked for again.
pub(crate) fn forget() {
    *INDEX.lock().unwrap_or_else(|e| e.into_inner()) = None;
    *PARTS.lock().unwrap_or_else(|e| e.into_inner()) = None;
    *ASKED.lock().unwrap_or_else(|e| e.into_inner()) = 0;
    VERSION.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
}

pub(crate) fn status() -> String {
    match INDEX.lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
        Some(i) => format!(
            "sources: {} items' drops, {} ores, {} natural blocks, {} mobs' spawns, {} trades",
            i.drops.len(),
            i.ores.len(),
            i.natural.len(),
            i.spawns.len(),
            i.trades.len()
        ),
        None => {
            let parts = PARTS.lock().unwrap_or_else(|e| e.into_inner());
            match parts.as_ref() {
                Some((g, total, got)) => format!("sources: generation {g}, {} of {total} parts", got.len()),
                None => "sources: none yet".to_owned(),
            }
        }
    }
}

/// `jei:sources` on the client: one part of the index.
pub(crate) struct FromServer;

impl IPayloadHandler for FromServer {
    fn handle(&self, payload: PayloadBytes, _cx: &mut dyn IPayloadContext) -> Result<(), PayloadHandleError> {
        receive(&payload.0);
        Ok(())
    }
}

fn receive(data: &[u8]) {
    let Some(nl) = data.iter().position(|b| *b == b'\n') else { return };
    let head = String::from_utf8_lossy(&data[..nl]);
    let mut nums = head.split(' ').filter_map(|n| n.parse::<u64>().ok());
    let (Some(generation), Some(i), Some(total)) = (nums.next(), nums.next(), nums.next()) else { return };
    let (i, total) = (i as usize, total as usize);
    let mut parts = PARTS.lock().unwrap_or_else(|e| e.into_inner());
    if parts.as_ref().is_none_or(|(g, t, _)| *g != generation || *t != total) {
        *parts = Some((generation, total, BTreeMap::new()));
    }
    let Some((_, _, got)) = parts.as_mut() else { return };
    got.insert(i, data[nl + 1..].to_vec());
    if got.len() == total {
        let bytes: Vec<u8> = got.values().flat_map(|p| p.iter().copied()).collect();
        *parts = None;
        if let Some(index) = Index::from_json(&String::from_utf8_lossy(&bytes)) {
            *INDEX.lock().unwrap_or_else(|e| e.into_inner()) = Some(Arc::new(index));
            VERSION.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        }
    }
}

/// A trade card's villager: `villager:<profession>:<level>` becomes its `VillagerData`.
pub(crate) fn snbt(spec: &str) -> Option<Compound> {
    let mut parts = spec.split(':');
    if parts.next()? != "villager" {
        return None;
    }
    let profession = parts.next()?;
    let level: i32 = parts.next().and_then(|l| l.parse().ok()).unwrap_or(1);
    let mut data = Compound::new();
    data.insert("profession".into(), Tag::String(format!("minecraft:{profession}").as_str().into()));
    data.insert("level".into(), Tag::Int(level));
    data.insert("type".into(), Tag::String("minecraft:plains".into()));
    let mut root = Compound::new();
    root.insert("VillagerData".into(), Tag::Compound(data));
    Some(root)
}

// ---------------------------------------------------------------------------------------------------------
// The cards.

const CATEGORIES: [(&str, &str, &str, &[&str]); 11] = [
    ("jei:world_gen", "World Gen", "minecraft:grass_block", &[]),
    ("jei:block_drops", "Block Drops", "minecraft:diamond_pickaxe", &["minecraft:diamond_pickaxe"]),
    ("jei:mob_drops", "Mob Drops", "minecraft:zombie_head", &[]),
    ("jei:chest_loot", "Chest Loot", "minecraft:chest", &["minecraft:chest"]),
    ("jei:archaeology", "Archaeology", "minecraft:brush", &["minecraft:brush"]),
    ("jei:fishing", "Fishing", "minecraft:fishing_rod", &["minecraft:fishing_rod"]),
    ("jei:bartering", "Piglin Bartering", "minecraft:gold_ingot", &["minecraft:gold_ingot"]),
    ("jei:shearing", "Shearing", "minecraft:shears", &["minecraft:shears"]),
    ("jei:villager_trades", "Villager Trades", "minecraft:emerald", &[]),
    ("jei:other_sources", "Other Sources", "minecraft:knowledge_book", &[]),
    ("jei:how_to_get", "How To Get It", "minecraft:book", &[]),
];

/// What a drop needs or gets besides: Silk Touch, shears, Looting, Fortune.
fn extras(flags: u8) -> String {
    let mut out = String::new();
    for (bit, text) in [
        (index::SILK, ", Silk Touch"),
        (index::SHEARS, ", shears"),
        (index::PLAYER, ", player kill"),
        (index::FORTUNE, ", more with Fortune"),
        (index::LOOTING, ", more with Looting"),
        (index::MAYBE, ", sometimes"),
    ] {
        if flags & bit != 0 {
            out.push_str(text);
        }
    }
    out
}

fn amount(d: &Drop) -> String {
    if d.low == d.high { format!("x{}", d.high.max(1)) } else { format!("x{}-{}", d.low, d.high) }
}

fn short(id: &str) -> &str {
    id.split_once(':').map_or(id, |p| p.1)
}

/// A list of biomes as a line: "All biomes" when it is every one of the dimension.
fn biome_list<'a>(biomes: impl Iterator<Item = &'a String>, of: usize) -> String {
    let list: Vec<&String> = biomes.collect();
    if list.len() >= of && of > 0 {
        return "All biomes".to_owned();
    }
    let names: Vec<String> = list.iter().take(6).map(|b| title_case(short(b))).collect();
    let more = list.len().saturating_sub(names.len());
    if more > 0 { format!("{} and {more} more", names.join(", ")) } else { names.join(", ") }
}

/// An entity's name in the game's language (resolved when drawn).
fn entity_name(id: &str) -> String {
    let (ns, path) = id.split_once(':').unwrap_or(("minecraft", id));
    format!("\u{2}%s\u{3}\u{1}entity.{ns}.{path}")
}

/// The item stacks a drop's item stands for: itself, or a tag's items.
fn stacks(cx: &Ctx<'_>, tags: &mut HashMap<String, Vec<ItemKey>>, item: &str, count: u32) -> Vec<(ItemKey, u32)> {
    if let Some(tag) = item.strip_prefix('#') {
        let keys = tags
            .entry(tag.to_owned())
            .or_insert_with(|| cx.ingredient_json(&Value::String(format!("#{tag}"))).into_iter().map(|(k, _)| k).collect());
        return keys.iter().map(|k| (*k, count)).collect();
    }
    cx.key(item).map(|k| vec![(k, count)]).unwrap_or_default()
}

/// A slot of drops: its stacks, the chance under it, the tooltip saying the count and what it needs.
fn drop_slot(c: &mut Card, at: [f32; 2], list: Vec<(ItemKey, u32)>, d: &Drop) {
    let line = format!("{} {}{}", percent(d.chance), amount(d), extras(d.flags));
    let s = c.slot(at, Role::Output, list);
    s.chance = Some(d.chance.min(1.0) as f32);
    c.hovers.push(([at[0] - 1.0, at[1] - 1.0, 18.0, 18.0], vec![line]));
}

/// Adds the sources' categories and cards (from `ensure_index`).
pub(crate) fn add(cx: &Ctx<'_>, categories: &mut Vec<Category>, cards: &mut Vec<Card>) {
    let Some(index) = INDEX.lock().unwrap_or_else(|e| e.into_inner()).clone() else { return };
    let base = categories.len();
    for (id, title, icon, catalysts) in CATEGORIES {
        categories.push(Category {
            id: id.to_owned(),
            title: title.to_owned(),
            icon: cx.key(icon),
            catalysts: catalysts.iter().filter_map(|c| cx.key(c)).collect(),
        });
    }
    let cat = |id: &str| base + CATEGORIES.iter().position(|c| c.0 == id).unwrap_or(0);
    let mut tags: HashMap<String, Vec<ItemKey>> = HashMap::new();

    // Who drops what: the drops by their source.
    let mut by_block: BTreeMap<String, Vec<(String, Drop)>> = BTreeMap::new();
    let mut by_mob: BTreeMap<String, Vec<(String, Drop)>> = BTreeMap::new();
    let mut by_table: BTreeMap<(Group, String), Vec<(String, Drop)>> = BTreeMap::new();
    for (item, list) in &index.drops {
        for d in list {
            let e = (item.clone(), d.clone());
            match &d.source {
                Source::Block(b) => by_block.entry(b.clone()).or_default().push(e),
                Source::Mob(m) => by_mob.entry(m.clone()).or_default().push(e),
                Source::Table(g, id) => by_table.entry((*g, id.clone())).or_default().push(e),
            }
        }
    }
    let sort = |list: &mut Vec<(String, Drop)>| list.sort_by(|a, b| b.1.chance.total_cmp(&a.1.chance).then_with(|| a.0.cmp(&b.0)));

    // World Gen: each ore's graph, then where other blocks generate.
    let world = cat("jei:world_gen");
    let mut graphed: HashSet<(String, usize)> = HashSet::new();
    let mut ore_blocks: Vec<&String> = index.ores.keys().collect();
    ore_blocks.sort();
    for block in ore_blocks {
        let Some(key) = cx.key(block) else { continue };
        let mut ores = index.ores[block].clone();
        ores.sort_by(|a, b| a.dimension.cmp(&b.dimension).then_with(|| a.biomes.is_some().cmp(&b.biomes.is_some())));
        let mut drops: Vec<(String, Drop)> = by_block.get(block).cloned().unwrap_or_default();
        drops.retain(|(_, d)| d.flags & (index::SILK | index::SHEARS) == 0);
        sort(&mut drops);
        for ore in ores {
            graphed.insert((block.clone(), ore.dimension));
            let mut c = Card::new(world, [160.0, 92.0], &format!("jei:world_gen/{}", short(block)));
            c.slot([1.0, 1.0], Role::Output, vec![(key, 1)]);
            for (i, (item, d)) in drops.iter().filter(|(i, _)| i != block).take(3).enumerate() {
                let list = stacks(cx, &mut tags, item, d.high.max(1));
                if !list.is_empty() {
                    drop_slot(&mut c, [141.0 - i as f32 * STEP, 1.0], list, d);
                }
            }
            let scope = match &ore.biomes {
                None => "All biomes".to_owned(),
                Some(b) => biome_list(b.iter(), index.biome_counts[ore.dimension]),
            };
            c.texts.push(([22.0, 2.0], DIMENSIONS[ore.dimension].to_owned(), TEXT));
            c.texts.push(([22.0, 11.0], clip(&scope, 24), 0xFF60_6060));
            let (bottom, height) = WORLD[ore.dimension];
            c.graph = Some(Graph { rect: [1.0, 22.0, 158.0, 34.0], values: ore.layers.clone(), bottom, top: bottom + height - 1, peak: ore.peak });
            c.texts.push(([1.0, 72.0], format!("{} of blocks, Y {} to {}", percent(ore.share), ore.low, ore.high), TEXT));
            c.texts.push(([1.0, 82.0], format!("Most at Y {}, about {} a chunk", ore.peak, trim_count(ore.per_chunk)), TEXT));
            cards.push(c);
        }
    }
    let mut natural: Vec<(&String, &index::Natural)> = index.natural.iter().collect();
    natural.sort_by(|a, b| a.0.cmp(b.0));
    for (block, n) in natural {
        let Some(key) = cx.key(block) else { continue };
        let mut lines: Vec<String> = Vec::new();
        for d in &n.terrain {
            lines.push(format!("{}: part of the terrain", DIMENSIONS[*d]));
        }
        for (d, biomes) in &n.biomes {
            if !graphed.contains(&(block.clone(), *d)) && !n.terrain.contains(d) {
                lines.push(format!("{}: {}", DIMENSIONS[*d], biome_list(biomes.iter(), index.biome_counts[*d])));
            }
        }
        if !n.structures.is_empty() {
            let list: Vec<&str> = n.structures.iter().map(String::as_str).collect();
            lines.push(format!("In structures: {}", list.join(", ")));
        }
        if lines.is_empty() {
            continue;
        }
        let mut c = Card::new(world, [160.0, 20.0], &format!("jei:natural/{}", short(block)));
        c.slot([1.0, 1.0], Role::Output, vec![(key, 1)]);
        c.texts.push(([22.0, 5.0], "Generates in the world".to_owned(), TEXT));
        for l in lines {
            c.texts.push(([0.0, 0.0], format!("\u{1}{l}"), TEXT));
        }
        cards.push(c);
    }

    // Block Drops: one card a block, its drops beside it.
    let blocks = cat("jei:block_drops");
    for (block, mut list) in by_block {
        // A block that simply drops itself, as JEI's own lists never show: only blocks that need a tool
        // (silk touch, shears), drop something else, or drop by chance or in numbers get a card.
        let plain = |(item, d): &(String, Drop)| {
            *item == block && d.flags & (index::SILK | index::SHEARS) == 0 && d.chance >= 1.0 && d.low <= 1 && d.high <= 1
        };
        if list.iter().all(plain) {
            continue;
        }
        sort(&mut list);
        let first = list.first().map(|(i, _)| i.clone());
        let icon = cx.key(&block).or_else(|| first.and_then(|i| cx.key(&i)));
        let Some(icon) = icon else { continue };
        let per_row = 5;
        let rows = list.len().div_ceil(per_row).max(1);
        let mut c = Card::new(blocks, [52.0 + per_row as f32 * STEP, 24.0 + rows as f32 * 28.0 - 6.0], &format!("jei:block_drops/{}", short(&block)));
        let (ns, path) = block.split_once(':').unwrap_or(("minecraft", &block));
        c.texts.push(([1.0, 1.0], format!("\u{2}%s\u{3}\u{1}block.{ns}.{path}"), TEXT));
        c.slot([1.0, 15.0], Role::Input, vec![(icon, 1)]);
        c.arrow = Some([22.0, 14.0]);
        for (i, (item, d)) in list.iter().enumerate() {
            let list = stacks(cx, &mut tags, item, d.high.max(1));
            if list.is_empty() {
                continue;
            }
            let at = [52.0 + (i % per_row) as f32 * STEP, 15.0 + (i / per_row) as f32 * 28.0];
            drop_slot(&mut c, at, list, d);
        }
        cards.push(c);
    }

    // Mob Drops: one card a mob, drawn, with where it spawns and its drops; its spawn egg goes in.
    let mobs = cat("jei:mob_drops");
    for (mob, mut list) in by_mob {
        sort(&mut list);
        let mut seen = HashSet::new();
        list.retain(|(i, _)| seen.insert(i.clone()));
        for (item, m, note) in CODE_DROPS {
            if *m == mob && !list.iter().any(|(i, _)| i == item) {
                list.push(((*item).to_owned(), Drop { source: Source::Mob(mob.clone()), chance: 0.0, low: 1, high: 1, flags: index::MAYBE }));
                let _ = note;
            }
        }
        let per_row = 5;
        let rows = list.len().div_ceil(per_row).max(1);
        let height = (46.0 + rows as f32 * 28.0).max(80.0);
        let mut c = Card::new(mobs, [46.0 + per_row as f32 * STEP, height], &format!("jei:mob_drops/{}", short(&mob)));
        c.entity = Some((mob.clone(), String::new(), [1.0, 1.0, 40.0, 56.0]));
        c.texts.push(([46.0, 2.0], entity_name(&mob), TEXT));
        let (category, by_dim) = index.spawns.get(&mob).cloned().unwrap_or_default();
        let mut parts = Vec::new();
        for (d, biomes) in by_dim.iter().enumerate() {
            if !biomes.is_empty() {
                let l = biome_list(biomes.iter(), index.biome_counts[d]);
                parts.push(if l == "All biomes" { DIMENSIONS[d].to_owned() } else { l });
            }
        }
        if let Some(list) = index.structure_spawns.get(&mob) {
            let list: Vec<&str> = list.iter().map(String::as_str).collect();
            parts.push(format!("in {}", list.join(", ")));
        }
        if parts.is_empty()
            && let Some((_, placed)) = PLACED.iter().find(|(m, _)| *m == mob)
        {
            parts.push((*placed).to_owned());
        }
        let spawn = if parts.is_empty() { "Spawns: other ways (events, spawners)".to_owned() } else { format!("Spawns: {}", parts.join("; ")) };
        let mut info = vec![clip(&spawn, 26)];
        if spawn.chars().count() > 26 {
            c.hovers.push(([46.0, 11.0, 120.0, 10.0], vec![spawn.clone()]));
        }
        match category.as_str() {
            "monster" => info.push("Light: 0".to_owned()),
            "creature" => info.push("Light: 9 or more".to_owned()),
            _ => {}
        }
        info.push(format!("XP: {}", experience(&mob, &category)));
        for (i, line) in info.into_iter().enumerate() {
            c.texts.push(([46.0, 12.0 + i as f32 * 10.0], line, 0xFF60_6060));
        }
        if let Some(egg) = cx.key(&format!("{mob}_spawn_egg")) {
            c.slot([12.0, 60.0], Role::Input, vec![(egg, 1)]);
        }
        for (i, (item, d)) in list.iter().enumerate() {
            let stacks_of = stacks(cx, &mut tags, item, d.high.max(1));
            if stacks_of.is_empty() {
                continue;
            }
            let at = [46.0 + (i % per_row) as f32 * STEP, 46.0 + (i / per_row) as f32 * 28.0];
            if d.chance <= 0.0 {
                // A drop the game's code makes (a creeper's discs): its note instead of a chance.
                let note = CODE_DROPS.iter().find(|(i, m, _)| *i == item && *m == mob).map_or("", |n| n.2);
                c.slot(at, Role::Output, stacks_of);
                c.hovers.push(([at[0] - 1.0, at[1] - 1.0, 18.0, 18.0], vec![note.to_owned()]));
            } else {
                drop_slot(&mut c, at, stacks_of, d);
            }
        }
        cards.push(c);
    }

    // Chests, archaeology, fishing, bartering, shearing and the other tables: one card a table.
    for ((group, id), mut list) in by_table {
        let category = cat(match group {
            Group::Chest => "jei:chest_loot",
            Group::Archaeology => "jei:archaeology",
            Group::Fishing => "jei:fishing",
            Group::Barter => "jei:bartering",
            Group::Shearing => "jei:shearing",
            Group::Other => "jei:other_sources",
        });
        sort(&mut list);
        let path = short(&id);
        let path = ["gameplay/", "chests/", "archaeology/", "shearing/", "pots/", "dispensers/", "spawners/"].iter().find_map(|p| path.strip_prefix(p)).unwrap_or(path);
        let per_row = 7;
        let rows = list.len().div_ceil(per_row).max(1);
        let mut c = Card::new(category, [per_row as f32 * STEP, 14.0 + rows as f32 * 28.0], &id);
        c.texts.push(([1.0, 1.0], title_case(&path.replace('/', " ")), TEXT));
        for (i, (item, d)) in list.iter().enumerate() {
            let s = stacks(cx, &mut tags, item, d.high.max(1));
            if s.is_empty() {
                continue;
            }
            let at = [1.0 + (i % per_row) as f32 * STEP, 13.0 + (i / per_row) as f32 * 28.0];
            drop_slot(&mut c, at, s, d);
        }
        cards.push(c);
    }

    // Villager Trades: one card a trade, the villager of its profession drawn.
    let trades = cat("jei:villager_trades");
    for t in &index.trades {
        let Some(gives) = cx.key(&t.gives.0) else { continue };
        let wandering = t.who == "wandering_trader";
        let mut c = Card::new(trades, [140.0, 48.0], &format!("jei:trade/{}/{}", t.who, short(&t.gives.0)));
        let (kind, spec) = if wandering {
            ("minecraft:wandering_trader".to_owned(), String::new())
        } else {
            ("minecraft:villager".to_owned(), format!("villager:{}:{}", t.who, t.level.unwrap_or(1)))
        };
        c.entity = Some((kind, spec, [1.0, 1.0, 26.0, 46.0]));
        // The profession, and under it the level by the merchant screen's own names: the work station's icon
        // takes the card's top right, so both stay within the 94 pixels before it.
        let who = if wandering { "entity.minecraft.wandering_trader".to_owned() } else { format!("entity.minecraft.villager.{}", t.who) };
        c.texts.push(([30.0, 2.0], format!("\u{2}%s\u{3}\u{1}{who}"), TEXT));
        if let Some(l) = t.level.filter(|_| !wandering) {
            c.texts.push(([30.0, 11.0], format!("\u{2}%s\u{3}\u{1}merchant.level.{}", l.clamp(1, 5)), 0xFF60_6060));
        }
        for (i, (item, n)) in t.wants.iter().enumerate().take(2) {
            if let Some(k) = cx.key(item) {
                c.slot([30.0 + i as f32 * STEP, 24.0], Role::Input, vec![(k, *n)]);
            }
        }
        c.arrow = Some([74.0, 23.0]);
        c.slot([106.0, 24.0], Role::Output, vec![(gives, t.gives.1)]);
        if let Some((_, block)) = WORK_STATIONS.iter().find(|(w, _)| *w == t.who)
            && let Some(k) = cx.key(&format!("minecraft:{block}"))
        {
            c.slot([124.0, 2.0], Role::Other, vec![(k, 1)]);
        }
        cards.push(c);
    }

    // How To Get It: what no table says.
    let how = cat("jei:how_to_get");
    let mut notes: Vec<(String, &str)> = SPECIAL.iter().map(|(i, n)| ((*i).to_owned(), *n)).collect();
    for id in cx.ids.keys() {
        if id.ends_with("_spawn_egg") {
            notes.push((id.clone(), "Creative mode only."));
        }
    }
    // Concrete has no recipe: its powder hardens where it touches water. The powder goes in, so its uses find it.
    let mut concrete: Vec<&String> = cx.ids.keys().filter(|id| id.ends_with("_concrete_powder")).collect();
    concrete.sort();
    for powder in concrete {
        let hard = powder.trim_end_matches("_powder");
        let (Some(p), Some(k)) = (cx.key(powder), cx.key(hard)) else { continue };
        let mut c = Card::new(how, [160.0, 20.0], &format!("jei:how_to_get/{}", short(hard)));
        c.slot([1.0, 1.0], Role::Input, vec![(p, 1)]);
        c.arrow = Some([22.0, 0.0]);
        c.slot([52.0, 1.0], Role::Output, vec![(k, 1)]);
        c.texts.push(([0.0, 0.0], "\u{1}Concrete powder hardens into concrete where it touches water.".to_owned(), TEXT));
        cards.push(c);
    }
    notes.sort();
    for (item, note) in notes {
        let Some(k) = cx.key(&item) else { continue };
        let mut c = Card::new(how, [160.0, 20.0], &format!("jei:how_to_get/{}", short(&item)));
        c.slot([1.0, 1.0], Role::Output, vec![(k, 1)]);
        c.texts.push(([0.0, 0.0], format!("\u{1}{note}"), TEXT));
        cards.push(c);
    }
}

/// A line cut to `n` characters.
fn clip(text: &str, n: usize) -> String {
    if text.chars().count() <= n {
        return text.to_owned();
    }
    let mut s: String = text.chars().take(n.saturating_sub(1)).collect();
    s.push('…');
    s
}

/// `getBaseExperienceReward` of a kill, for the mobs that differ from their kind.
fn experience(mob: &str, category: &str) -> &'static str {
    match short(mob) {
        "ender_dragon" => "500 (12000 the first time)",
        "wither" => "50",
        "ravager" | "piglin_brute" => "20",
        "blaze" | "guardian" | "elder_guardian" | "evoker" | "illusioner" | "breeze" => "10",
        "slime" | "magma_cube" => "by size",
        "villager" | "wandering_trader" | "iron_golem" | "snow_golem" | "allay" | "bat" => "0",
        "zombie" | "husk" | "drowned" | "skeleton" | "stray" | "bogged" | "creeper" | "spider" | "cave_spider" | "enderman" | "ghast" | "hoglin" | "zoglin" | "phantom" | "piglin" | "pillager" | "shulker"
        | "silverfish" | "endermite" | "vex" | "vindicator" | "warden" | "witch" | "wither_skeleton" | "zombie_villager" | "zombified_piglin" => "5",
        _ if category == "monster" => "5",
        _ => "1-3",
    }
}

const WORK_STATIONS: [(&str, &str); 13] = [
    ("armorer", "blast_furnace"),
    ("butcher", "smoker"),
    ("cartographer", "cartography_table"),
    ("cleric", "brewing_stand"),
    ("farmer", "composter"),
    ("fisherman", "barrel"),
    ("fletcher", "fletching_table"),
    ("leatherworker", "cauldron"),
    ("librarian", "lectern"),
    ("mason", "stonecutter"),
    ("shepherd", "loom"),
    ("toolsmith", "smithing_table"),
    ("weaponsmith", "grindstone"),
];

const CODE_DROPS: &[(&str, &str, &str)] = &[
    ("minecraft:nether_star", "minecraft:wither", "Wither: always, x1"),
    ("minecraft:music_disc_11", "minecraft:creeper", "Creeper killed by a skeleton or bogged"),
    ("minecraft:music_disc_13", "minecraft:creeper", "Creeper killed by a skeleton or bogged"),
    ("minecraft:music_disc_blocks", "minecraft:creeper", "Creeper killed by a skeleton or bogged"),
    ("minecraft:music_disc_cat", "minecraft:creeper", "Creeper killed by a skeleton or bogged"),
    ("minecraft:music_disc_chirp", "minecraft:creeper", "Creeper killed by a skeleton or bogged"),
    ("minecraft:music_disc_far", "minecraft:creeper", "Creeper killed by a skeleton or bogged"),
    ("minecraft:music_disc_mall", "minecraft:creeper", "Creeper killed by a skeleton or bogged"),
    ("minecraft:music_disc_mellohi", "minecraft:creeper", "Creeper killed by a skeleton or bogged"),
    ("minecraft:music_disc_stal", "minecraft:creeper", "Creeper killed by a skeleton or bogged"),
    ("minecraft:music_disc_strad", "minecraft:creeper", "Creeper killed by a skeleton or bogged"),
    ("minecraft:music_disc_wait", "minecraft:creeper", "Creeper killed by a skeleton or bogged"),
    ("minecraft:music_disc_ward", "minecraft:creeper", "Creeper killed by a skeleton or bogged"),
];

/// Where the game places the mobs no biome spawns and no structure's spawn overrides name: the structures whose
/// pieces or spawners put them there, and the other ways vanilla makes them.
const PLACED: &[(&str, &str)] = &[
    ("minecraft:shulker", "in End Cities"),
    ("minecraft:elder_guardian", "in Ocean Monuments"),
    ("minecraft:evoker", "in Woodland Mansions; raids"),
    ("minecraft:vindicator", "in Woodland Mansions; raids"),
    ("minecraft:piglin_brute", "in Bastion Remnants"),
    ("minecraft:breeze", "from Trial Spawners in Trial Chambers"),
    ("minecraft:allay", "caged in Pillager Outposts, Woodland Mansions"),
    ("minecraft:iron_golem", "in Villages; built, or called by villagers"),
    ("minecraft:villager", "in Villages"),
    ("minecraft:zombie_villager", "in Igloos, abandoned Villages; zombies infect villagers"),
    ("minecraft:warden", "summoned by Sculk Shriekers in Ancient Cities"),
    ("minecraft:ender_dragon", "in The End"),
    ("minecraft:wither", "built from soul sand and wither skulls"),
    ("minecraft:snow_golem", "built from snow and a carved pumpkin"),
    ("minecraft:creaking", "from a Creaking Heart at night"),
    ("minecraft:silverfish", "from infested blocks and spawners in Strongholds"),
    ("minecraft:cave_spider", "from spawners in Mineshafts"),
    ("minecraft:blaze", "in Nether Fortresses"),
    ("minecraft:wandering_trader", "near players, with trader llamas"),
    ("minecraft:trader_llama", "with wandering traders"),
    ("minecraft:skeleton_horse", "skeleton trap lightning in thunderstorms"),
    ("minecraft:illusioner", "commands only"),
    ("minecraft:giant", "commands only"),
];

const SPECIAL: &[(&str, &str)] = &[
    ("minecraft:blue_egg", "Laid by cold chickens."),
    ("minecraft:brown_egg", "Laid by warm chickens."),
    ("minecraft:budding_amethyst", "Lines amethyst geodes; breaks to nothing, so it can't be collected in survival."),
    ("minecraft:chorus_plant", "Grows on end stone in the End Highlands; breaks into chorus fruit, so only creative mode holds it."),
    ("minecraft:dirt_path", "Use a shovel on dirt or grass; Silk Touch keeps it."),
    ("minecraft:farmland", "Use a hoe on dirt or grass; Silk Touch keeps it."),
    ("minecraft:frogspawn", "Laid by frogs on water; can't be collected in survival."),
    ("minecraft:flow_pottery_sherd", "From decorated pots in Trial Chambers."),
    ("minecraft:guster_pottery_sherd", "From decorated pots in Trial Chambers."),
    ("minecraft:scrape_pottery_sherd", "From decorated pots in Trial Chambers."),
    ("minecraft:infested_chiseled_stone_bricks", "Generated in strongholds; turns to a silverfish when broken, so only creative mode holds it."),
    ("minecraft:infested_cobblestone", "Generated in igloos and woodland mansions; can't be collected in survival."),
    ("minecraft:infested_cracked_stone_bricks", "Generated in strongholds; can't be collected in survival."),
    ("minecraft:infested_mossy_stone_bricks", "Generated in strongholds; can't be collected in survival."),
    ("minecraft:infested_stone_bricks", "Generated in strongholds; can't be collected in survival."),
    ("minecraft:infested_stone", "Generated in mountains; can't be collected in survival."),
    ("minecraft:infested_deepslate", "Generated in deep mountains; can't be collected in survival."),
    ("minecraft:reinforced_deepslate", "Around the Ancient City's center; can't be collected in survival."),
    ("minecraft:spawner", "Dungeons, strongholds, mineshafts, fortresses and bastions; can't be collected in survival."),
    ("minecraft:trial_spawner", "Trial Chambers; can't be collected in survival."),
    ("minecraft:vault", "Trial Chambers; can't be collected in survival."),
    ("minecraft:sulfur_cube_bucket", "Use a water bucket on a sulfur cube."),
    ("minecraft:suspicious_gravel", "Trail ruins and cold ocean ruins; only creative mode holds it. Use a brush for its loot."),
    ("minecraft:suspicious_sand", "Desert wells, pyramids and warm ocean ruins; only creative mode holds it. Use a brush for its loot."),
    ("minecraft:water_bucket", "Use a bucket on a water source."),
    ("minecraft:lava_bucket", "Use a bucket on a lava source."),
    ("minecraft:milk_bucket", "Use a bucket on a cow, goat or mooshroom."),
    ("minecraft:powder_snow_bucket", "Use a bucket on powder snow."),
    ("minecraft:cod_bucket", "Use a water bucket on a cod."),
    ("minecraft:salmon_bucket", "Use a water bucket on a salmon."),
    ("minecraft:pufferfish_bucket", "Use a water bucket on a pufferfish."),
    ("minecraft:tropical_fish_bucket", "Use a water bucket on a tropical fish."),
    ("minecraft:axolotl_bucket", "Use a water bucket on an axolotl."),
    ("minecraft:tadpole_bucket", "Use a water bucket on a tadpole."),
    ("minecraft:glass_bottle", "Crafted, or dropped by witches."),
    ("minecraft:honey_bottle", "Use a glass bottle on a full bee nest or beehive."),
    ("minecraft:dragon_breath", "Use a glass bottle in the Ender Dragon's breath."),
    ("minecraft:dragon_egg", "Appears on the exit portal after the first Ender Dragon kill."),
    ("minecraft:elytra", "In an item frame on End Ships, in End Cities."),
    ("minecraft:ominous_bottle", "Dropped by raid captains; found in ominous trial vaults."),
    ("minecraft:experience_bottle", "Clerics sell them; also found in chests."),
    ("minecraft:filled_map", "Use an empty map; cartographers sell explorer maps."),
    ("minecraft:written_book", "Sign a book and quill."),
    ("minecraft:firework_star", "Crafted with gunpowder and dye."),
    ("minecraft:player_head", "Dropped by a player killed by a charged creeper."),
    ("minecraft:command_block", "Creative mode only, with /give."),
    ("minecraft:chain_command_block", "Creative mode only, with /give."),
    ("minecraft:repeating_command_block", "Creative mode only, with /give."),
    ("minecraft:command_block_minecart", "Creative mode only, with /give."),
    ("minecraft:barrier", "Creative mode only, with /give."),
    ("minecraft:light", "Creative mode only, with /give."),
    ("minecraft:structure_block", "Creative mode only, with /give."),
    ("minecraft:structure_void", "Creative mode only, with /give."),
    ("minecraft:jigsaw", "Creative mode only, with /give."),
    ("minecraft:debug_stick", "Creative mode only, with /give."),
    ("minecraft:knowledge_book", "Creative mode only, with /give."),
    ("minecraft:test_block", "Creative mode only, with /give."),
    ("minecraft:test_instance_block", "Creative mode only, with /give."),
];
