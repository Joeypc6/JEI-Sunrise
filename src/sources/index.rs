//! Where every item comes from, worked out on the server from its own data packs (vanilla's included, and any a
//! world adds): loot tables give the blocks, mobs, chests and other sources of an item with chances worked out from
//! the tables; the world generator's data gives where blocks generate and how much of each ore there is; the
//! `villager_trade` files give the trades. Ported from the Horizon JEI kit's `collection.rs`, which read the same
//! JSON from the game's files; here a server reload listener hands it over ([`super::server`]).
//!
//! Nothing here panics on bad data: a table or feature that doesn't read is skipped (a panic in a reload listener
//! would stop the server).

use super::math::{average, height_shape, range, solid, tries, vein_blocks};
use mod_api::serde_json::{self, Value, json};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

pub(crate) const SILK: u8 = 1;
pub(crate) const SHEARS: u8 = 2;
pub(crate) const PLAYER: u8 = 4;
pub(crate) const FORTUNE: u8 = 8;
pub(crate) const LOOTING: u8 = 16;
/// Depends on something a page can't show (the mob's variant, the weather, ...).
pub(crate) const MAYBE: u8 = 32;
/// Depends on the block's state (a grown crop): an `alternatives` entry goes on to the next child.
const STATE: u8 = 64;

pub(crate) const DIMENSIONS: [&str; 3] = ["Overworld", "The Nether", "The End"];
/// Each dimension's lowest block and height.
pub(crate) const WORLD: [(i32, i32); 3] = [(-64, 384), (0, 256), (0, 256)];

/// The raw folders a server reload listener read: id (`minecraft:blocks/stone`) to its JSON. Structures are their
/// templates' palettes already (`minecraft:village/plains/houses/plains_small_house_1` to block ids).
#[derive(Default, Clone)]
pub(crate) struct Data {
    pub loot: HashMap<String, Value>,
    pub features: HashMap<String, Value>,
    pub placed: HashMap<String, Value>,
    pub biomes: HashMap<String, Value>,
    pub providers: HashMap<String, Value>,
    pub rules: HashMap<String, Value>,
    pub settings: HashMap<String, Value>,
    pub biome_tags: HashMap<String, Value>,
    pub trades: HashMap<String, Value>,
    /// `tags/villager_trade`: which professions, at which level, offer which trades (`minecraft:armorer/level_2`).
    pub trade_tags: HashMap<String, Value>,
    pub structures: HashMap<String, Vec<String>>,
    /// `worldgen/structure`: each structure's definition (its `spawn_overrides`).
    pub structure_defs: HashMap<String, Value>,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Tool {
    Hand,
    Silk,
    Shears,
}

/// A loot table's kind of source, for its tab.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Group {
    Chest,
    Archaeology,
    Fishing,
    Barter,
    Shearing,
    Other,
}

impl Group {
    const ALL: [Group; 6] = [Group::Chest, Group::Archaeology, Group::Fishing, Group::Barter, Group::Shearing, Group::Other];
    fn code(self) -> u8 {
        self as u8
    }
    fn of(code: u64) -> Option<Group> {
        Self::ALL.get(code as usize).copied()
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Source {
    Block(String),
    Mob(String),
    Table(Group, String),
}

#[derive(Clone, Debug)]
pub(crate) struct Drop {
    pub source: Source,
    pub chance: f64,
    pub low: u32,
    pub high: u32,
    pub flags: u8,
}

/// How much of one ore a dimension's biomes generate.
#[derive(Clone, Debug)]
pub(crate) struct Ore {
    pub dimension: usize,
    /// None: every biome of the dimension.
    pub biomes: Option<Vec<String>>,
    pub per_chunk: f64,
    /// Its part of all blocks from `low` to `high`.
    pub share: f64,
    /// Its percent of each layer's blocks, from the world's bottom up.
    pub layers: Vec<f32>,
    pub low: i32,
    pub high: i32,
    pub peak: i32,
}

#[derive(Default, Clone, Debug)]
pub(crate) struct Natural {
    /// Dimensions whose terrain has it.
    pub terrain: BTreeSet<usize>,
    /// Dimension to the biomes a feature places it in.
    pub biomes: BTreeMap<usize, BTreeSet<String>>,
    pub structures: BTreeSet<String>,
}

/// One villager trade.
#[derive(Clone, Debug)]
pub(crate) struct Trade {
    pub who: String,
    pub level: Option<u32>,
    pub wants: Vec<(String, u32)>,
    pub gives: (String, u32),
}

#[derive(Default, Clone, Debug)]
pub(crate) struct Index {
    pub drops: HashMap<String, Vec<Drop>>,
    pub natural: HashMap<String, Natural>,
    pub ores: HashMap<String, Vec<Ore>>,
    /// Each dimension's number of biomes.
    pub biome_counts: [usize; 3],
    /// A mob's category and the biomes it spawns in, by dimension.
    pub spawns: HashMap<String, (String, [BTreeSet<String>; 3])>,
    pub trades: Vec<Trade>,
    /// A mob to the structures that spawn it (`spawn_overrides`: witch huts, monuments, fortresses...).
    pub structure_spawns: HashMap<String, BTreeSet<String>>,
}

// ---------------------------------------------------------------------------------------------------------
// Loot tables.

#[derive(Clone, Copy)]
struct Test {
    pass: bool,
    chance: f64,
    flags: u8,
}

const YES: Test = Test { pass: true, chance: 1.0, flags: 0 };

/// Whether a loot condition's tool predicate matches the tool a source is read for.
fn tool_matches(v: &Value, tool: Tool) -> bool {
    let text = v.get("predicate").unwrap_or(&Value::Null).to_string();
    match tool {
        Tool::Hand => false,
        Tool::Silk => text.contains("silk_touch"),
        Tool::Shears => text.contains("shears"),
    }
}

/// One loot condition, tested for the source's tool.
fn condition(v: &Value, tool: Tool, depth: u32) -> Test {
    if depth > 16 {
        return Test { flags: MAYBE, ..YES };
    }
    // A named predicate: the tools are all a page needs of them.
    if let Value::String(name) = v {
        return match (name.contains("silk_touch"), name.contains("shear")) {
            (true, _) => Test { pass: tool == Tool::Silk, ..YES },
            (_, true) => Test { pass: tool == Tool::Shears, ..YES },
            _ => Test { flags: MAYBE, ..YES },
        };
    }
    let kind = v.get("type").or_else(|| v.get("condition")).and_then(Value::as_str).unwrap_or("").trim_start_matches("minecraft:");
    let list = |key: &str| v.get(key).and_then(Value::as_array).map_or(&[][..], |a| a.as_slice());
    match kind {
        "random_chance" => Test { chance: v.get("chance").map_or(1.0, average), ..YES },
        "random_chance_with_enchanted_bonus" => {
            Test { chance: v.get("unenchanted_chance").map_or(1.0, average), flags: LOOTING, ..YES }
        }
        "table_bonus" => Test {
            chance: v.get("chances").and_then(Value::as_array).and_then(|c| c.first()).map_or(1.0, average),
            flags: FORTUNE,
            ..YES
        },
        "killed_by_player" => Test { flags: PLAYER, ..YES },
        "match_tool" => Test { pass: tool_matches(v, tool), ..YES },
        "survives_explosion" => YES,
        "block_state_property" | "match_block" => Test { flags: STATE, ..YES },
        "inverted" => {
            let t = v.get("term").map_or(YES, |t| condition(t, tool, depth + 1));
            if t.flags & (MAYBE | STATE) != 0 { t } else { Test { pass: !t.pass, chance: 1.0, flags: t.flags } }
        }
        "any_of" => {
            let mut out = Test { pass: false, chance: 0.0, flags: 0 };
            for t in list("terms").iter().map(|t| condition(t, tool, depth + 1)) {
                if t.pass {
                    out.pass = true;
                    out.chance = out.chance.max(t.chance);
                    out.flags |= t.flags;
                }
            }
            out
        }
        "all_of" => all(list("terms"), tool, depth + 1),
        _ => Test { flags: MAYBE, ..YES },
    }
}

/// All of a list of conditions (an `all_of`, or an entry's conditions).
fn all(list: &[Value], tool: Tool, depth: u32) -> Test {
    let mut out = YES;
    for t in list.iter().map(|t| condition(t, tool, depth)) {
        out.pass &= t.pass;
        out.chance *= t.chance;
        out.flags |= t.flags;
    }
    out
}

/// An entry's or pool's conditions, tested together.
fn conditions(v: &Value, tool: Tool) -> Test {
    let mut out = v.get("conditions").and_then(Value::as_array).map_or(YES, |l| all(l, tool, 0));
    match v.get("condition") {
        Some(Value::Array(l)) => {
            let t = all(l, tool, 0);
            out = Test { pass: out.pass && t.pass, chance: out.chance * t.chance, flags: out.flags | t.flags };
        }
        Some(c) => {
            let t = condition(c, tool, 0);
            out = Test { pass: out.pass && t.pass, chance: out.chance * t.chance, flags: out.flags | t.flags };
        }
        None => {}
    }
    out
}

#[derive(Clone, Debug)]
struct Got {
    item: String,
    chance: f64,
    low: u32,
    high: u32,
    flags: u8,
}

/// What `functions` make of one stack's count.
fn count_of(v: &Value, got: &mut Got) {
    let one = v.get("modifier").filter(|m| m.is_object());
    let lists = [v.get("functions"), v.get("modifier")];
    let list = lists.iter().flatten().filter_map(|l| l.as_array()).flatten().chain(one);
    for f in list {
        match f.get("type").or_else(|| f.get("function")).and_then(Value::as_str).unwrap_or("").trim_start_matches("minecraft:") {
            "set_count" => {
                let (low, high, chance) = f.get("count").map_or((1, 1, 1.0), range);
                if f.get("add").and_then(Value::as_bool) == Some(true) {
                    got.high = got.high.saturating_add(high);
                } else if f.get("conditions").is_some() || f.get("condition").is_some() {
                    // Only in some states (more candles): the count ranges over both.
                    got.low = got.low.min(low);
                    got.high = got.high.max(high);
                } else {
                    got.low = low;
                    got.high = high;
                    got.chance *= chance;
                }
            }
            "enchanted_count_increase" | "looting_enchant" => got.flags |= LOOTING,
            "apply_bonus" => got.flags |= FORTUNE,
            _ => {}
        }
    }
}

/// The stacks an entry gives when it is picked; false when its conditions fail.
fn entry(v: &Value, tables: &HashMap<String, Value>, tool: Tool, depth: u32, out: &mut Vec<Got>) -> bool {
    let test = conditions(v, tool);
    if !test.pass {
        return false;
    }
    let start = out.len();
    let kind = v.get("type").and_then(Value::as_str).unwrap_or("").trim_start_matches("minecraft:");
    let children = v.get("children").and_then(Value::as_array).map_or(&[][..], |c| c.as_slice());
    match kind {
        "item" => {
            if let Some(name) = v.get("name").and_then(Value::as_str) {
                let mut got = Got { item: qualify(name), chance: 1.0, low: 1, high: 1, flags: 0 };
                count_of(v, &mut got);
                out.push(got);
            }
        }
        // A tag's items are expanded on the client, which knows the tags.
        "tag" => {
            if let Some(name) = v.get("name").or_else(|| v.get("items")).and_then(Value::as_str) {
                let mut got = Got { item: format!("#{}", qualify(name.trim_start_matches('#'))), chance: 1.0, low: 1, high: 1, flags: 0 };
                count_of(v, &mut got);
                out.push(got);
            }
        }
        "loot_table" if depth < 6 => {
            let nested = v.get("value").or_else(|| v.get("name"));
            let table = match nested {
                Some(Value::String(id)) => tables.get(&qualify(id)),
                other => other,
            };
            if let Some(table) = table {
                out.extend(table_items(table, tables, tool, depth + 1));
            }
        }
        "group" => {
            for c in children {
                entry(c, tables, tool, depth + 1, out);
            }
        }
        "sequence" => {
            for c in children {
                if !entry(c, tables, tool, depth + 1, out) {
                    break;
                }
            }
        }
        "alternatives" => {
            for c in children {
                let open = conditions(c, tool).flags & (MAYBE | STATE) != 0;
                if entry(c, tables, tool, depth + 1, out) && !open {
                    break;
                }
            }
        }
        _ => {}
    }
    for got in &mut out[start..] {
        got.chance *= test.chance;
        got.flags |= test.flags & !STATE;
    }
    true
}

/// Everything a table can give and its chance per roll of the whole table.
fn table_items(table: &Value, tables: &HashMap<String, Value>, tool: Tool, depth: u32) -> Vec<Got> {
    let mut merged: BTreeMap<String, Got> = BTreeMap::new();
    for pool in table.get("pools").and_then(Value::as_array).into_iter().flatten() {
        let test = conditions(pool, tool);
        if !test.pass {
            continue;
        }
        let rolls = pool.get("rolls").map_or(1.0, average);
        let mut picks: Vec<(f64, Vec<Got>)> = Vec::new();
        let mut total = 0.0;
        for e in pool.get("entries").and_then(Value::as_array).into_iter().flatten() {
            let mut got = Vec::new();
            if entry(e, tables, tool, depth, &mut got) {
                let weight = e.get("weight").and_then(Value::as_f64).unwrap_or(1.0);
                total += weight;
                picks.push((weight, got));
            }
        }
        if total <= 0.0 {
            continue;
        }
        let mut per_roll: BTreeMap<String, Got> = BTreeMap::new();
        for (weight, got) in picks {
            for g in got {
                let p = g.chance * weight / total;
                let e = per_roll.entry(g.item.clone()).or_insert(Got { chance: 0.0, ..g.clone() });
                e.chance += p;
                e.low = e.low.min(g.low);
                e.high = e.high.max(g.high);
                e.flags |= g.flags;
            }
        }
        for (id, mut g) in per_roll {
            // The chance counts the pool's rolls; the count stays one stack's, as a chest shows it (a ruined
            // portal's 4-8 rolls are not 8 golden swords in one slot).
            let chance = (1.0 - (1.0 - g.chance.min(1.0)).powf(rolls)) * test.chance;
            g.flags |= test.flags & !STATE;
            count_of(pool, &mut g);
            let m = merged.entry(id).or_insert(Got { chance: 0.0, low: 0, high: 0, ..g.clone() });
            m.chance = 1.0 - (1.0 - m.chance) * (1.0 - chance);
            m.low = if m.high == 0 { g.low } else { m.low.min(g.low) };
            m.high = m.high.saturating_add(g.high);
            m.flags |= g.flags;
        }
    }
    merged.into_values().collect()
}

fn qualify(id: &str) -> String {
    if id.contains(':') { id.to_owned() } else { format!("minecraft:{id}") }
}

/// Indexes every loot table: what each block, mob, chest and other source drops.
fn loot(tables: &HashMap<String, Value>, index: &mut Index) {
    let mut ids: Vec<&String> = tables.keys().collect();
    ids.sort();
    let mut add = |item: String, drop: Drop| index.drops.entry(item).or_default().push(drop);
    for id in ids {
        let Some(table) = tables.get(id) else { continue };
        let Some((ns, path)) = id.split_once(':') else { continue };
        let (top, rest) = path.split_once('/').unwrap_or((path, ""));
        match top {
            "blocks" => {
                if rest.contains('/') {
                    continue;
                }
                let hand = table_items(table, tables, Tool::Hand, 0);
                let block = format!("{ns}:{rest}");
                for g in &hand {
                    add(g.item.clone(), Drop { source: Source::Block(block.clone()), chance: g.chance, low: g.low, high: g.high, flags: g.flags });
                }
                for (tool, flag) in [(Tool::Silk, SILK), (Tool::Shears, SHEARS)] {
                    for g in table_items(table, tables, tool, 0) {
                        if !hand.iter().any(|h| h.item == g.item) {
                            add(g.item.clone(), Drop { source: Source::Block(block.clone()), chance: g.chance, low: g.low, high: g.high, flags: g.flags | flag });
                        }
                    }
                }
            }
            "entities" => {
                let mob = rest.split('/').next().unwrap_or(rest);
                let source = Source::Mob(format!("{ns}:{mob}"));
                for g in table_items(table, tables, Tool::Hand, 0) {
                    add(g.item.clone(), Drop { source: source.clone(), chance: g.chance, low: g.low, high: g.high, flags: g.flags });
                }
            }
            "equipment" | "shearing_block" => {}
            _ => {
                let group = match top {
                    "chests" | "pots" | "dispensers" | "spawners" => Group::Chest,
                    "archaeology" => Group::Archaeology,
                    "shearing" => Group::Shearing,
                    _ if path.starts_with("gameplay/fishing") => Group::Fishing,
                    _ if path == "gameplay/piglin_bartering" => Group::Barter,
                    _ => Group::Other,
                };
                for g in table_items(table, tables, Tool::Hand, 0) {
                    add(g.item.clone(), Drop { source: Source::Table(group, id.clone()), chance: g.chance, low: g.low, high: g.high, flags: g.flags });
                }
            }
        }
    }
    // Each source once per item: a mob's table and its variants' say the same.
    for list in index.drops.values_mut() {
        let mut kept: Vec<Drop> = Vec::new();
        for d in list.drain(..) {
            match kept.iter().position(|k| k.source == d.source) {
                Some(i) => {
                    if d.chance > kept[i].chance {
                        kept[i] = d;
                    }
                }
                None => kept.push(d),
            }
        }
        *list = kept;
    }
}

// ---------------------------------------------------------------------------------------------------------
// The world generator's data.

/// A block id a generator's JSON names, without what only tests for blocks.
fn walk_blocks(v: &Value, maps: &[&HashMap<String, Value>], seen: &mut HashSet<String>, out: &mut BTreeSet<String>, depth: u32) {
    const TESTS: [&str; 15] = [
        "predicate", "target", "replaceable", "can_place_on", "can_grow_through", "can_replace", "placement", "biome", "tag",
        "if_true", "cannot_replace", "invalid_blocks", "valid_base_block", "replaceable_blocks", "density",
    ];
    if depth > 64 {
        return;
    }
    match v {
        Value::String(s) => {
            if !s.contains(':') || s.starts_with('#') {
                return;
            }
            let mut followed = false;
            for (i, map) in maps.iter().enumerate() {
                if let Some(next) = map.get(s) {
                    followed = true;
                    if seen.insert(format!("{i} {s}")) {
                        walk_blocks(next, maps, seen, out, depth + 1);
                    }
                }
            }
            if !followed {
                out.insert(s.clone());
            }
        }
        Value::Array(a) => a.iter().for_each(|x| walk_blocks(x, maps, seen, out, depth + 1)),
        Value::Object(o) => {
            if o.contains_key("predicate_type") {
                return;
            }
            // A block state names its block in `Name`.
            if let Some(Value::String(name)) = o.get("Name") {
                out.insert(name.clone());
                return;
            }
            for (k, x) in o {
                if k != "type" && !TESTS.contains(&k.as_str()) {
                    walk_blocks(x, maps, seen, out, depth + 1);
                }
            }
        }
        _ => {}
    }
}

/// A biome tag's biomes, nested tags expanded.
fn tag_biomes(tags: &HashMap<String, Value>, id: &str, depth: u32, out: &mut BTreeSet<String>) {
    if depth > 8 {
        return;
    }
    let Some(tag) = tags.get(id) else { return };
    for v in tag.get("values").and_then(Value::as_array).into_iter().flatten() {
        let name = v.as_str().or_else(|| v.get("id").and_then(Value::as_str)).unwrap_or("");
        if let Some(inner) = name.strip_prefix('#') {
            tag_biomes(tags, &qualify(inner), depth + 1, out);
        } else if !name.is_empty() {
            out.insert(qualify(name));
        }
    }
}

/// A block state's name, as a feature's target writes it (`{"Name": ...}` or a bare id).
fn state_name(v: &Value) -> Option<String> {
    v.get("Name").and_then(Value::as_str).or_else(|| v.as_str()).map(qualify)
}

/// Indexes world generation: where each ore and other feature is placed, what the terrain is made of, what
/// structures hold and where mobs spawn.
fn worldgen(data: &Data, index: &mut Index) {
    let mut dims: [BTreeSet<String>; 3] = Default::default();
    for (d, tag) in ["minecraft:is_overworld", "minecraft:is_nether", "minecraft:is_end"].iter().enumerate() {
        tag_biomes(&data.biome_tags, tag, 0, &mut dims[d]);
        dims[d].retain(|b| data.biomes.contains_key(b));
    }
    for (i, d) in dims.iter().enumerate() {
        index.biome_counts[i] = d.len();
    }
    for (d, list) in dims.iter().enumerate() {
        for biome in list {
            let Some(b) = data.biomes.get(biome) else { continue };
            let spawns = b
                .get("attributes")
                .and_then(|a| a.get("minecraft:gameplay/natural_mob_spawns"))
                .and_then(|s| s.get("argument").or(Some(s)))
                .and_then(|a| a.get("spawns_by_category"))
                .or_else(|| b.get("spawners"))
                .and_then(Value::as_object);
            let Some(by_category) = spawns else { continue };
            for (category, list) in by_category {
                for mob in list.as_array().into_iter().flatten().filter_map(|s| s.get("type")?.as_str()) {
                    let e = index.spawns.entry(qualify(mob)).or_insert_with(|| (category.clone(), Default::default()));
                    e.1[d].insert(biome.clone());
                }
            }
        }
    }
    // Structures' own spawns.
    for (id, def) in &data.structure_defs {
        let Some(overrides) = def.get("spawn_overrides").and_then(Value::as_object) else { continue };
        let name = super::math::title_case(id.split_once(':').map_or(id.as_str(), |p| p.1).replace('/', " ").as_str());
        for o in overrides.values() {
            for mob in o.get("spawns").and_then(Value::as_array).into_iter().flatten().filter_map(|s| s.get("type")?.as_str()) {
                index.structure_spawns.entry(qualify(mob)).or_default().insert(name.clone());
            }
        }
    }
    // Ore: (block, dimension, the biomes when not all) to the blocks per chunk at each Y.
    let mut ore_density: BTreeMap<(String, usize, Option<Vec<String>>), Vec<f64>> = BTreeMap::new();
    // Each placed feature's biomes in each dimension.
    let mut feature_biomes: BTreeMap<String, [BTreeSet<String>; 3]> = BTreeMap::new();
    for (d, list) in dims.iter().enumerate() {
        for biome in list {
            let Some(b) = data.biomes.get(biome) else { continue };
            for step in b.get("features").and_then(Value::as_array).into_iter().flatten() {
                for f in step.as_array().into_iter().flatten().filter_map(Value::as_str) {
                    feature_biomes.entry(qualify(f)).or_default()[d].insert(biome.clone());
                }
            }
        }
    }
    let maps: [&HashMap<String, Value>; 3] = [&data.placed, &data.features, &data.providers];
    for (id, per_dim) in &feature_biomes {
        let Some(p) = data.placed.get(id) else { continue };
        let mut blocks = BTreeSet::new();
        let mut seen = HashSet::new();
        walk_blocks(p, &maps, &mut seen, &mut blocks, 0);
        for block in blocks {
            let n = index.natural.entry(block).or_default();
            for (d, list) in per_dim.iter().enumerate() {
                if !list.is_empty() {
                    n.biomes.entry(d).or_default().extend(list.iter().cloned());
                }
            }
        }
        let config = match p.get("feature") {
            Some(Value::String(f)) => data.features.get(&qualify(f)),
            other => other,
        };
        let Some(config) = config else { continue };
        let kind = config.get("type").and_then(Value::as_str).unwrap_or("");
        if kind != "minecraft:ore" && kind != "minecraft:scattered_ore" {
            continue;
        }
        let settings = config.get("config").unwrap_or(config);
        let placement = p.get("placement").and_then(Value::as_array).map_or(&[][..], |a| a.as_slice());
        let size = settings.get("size").and_then(Value::as_f64).unwrap_or(1.0);
        let targets = settings.get("targets").and_then(Value::as_array).map_or(&[][..], |a| a.as_slice());
        let states: Vec<String> = targets.iter().filter_map(|t| t.get("state").and_then(state_name)).collect();
        let layered = states.iter().any(|s| s.contains("deepslate_")) && states.len() > 1;
        for (d, list) in per_dim.iter().enumerate() {
            if list.is_empty() {
                continue;
            }
            let world = WORLD[d];
            let shape = height_shape(placement, world);
            let blocks_per_chunk = tries(placement) * vein_blocks(size);
            let scope = (list.len() != dims[d].len()).then(|| list.iter().cloned().collect::<Vec<_>>());
            for state in &states {
                let deep = state.contains("deepslate_");
                let e = ore_density.entry((state.clone(), d, scope.clone())).or_insert_with(|| vec![0.0; world.1 as usize]);
                for (i, chance) in shape.iter().enumerate().take(e.len()) {
                    let y = world.0 + i as i32;
                    // Stone gives way to deepslate from Y 8 down to Y 0.
                    let stone = (f64::from(y) / 8.0).clamp(0.0, 1.0);
                    let share = if !layered { 1.0 } else if deep { 1.0 - stone } else { stone };
                    e[i] += chance * blocks_per_chunk * share * solid(d, y);
                }
            }
        }
    }
    for ((state, d, scope), density) in ore_density {
        let total: f64 = density.iter().sum();
        if total <= 0.0 {
            continue;
        }
        let world = WORLD[d];
        // The heights holding most of it.
        let Some(peak) = density.iter().enumerate().max_by(|a, b| a.1.total_cmp(b.1)).map(|p| p.0) else { continue };
        let most = density[peak] * 0.1;
        let Some(first) = density.iter().position(|v| *v > most) else { continue };
        let last = density.iter().rposition(|v| *v > most).unwrap_or(first);
        index.ores.entry(state).or_default().push(Ore {
            dimension: d,
            biomes: scope,
            per_chunk: total,
            share: density[first..=last].iter().sum::<f64>() / (256.0 * (last - first + 1) as f64),
            layers: density.iter().map(|v| (*v / 256.0 * 100.0) as f32).collect(),
            low: world.0 + first as i32,
            high: world.0 + last as i32,
            peak: world.0 + peak as i32,
        });
    }
    // Terrain: what each dimension's noise settings and surface rules place.
    let rule_maps: [&HashMap<String, Value>; 1] = [&data.rules];
    for (d, name) in ["minecraft:overworld", "minecraft:nether", "minecraft:end"].into_iter().enumerate() {
        let mut blocks = BTreeSet::new();
        if let Some(s) = data.settings.get(name) {
            for key in ["default_block", "default_fluid"] {
                if let Some(b) = s.get(key).and_then(state_name) {
                    blocks.insert(b);
                }
            }
            if let Some(r) = s.get("surface_rule") {
                let mut seen = HashSet::new();
                walk_blocks(r, &rule_maps, &mut seen, &mut blocks, 0);
            }
        }
        if let Some(r) = data.rules.get(name) {
            let mut seen = HashSet::new();
            walk_blocks(r, &rule_maps, &mut seen, &mut blocks, 0);
        }
        for block in blocks {
            index.natural.entry(block).or_default().terrain.insert(d);
        }
    }
    // Structures: each template's palette, named by its folder.
    for (id, palette) in &data.structures {
        let path = id.split_once(':').map_or(id.as_str(), |p| p.1);
        let folder = path.split('/').next().unwrap_or(path);
        let label = super::math::title_case(folder);
        for block in palette {
            index.natural.entry(block.clone()).or_default().structures.insert(label.clone());
        }
    }
    // Structures the game builds in code, not from templates.
    for (name, blocks) in CODE_STRUCTURES {
        for block in *blocks {
            index.natural.entry(format!("minecraft:{block}")).or_default().structures.insert((*name).to_owned());
        }
    }
    for (block, n) in &mut index.natural {
        if matches!(block.as_str(), "minecraft:air" | "minecraft:cave_air" | "minecraft:void_air" | "minecraft:jigsaw" | "minecraft:structure_void" | "minecraft:structure_block") {
            n.structures.clear();
            n.biomes.clear();
            n.terrain.clear();
        }
    }
    index.natural.retain(|_, n| !n.structures.is_empty() || !n.biomes.is_empty() || !n.terrain.is_empty());
}

const CODE_STRUCTURES: &[(&str, &[&str])] = &[
    ("Mineshaft", &["rail", "oak_planks", "oak_fence", "cobweb", "torch", "dark_oak_planks", "dark_oak_fence"]),
    ("Stronghold", &["stone_bricks", "mossy_stone_bricks", "cracked_stone_bricks", "infested_stone_bricks", "iron_bars", "iron_door", "end_portal_frame", "oak_door", "smooth_stone_slab", "bookshelf", "cobweb", "torch", "ladder", "oak_fence"]),
    ("Ocean Monument", &["prismarine", "prismarine_bricks", "dark_prismarine", "sea_lantern", "gold_block", "wet_sponge", "sponge"]),
    ("Nether Fortress", &["nether_bricks", "nether_brick_fence", "nether_brick_stairs", "soul_sand", "nether_wart"]),
    ("Desert Pyramid", &["sandstone", "chiseled_sandstone", "cut_sandstone", "sandstone_stairs", "sandstone_slab", "orange_terracotta", "blue_terracotta", "tnt", "stone_pressure_plate", "suspicious_sand"]),
    ("Jungle Temple", &["cobblestone", "mossy_cobblestone", "chiseled_stone_bricks", "cobblestone_stairs", "dispenser", "tripwire_hook", "redstone_wire", "sticky_piston", "repeater", "lever", "vine"]),
    ("Swamp Hut", &["spruce_planks", "oak_log", "spruce_stairs", "oak_fence", "crafting_table", "cauldron", "potted_red_mushroom"]),
    ("End Ship", &["purpur_block", "purpur_stairs", "purpur_slab", "end_stone_bricks", "magenta_stained_glass", "brewing_stand", "dragon_head"]),
];

/// The villager trades, from `villager_trade/<folder>/<level>/<name>`, each under the professions that offer it.
///
/// The folder is not always a profession: `smith/2/emerald_bell` is shared by the armorer, the toolsmith and the
/// weaponsmith. Who offers what is `tags/villager_trade/<profession>/level_<n>` (or a wandering trader's
/// `buying`/`common`/`uncommon`), whose nested tags (`#common_smith/level_2`) are groups, not professions. A trade
/// no tag lists keeps its folder's name.
fn trades(data: &Data, index: &mut Index) {
    let stack = |v: &Value| Some((qualify(v.get("id")?.as_str()?), v.get("count").and_then(Value::as_u64).unwrap_or(1).clamp(1, 64) as u32));
    let mut groups = HashSet::new();
    for tag in data.trade_tags.values() {
        for v in tag.get("values").and_then(Value::as_array).into_iter().flatten() {
            let name = v.as_str().or_else(|| v.get("id").and_then(Value::as_str)).unwrap_or("");
            if let Some(inner) = name.strip_prefix('#') {
                groups.insert(qualify(inner));
            }
        }
    }
    let mut owners: HashMap<String, Vec<(String, Option<u32>)>> = HashMap::new();
    let mut tag_ids: Vec<&String> = data.trade_tags.keys().filter(|id| !groups.contains(*id)).collect();
    tag_ids.sort();
    for id in tag_ids {
        let path = id.split_once(':').map_or(id.as_str(), |p| p.1);
        let (who, rest) = path.split_once('/').unwrap_or((path, ""));
        let level = rest.strip_prefix("level_").and_then(|l| l.parse().ok());
        let mut listed = BTreeSet::new();
        tag_biomes(&data.trade_tags, id, 0, &mut listed);
        for trade in listed {
            let list = owners.entry(trade).or_default();
            if !list.iter().any(|(w, l)| w == who && *l == level) {
                list.push((who.to_owned(), level));
            }
        }
    }
    for (id, trade) in &data.trades {
        let Some(gives) = trade.get("gives").and_then(stack) else { continue };
        let mut wants: Vec<(String, u32)> = trade.get("wants").and_then(stack).into_iter().collect();
        wants.extend(trade.get("additional_wants").and_then(stack));
        let who = match owners.get(id) {
            Some(list) => list.clone(),
            None => {
                let path = id.split_once(':').map_or(id.as_str(), |p| p.1);
                let mut parts = path.split('/');
                let folder = parts.next().unwrap_or(path).to_owned();
                vec![(folder, parts.next().and_then(|l| l.trim_start_matches("level_").parse().ok()))]
            }
        };
        for (who, level) in who {
            index.trades.push(Trade { who, level, wants: wants.clone(), gives: gives.clone() });
        }
    }
    index.trades.sort_by(|a, b| a.who.cmp(&b.who).then_with(|| a.level.cmp(&b.level)).then_with(|| a.gives.0.cmp(&b.gives.0)));
}

/// The whole index.
pub(crate) fn build(data: &Data) -> Index {
    let mut index = Index::default();
    loot(&data.loot, &mut index);
    worldgen(data, &mut index);
    trades(data, &mut index);
    index
}

// ---------------------------------------------------------------------------------------------------------
// The wire: the index as compact JSON, sent to each client.

impl Index {
    pub(crate) fn to_json(&self) -> String {
        let source = |s: &Source| match s {
            Source::Block(b) => json!(["b", b]),
            Source::Mob(m) => json!(["m", m]),
            Source::Table(g, id) => json!(["t", g.code(), id]),
        };
        let round = |c: f64| (c * 1e5).round() / 1e5;
        let drops: serde_json::Map<String, Value> = self
            .drops
            .iter()
            .map(|(item, list)| {
                let list: Vec<Value> = list.iter().map(|d| json!([source(&d.source), round(d.chance), d.low, d.high, d.flags])).collect();
                (item.clone(), Value::Array(list))
            })
            .collect();
        let natural: serde_json::Map<String, Value> = self
            .natural
            .iter()
            .map(|(block, n)| {
                let biomes: serde_json::Map<String, Value> = n.biomes.iter().map(|(d, b)| (d.to_string(), json!(b))).collect();
                (block.clone(), json!([n.terrain, biomes, n.structures]))
            })
            .collect();
        let ores: serde_json::Map<String, Value> = self
            .ores
            .iter()
            .map(|(block, list)| {
                let list: Vec<Value> = list
                    .iter()
                    .map(|o| {
                        let layers: Vec<f32> = o.layers.iter().map(|l| (l * 1000.0).round() / 1000.0).collect();
                        json!([o.dimension, o.biomes, round(o.per_chunk), round(o.share), layers, o.low, o.high, o.peak])
                    })
                    .collect();
                (block.clone(), Value::Array(list))
            })
            .collect();
        let spawns: serde_json::Map<String, Value> =
            self.spawns.iter().map(|(mob, (category, by_dim))| (mob.clone(), json!([category, by_dim]))).collect();
        let trades: Vec<Value> = self.trades.iter().map(|t| json!([t.who, t.level, t.wants, t.gives])).collect();
        let structure_spawns: serde_json::Map<String, Value> = self.structure_spawns.iter().map(|(m, s)| (m.clone(), json!(s))).collect();
        json!({ "drops": drops, "natural": natural, "ores": ores, "biomes": self.biome_counts, "spawns": spawns, "trades": trades, "structure_spawns": structure_spawns }).to_string()
    }

    pub(crate) fn from_json(text: &str) -> Option<Index> {
        let v: Value = serde_json::from_str(text).ok()?;
        let s = |v: &Value| v.as_str().map(str::to_owned);
        let n = |v: &Value| v.as_u64().unwrap_or(0) as u32;
        let set = |v: &Value| -> BTreeSet<String> { v.as_array().into_iter().flatten().filter_map(s).collect() };
        let mut index = Index::default();
        for (item, list) in v["drops"].as_object()? {
            let mut out = Vec::new();
            for d in list.as_array().into_iter().flatten() {
                let src = &d[0];
                let source = match src[0].as_str() {
                    Some("b") => Source::Block(s(&src[1])?),
                    Some("m") => Source::Mob(s(&src[1])?),
                    Some("t") => Source::Table(Group::of(src[1].as_u64()?)?, s(&src[2])?),
                    _ => continue,
                };
                out.push(Drop { source, chance: d[1].as_f64().unwrap_or(0.0), low: n(&d[2]), high: n(&d[3]), flags: n(&d[4]) as u8 });
            }
            index.drops.insert(item.clone(), out);
        }
        for (block, x) in v["natural"].as_object()? {
            let terrain = x[0].as_array().into_iter().flatten().filter_map(|d| d.as_u64().map(|d| d as usize)).collect();
            let biomes = x[1].as_object().into_iter().flatten().filter_map(|(d, b)| Some((d.parse().ok()?, set(b)))).collect();
            index.natural.insert(block.clone(), Natural { terrain, biomes, structures: set(&x[2]) });
        }
        for (block, list) in v["ores"].as_object()? {
            let mut out = Vec::new();
            for o in list.as_array().into_iter().flatten() {
                out.push(Ore {
                    dimension: (o[0].as_u64().unwrap_or(0) as usize).min(2),
                    biomes: o[1].as_array().map(|a| a.iter().filter_map(s).collect()),
                    per_chunk: o[2].as_f64().unwrap_or(0.0),
                    share: o[3].as_f64().unwrap_or(0.0),
                    layers: o[4].as_array().into_iter().flatten().map(|l| l.as_f64().unwrap_or(0.0) as f32).collect(),
                    low: o[5].as_i64().unwrap_or(0) as i32,
                    high: o[6].as_i64().unwrap_or(0) as i32,
                    peak: o[7].as_i64().unwrap_or(0) as i32,
                });
            }
            index.ores.insert(block.clone(), out);
        }
        for (i, c) in v["biomes"].as_array().into_iter().flatten().enumerate().take(3) {
            index.biome_counts[i] = c.as_u64().unwrap_or(0) as usize;
        }
        for (mob, x) in v["spawns"].as_object()? {
            let mut by_dim: [BTreeSet<String>; 3] = Default::default();
            for (d, b) in x[1].as_array().into_iter().flatten().enumerate().take(3) {
                by_dim[d] = set(b);
            }
            index.spawns.insert(mob.clone(), (s(&x[0]).unwrap_or_default(), by_dim));
        }
        let pair = |v: &Value| Some((s(&v[0])?, n(&v[1]).max(1)));
        for t in v["trades"].as_array().into_iter().flatten() {
            let Some(gives) = pair(&t[3]) else { continue };
            index.trades.push(Trade {
                who: s(&t[0]).unwrap_or_default(),
                level: t[1].as_u64().map(|l| l as u32),
                wants: t[2].as_array().into_iter().flatten().filter_map(pair).collect(),
                gives,
            });
        }
        for (mob, list) in v["structure_spawns"].as_object().into_iter().flatten() {
            index.structure_spawns.insert(mob.clone(), set(list));
        }
        Some(index)
    }
}

#[cfg(test)]
mod trade_tests {
    use super::*;

    /// 26.3 keeps trades several professions share in a folder of their own (`smith/2/emerald_bell`); the tags say
    /// who offers them. A trade no tag lists keeps its folder's name.
    #[test]
    fn shared_trades_go_to_the_professions_their_tags_name() {
        let mut data = Data::default();
        let trade = |gives: &str| json!({ "gives": { "id": gives }, "wants": { "id": "minecraft:emerald", "count": 36 } });
        data.trades.insert("minecraft:smith/2/emerald_bell".into(), trade("minecraft:bell"));
        data.trades.insert("minecraft:armorer/2/emerald_chainmail_boots".into(), trade("minecraft:chainmail_boots"));
        data.trades.insert("minecraft:wandering_trader/emerald_acacia_log".into(), trade("minecraft:acacia_log"));
        data.trades.insert("minecraft:unlisted/1/emerald_stick".into(), trade("minecraft:stick"));
        data.trade_tags.insert("minecraft:common_smith/level_2".into(), json!({ "values": ["minecraft:smith/2/emerald_bell"] }));
        for who in ["armorer", "toolsmith", "weaponsmith"] {
            let mut values = vec![json!("#minecraft:common_smith/level_2")];
            if who == "armorer" {
                values.push(json!("minecraft:armorer/2/emerald_chainmail_boots"));
            }
            data.trade_tags.insert(format!("minecraft:{who}/level_2"), json!({ "values": values }));
        }
        data.trade_tags.insert(
            "minecraft:wandering_trader/common".into(),
            json!({ "values": [{ "id": "minecraft:wandering_trader/emerald_acacia_log", "required": false }] }),
        );
        let mut index = Index::default();
        trades(&data, &mut index);
        let who = |item: &str| {
            index.trades.iter().filter(|t| t.gives.0 == item).map(|t| (t.who.as_str(), t.level)).collect::<Vec<_>>()
        };
        assert_eq!(who("minecraft:bell"), [("armorer", Some(2)), ("toolsmith", Some(2)), ("weaponsmith", Some(2))]);
        assert_eq!(who("minecraft:chainmail_boots"), [("armorer", Some(2))]);
        assert_eq!(who("minecraft:acacia_log"), [("wandering_trader", None)]);
        assert_eq!(who("minecraft:stick"), [("unlisted", Some(1))]);
        // No group is ever a villager.
        assert!(index.trades.iter().all(|t| t.who != "common_smith" && t.who != "smith"));
    }
}
