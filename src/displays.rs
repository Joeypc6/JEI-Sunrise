//! Recipe-book display ids for "+". The fork's transfer (`RecipesClientHost::transfer_recipe(book_id)`) takes the id
//! vanilla gives each recipe display (`RecipeManager.unpackRecipeInfo`: every recipe in registration order, path
//! then namespace, each with its displays), which the API does not hand out by recipe. The server works them out
//! from its own recipe table: a server reload listener learns the recipe types in force from the `recipe/` files,
//! and at login the numbering of every recipe of those types (`Host::recipes_of_type`, data packs and load
//! conditions held) goes to the player on `jei:displays`. One mod instance serves both sides, so single player
//! fills the same table.

use mod_api::serde_json::{self, Value};
use mod_api::*;
use std::collections::{BTreeSet, HashMap};
use std::sync::{Arc, Mutex};

pub(crate) const CHANNEL: &str = "jei:displays";

/// The recipe types the server's `recipe/` files name.
static TYPES: Mutex<BTreeSet<String>> = Mutex::new(BTreeSet::new());
/// Client: display id by recipe id.
pub(crate) static DISPLAYS: Mutex<Option<HashMap<String, i32>>> = Mutex::new(None);

pub(crate) struct Types;

impl ServerReloadListener for Types {
    fn prepare(&self, context: &ServerReloadContext) -> Result<Box<dyn FnOnce() + Send + 'static>, ReloadResourceError> {
        let mut types = BTreeSet::new();
        for id in context.list() {
            if let Some(kind) = context
                .read(id)
                .and_then(|b| serde_json::from_slice::<Value>(b).ok())
                .and_then(|v| v["type"].as_str().map(str::to_owned))
            {
                types.insert(if kind.contains(':') { kind } else { format!("minecraft:{kind}") });
            }
        }
        Ok(Box::new(move || *TYPES.lock().unwrap_or_else(|e| e.into_inner()) = types))
    }
}

pub(crate) fn listener() -> Arc<Types> {
    Arc::new(Types)
}

/// The recipe type a vanilla serializer's recipes are listed under; any other type is its own.
fn recipe_type(serializer: &str) -> &str {
    if serializer.starts_with("minecraft:crafting_") {
        "minecraft:crafting"
    } else if serializer.starts_with("minecraft:smithing_") {
        "minecraft:smithing"
    } else {
        serializer
    }
}

/// How many book displays a recipe has (`Recipe.display()`), as the fork's `RecipeIndex` counts them.
fn displays(kind: &str, json: &Value) -> i32 {
    if kind.starts_with("minecraft:crafting_special_") || kind == "minecraft:crafting_decorated_pot" || kind == "minecraft:brewing" {
        return 0;
    }
    if kind == "minecraft:crafting_transmute"
        && let Some(o) = json["material_count"].as_object()
    {
        let min = o.get("min").and_then(Value::as_u64).unwrap_or(1);
        let max = o.get("max").and_then(Value::as_u64).unwrap_or(8);
        return (max.saturating_sub(min) + 1) as i32;
    }
    1
}

/// The numbering of the crafting recipes, as text lines `<recipe id> <first display id>` (every recipe takes its
/// place in the numbering; only crafting ones go into a grid, so only they are listed).
pub(crate) fn table(host: &dyn Host) -> String {
    let types: Vec<String> = TYPES.lock().unwrap_or_else(|e| e.into_inner()).iter().cloned().collect();
    // (path.json, namespace, displays, crafting)
    let mut all: Vec<(String, String, i32, bool)> = Vec::new();
    // The files name serializers; Sunrise lists recipes by their recipe type (`crafting_shaped` is under
    // `minecraft:crafting`), so each type is read once and every recipe tells its serializer by its `type`.
    let mut lookups: Vec<&str> = types.iter().map(|k| recipe_type(k)).collect();
    lookups.sort();
    lookups.dedup();
    for lookup in lookups {
        for r in host.recipes_of_type(lookup).iter() {
            let kind = crate::vanilla::serializer(r);
            let crafting = kind.starts_with("minecraft:crafting");
            let (ns, path) = r.id.split_once(':').unwrap_or(("minecraft", &r.id));
            all.push((format!("{path}.json"), ns.to_owned(), displays(kind, &r.json), crafting));
        }
    }
    all.sort();
    all.dedup_by(|a, b| a.0 == b.0 && a.1 == b.1);
    let mut next = 0;
    let mut out = String::new();
    for (path, ns, n, crafting) in all {
        if n > 0 && crafting {
            out.push_str(&format!("{ns}:{} {next}\n", path.trim_end_matches(".json")));
        }
        next += n;
    }
    out
}

/// What the last send or request did, for `/jei debug`.
pub(crate) static LAST: Mutex<String> = Mutex::new(String::new());

pub(crate) fn note(text: String) {
    *LAST.lock().unwrap_or_else(|e| e.into_inner()) = text;
}

pub(crate) fn types() -> usize {
    TYPES.lock().unwrap_or_else(|e| e.into_inner()).len()
}

/// Every recipe type the server's `recipe/` files name, as Sunrise lists recipes (`crafting_shaped` is
/// `minecraft:crafting`), for `datapack_sync`: a client's own recipe table holds only vanilla's and the content mods'
/// recipes, not a native mod's `data/` ones (AE2's crafting, inscriber and charger recipes), so the server sends the
/// player its recipes of these types and JEI's cards show them. // jeiA
pub(crate) fn sync_types() -> Vec<String> {
    let types = TYPES.lock().unwrap_or_else(|e| e.into_inner());
    // Not vanilla's: the client has those already, and on the Nightly of 2026-11-20 any type asked for here made a
    // saved world's rejoin drop with "End of stream" (minecraft:smelting alone did).
    let mut out: Vec<String> =
        types.iter().map(|k| recipe_type(k).to_owned()).filter(|t| !t.starts_with("minecraft:")).collect();
    out.sort();
    out.dedup();
    out
}

pub(crate) fn send(host: &mut dyn ServerHost, player: u128) {
    let table = table(host);
    // A mod message holds at most 30000 bytes: the table goes in parts of whole lines, the first marked "!".
    let mut parts: Vec<String> = vec!["!\n".to_owned()];
    for line in table.lines() {
        if parts.last().is_some_and(|p| p.len() + line.len() + 1 > 28_000) {
            parts.push(String::new());
        }
        if let Some(last) = parts.last_mut() {
            last.push_str(line);
            last.push('\n');
        }
    }
    for part in &parts {
        let Some(to) = host.player(player) else { return };
        if let Err(e) = to.send_payload(host, &PayloadId(CHANNEL.into()), PayloadBytes(part.clone().into_bytes())) {
            note(format!("send failed: {e:?}"));
            return;
        }
    }
    note(format!("sent {} lines in {} parts", table.lines().count(), parts.len()));
}

/// Client: asks the server for the table (at login it may have been sent before the client listened).
pub(crate) fn request(host: &mut dyn ClientHost) {
    if let Err(e) = host.send_payload_to_server(&PayloadId(CHANNEL.into()), PayloadBytes(b"?".to_vec())) {
        note(format!("request failed: {e:?}"));
    }
}

/// `jei:displays` on the server: any message from a client asks for the table.
pub(crate) struct FromClient;

impl IPayloadHandler for FromClient {
    fn handle(&self, _payload: PayloadBytes, cx: &mut dyn IPayloadContext) -> Result<(), PayloadHandleError> {
        let player = cx.player().map_err(|e| PayloadHandleError::Failed(format!("{e:?}")))?.uuid();
        let host = cx.server_host().map_err(|e| PayloadHandleError::Failed(format!("{e:?}")))?;
        send(host, player);
        Ok(())
    }
}

/// `jei:displays` on the client: a part of the server's table.
pub(crate) struct FromServer;

impl IPayloadHandler for FromServer {
    fn handle(&self, payload: PayloadBytes, _cx: &mut dyn IPayloadContext) -> Result<(), PayloadHandleError> {
        receive(&payload.0);
        Ok(())
    }
}

pub(crate) fn receive(data: &[u8]) {
    let text = String::from_utf8_lossy(data);
    let mut g = DISPLAYS.lock().unwrap_or_else(|e| e.into_inner());
    let (first, body) = match text.strip_prefix("!\n") {
        Some(rest) => (true, rest),
        None => (false, &*text),
    };
    if first || g.is_none() {
        *g = Some(HashMap::new());
    }
    if let Some(map) = g.as_mut() {
        for l in body.lines() {
            if let Some((id, n)) = l.rsplit_once(' ')
                && let Ok(n) = n.parse()
            {
                map.insert(id.to_owned(), n);
            }
        }
    }
}

pub(crate) fn of(recipe: &str) -> Option<i32> {
    DISPLAYS.lock().unwrap_or_else(|e| e.into_inner()).as_ref()?.get(recipe).copied()
}

// jeiA
#[cfg(test)]
mod sync_tests {
    use super::*;

    /// The files' serializers become the recipe types a client asks for: AE2's own and vanilla's crafting.
    #[test]
    fn only_mod_recipe_types_sync() {
        *TYPES.lock().unwrap() = ["minecraft:crafting_shaped", "minecraft:crafting_shapeless", "minecraft:smithing_trim", "minecraft:smelting", "ae2:inscriber", "ae2:charger"]
            .iter()
            .map(|s| (*s).to_owned())
            .collect();
        assert_eq!(sync_types(), vec!["ae2:charger", "ae2:inscriber"]);
    }
}
