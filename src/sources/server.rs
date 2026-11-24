//! The server half of the sources: reload listeners keep the folders the index is made from (with the world's data
//! packs and every mod's `data/` over vanilla's), a thread builds the index, and `jei:sources` sends it to a client
//! that asks. One mod instance serves both sides, so single player goes through the same channel.

use super::index::{self, Data};
use mod_api::serde_json::{self, Value};
use mod_api::*;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

pub(crate) const CHANNEL: &str = "jei:sources";
/// A part's most bytes: well under a message's 1 MiB, so a burst of parts doesn't fill the connection's budget.
const PART: usize = 200_000;

/// The folders, as the last reload left them.
static DATA: Mutex<Option<Data>> = Mutex::new(None);
/// Bumped by every reload: the index is made again for a new generation.
static GENERATION: Mutex<u64> = Mutex::new(0);
/// The index of a generation, ready to send.
static BUILT: Mutex<Option<(u64, Arc<Vec<Vec<u8>>>)>> = Mutex::new(None);
/// A generation being built.
static BUILDING: Mutex<Option<u64>> = Mutex::new(None);

/// Which folder a listener keeps.
#[derive(Clone, Copy)]
enum Slot {
    Loot,
    Features,
    Placed,
    Biomes,
    Providers,
    Rules,
    Settings,
    BiomeTags,
    Trades,
    TradeTags,
    Structures,
    StructureDefs,
}

const FOLDERS: [(&str, &str, Slot); 12] = [
    ("jei:sources_loot", "loot_table", Slot::Loot),
    ("jei:sources_features", "worldgen/feature", Slot::Features),
    ("jei:sources_placed", "worldgen/placed_feature", Slot::Placed),
    ("jei:sources_biomes", "worldgen/biome", Slot::Biomes),
    ("jei:sources_providers", "worldgen/block_state_provider", Slot::Providers),
    ("jei:sources_rules", "worldgen/material_rule", Slot::Rules),
    ("jei:sources_settings", "worldgen/noise_settings", Slot::Settings),
    ("jei:sources_biome_tags", "tags/worldgen/biome", Slot::BiomeTags),
    ("jei:sources_trades", "villager_trade", Slot::Trades),
    ("jei:sources_trade_tags", "tags/villager_trade", Slot::TradeTags),
    ("jei:sources_structures", "structure", Slot::Structures),
    ("jei:sources_structure_defs", "worldgen/structure", Slot::StructureDefs),
];

struct Folder(Slot);

enum Read {
    Json(HashMap<String, Value>),
    Palettes(HashMap<String, Vec<String>>),
}

impl ServerReloadListener for Folder {
    fn prepare(&self, context: &ServerReloadContext) -> Result<Box<dyn FnOnce() + Send + 'static>, ReloadResourceError> {
        let slot = self.0;
        let read = match slot {
            Slot::Structures => Read::Palettes(
                context.list().filter_map(|id| Some((id.to_string(), super::nbt::palette(context.read(id)?)))).collect(),
            ),
            _ => Read::Json(
                context
                    .list()
                    .filter_map(|id| Some((id.to_string(), serde_json::from_slice::<Value>(context.read(id)?).ok()?)))
                    .collect(),
            ),
        };
        Ok(Box::new(move || {
            let mut data = DATA.lock().unwrap_or_else(|e| e.into_inner());
            let d = data.get_or_insert_with(Data::default);
            match (slot, read) {
                (Slot::Structures, Read::Palettes(p)) => d.structures = p,
                (_, Read::Json(m)) => *match slot {
                    Slot::Loot => &mut d.loot,
                    Slot::Features => &mut d.features,
                    Slot::Placed => &mut d.placed,
                    Slot::Biomes => &mut d.biomes,
                    Slot::Providers => &mut d.providers,
                    Slot::Rules => &mut d.rules,
                    Slot::Settings => &mut d.settings,
                    Slot::BiomeTags => &mut d.biome_tags,
                    Slot::StructureDefs => &mut d.structure_defs,
                    Slot::TradeTags => &mut d.trade_tags,
                    Slot::Trades | Slot::Structures => &mut d.trades,
                } = m,
                _ => {}
            }
            *GENERATION.lock().unwrap_or_else(|e| e.into_inner()) += 1;
        }))
    }
}

/// Registers the listeners (from `add_server_reload_listeners`).
pub(crate) fn listen(event: &mut AddServerReloadListenersEvent<'_>) {
    for (id, folder, slot) in FOLDERS {
        let conv = match slot {
            Slot::Structures => FileToIdConverter::new(folder, ".nbt"),
            _ => FileToIdConverter::json(folder),
        };
        let (Ok(id), Ok(conv)) = (Identifier::parse(id), conv) else { continue };
        if let Err(e) = event.listeners.add_listener(id, conv, Arc::new(Folder(slot))) {
            eprintln!("[jei] {folder} not read: {e:?}");
        }
    }
}

/// The index of the current generation in parts, or None while it is being made (a thread starts if none runs).
fn parts() -> Option<Arc<Vec<Vec<u8>>>> {
    let generation = *GENERATION.lock().unwrap_or_else(|e| e.into_inner());
    if let Some((g, parts)) = BUILT.lock().unwrap_or_else(|e| e.into_inner()).as_ref()
        && *g == generation
    {
        return Some(parts.clone());
    }
    let mut building = BUILDING.lock().unwrap_or_else(|e| e.into_inner());
    if *building == Some(generation) {
        return None;
    }
    let Some(data) = DATA.lock().unwrap_or_else(|e| e.into_inner()).clone() else { return None };
    *building = Some(generation);
    let started = std::thread::Builder::new().name("jei-sources".into()).spawn(move || {
        let text = index::build(&data).to_json();
        let bytes = text.into_bytes();
        let chunks: Vec<&[u8]> = bytes.chunks(PART).collect();
        let total = chunks.len();
        let parts: Vec<Vec<u8>> = chunks
            .into_iter()
            .enumerate()
            .map(|(i, c)| {
                let mut p = format!("{generation} {i} {total}\n").into_bytes();
                p.extend_from_slice(c);
                p
            })
            .collect();
        *BUILT.lock().unwrap_or_else(|e| e.into_inner()) = Some((generation, Arc::new(parts)));
        *BUILDING.lock().unwrap_or_else(|e| e.into_inner()) = None;
    });
    if started.is_err() {
        *building = None;
    }
    None
}

/// Sends the index to `player`, if it is made; the client asks again until it has it.
pub(crate) fn send(host: &mut dyn ServerHost, player: u128) {
    let Some(parts) = parts() else { return };
    for part in parts.iter() {
        let Some(to) = host.player(player) else { return };
        if to.send_payload(host, &PayloadId(CHANNEL.into()), PayloadBytes(part.clone())).is_err() {
            return;
        }
    }
}

/// Starts making the index as soon as a world is up, so the first client's ask finds it ready.
pub(crate) fn warm() {
    let _ = parts();
}

/// `jei:sources` on the server: any message from a client asks for the index.
pub(crate) struct FromClient;

impl IPayloadHandler for FromClient {
    fn handle(&self, _payload: PayloadBytes, cx: &mut dyn IPayloadContext) -> Result<(), PayloadHandleError> {
        let player = cx.player().map_err(|e| PayloadHandleError::Failed(format!("{e:?}")))?.uuid();
        let host = cx.server_host().map_err(|e| PayloadHandleError::Failed(format!("{e:?}")))?;
        send(host, player);
        Ok(())
    }
}
