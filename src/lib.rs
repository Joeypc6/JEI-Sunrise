//! Just Enough Items (mezz, MIT) on the arismoko/horizon fork's mod API: Hooks only (no mixins, no `[[inject]]`, no
//! game internals), so it builds against the fork's prebuilt SDK.
//!
//! What the fork gives and this uses:
//! - the item list and its search box are the game's own panel beside container screens (`panel_items`), and every
//!   click, search and key over an item comes back as `panel_event`, screen slots included (R, U, A);
//! - "+" is the fork's recipe-viewer transfer (`transfer_recipe`), with its server-side dry run
//!   (`transfer_recipe_check`) marking what is missing;
//! - recipes come from `recipes_of_type`, items' names and tooltips from `item_resource_tooltip`, tags from `tags()`;
//! - R, U and A are key mappings on Options > Controls (`register_key_mapping`), read with `mod_key`;
//! - other mods add categories with files under `assets/<ns>/jei/` (a client reload listener reads them), so they
//!   need no code of ours.
//!
//! What it draws itself, because the fork's own recipe page (`native_panel_recipes`) is not in the mod API: the recipe
//! window, over the open screen (`screen_render_post` and the `screen_*_pre` input hooks), and the bookmark bar.

pub mod api;
mod sources;
mod bookmarks;
mod displays;
mod provider;
mod search;
mod state;
mod subtypes;
mod vanilla;
mod view;
mod window;

use mod_api::*;
use state::{Action, with};
use view::Focus;

pub(crate) const ID: &str = "jei";
/// Whether `datapack_sync` asks the server for its recipe types (see that hook).
const SYNC_RECIPE_TYPES: bool = false;

/// A translation, or the fallback when no language has the key.
/// A screen's background dim (`Screen.renderTransparentBackground`: 16, 16, 16 at 160 of 255).
const DIM: u32 = 0xA010_1010;

pub(crate) fn tr(host: &dyn ClientHost, key: &str, fallback: &str) -> String {
    host.translate(key).unwrap_or_else(|| fallback.to_owned())
}

#[derive(Default)]
struct Mod;

/// The bound key of one of our mappings (`key.keyboard.r` by default for `show_recipe`).
fn bound(host: &dyn ClientHost, name: &str, default: &str) -> String {
    host.mod_key(ID, name).map(|(k, _)| k).unwrap_or_else(|| default.to_owned())
}

#[derive(Clone, Copy, PartialEq)]
pub(crate) enum KeyAct {
    Recipes,
    Uses,
    Bookmark,
}

pub(crate) fn key_act(host: &dyn ClientHost, key: &str) -> Option<KeyAct> {
    if key == bound(host, "show_recipe", "key.keyboard.r") {
        Some(KeyAct::Recipes)
    } else if key == bound(host, "show_uses", "key.keyboard.u") {
        Some(KeyAct::Uses)
    } else if key == bound(host, "bookmark", "key.keyboard.a") {
        Some(KeyAct::Bookmark)
    } else {
        None
    }
}

/// GuideME's open key over an item: its page, when GuideME is installed and has one.
fn guide(host: &dyn ClientHost, key: &str, item: ItemKey) -> bool {
    let bound = host.mod_key("guideme", "open").map(|(k, _)| k).unwrap_or_else(|| "key.keyboard.w".to_owned());
    if key != bound {
        return false;
    }
    let id = item.id(host);
    let mut opened = false;
    mod_api::if_installed!(host, guideme, {
        opened = guideme::api::open_item(&id);
    });
    let _ = &id;
    opened
}

/// The focus of the creative screen's slot under the pointer: its stack's subtype when the item has one registered,
/// else its item.
fn creative_focus(host: &dyn ClientHost, j: &state::Jei, at: [f32; 2]) -> Option<Focus> {
    if let Some(menu) = host.local_player().and_then(|p| p.container_menu(host))
        && let Some(boxes) = menu.slot_boxes(host)
        && let Some(index) = boxes.iter().position(|b| b.is_some_and(|r| state::inside(r, at)))
        && let Some(Some(stack)) = menu.slot(host, index)
    {
        let item = stack.item();
        if let Some(component) = j.subtypes.get(&item)
            && let Some(Tag::String(value)) = stack.components().and_then(|c| c.as_nbt().get(&component.as_str().into()).cloned())
        {
            let value = mod_api::serde_json::Value::String(value.to_string_lossy()).to_string();
            return Some(Focus::Sub(item, component.clone(), value));
        }
        return Some(Focus::Item(item));
    }
    // The creative screen gives the hovered item without its components. Its inventory tab shows the player's own
    // stacks: when every stack of that item there has the same subtype, that is the one under the pointer.
    let item = host.screen_hovered_item()?;
    if let Some(component) = j.subtypes.get(&item) {
        let values: std::collections::BTreeSet<String> = host
            .local_player()
            .and_then(|p| p.inventory(host))
            .unwrap_or_default()
            .iter()
            .filter(|s| s.item() == item)
            .filter_map(|s| match s.components().and_then(|c| c.as_nbt().get(&component.as_str().into()).cloned()) {
                Some(Tag::String(v)) => Some(mod_api::serde_json::Value::String(v.to_string_lossy()).to_string()),
                _ => None,
            })
            .collect();
        if values.len() == 1 {
            return Some(Focus::Sub(item, component.clone(), values.into_iter().next()?));
        }
    }
    Some(Focus::Item(item))
}

fn act_on(jei: &mut state::Jei, act: KeyAct, focus: Focus) {
    match (act, focus) {
        (KeyAct::Recipes, f) => {
            jei.open(f, false);
        }
        (KeyAct::Uses, f) => {
            jei.open(f, true);
        }
        (KeyAct::Bookmark, Focus::Item(item) | Focus::Sub(item, _, _)) => jei.toggle_bookmark(item),
        _ => {}
    }
}

/// What the panel leaves out before the creative tabs load: the Operator Utilities tab's items and the items no tab
/// shows.
const TECHNICAL: [&str; 14] = [
    "minecraft:command_block",
    "minecraft:chain_command_block",
    "minecraft:repeating_command_block",
    "minecraft:command_block_minecart",
    "minecraft:jigsaw",
    "minecraft:structure_block",
    "minecraft:structure_void",
    "minecraft:barrier",
    "minecraft:light",
    "minecraft:debug_stick",
    "minecraft:test_block",
    "minecraft:test_instance_block",
    "minecraft:knowledge_book",
    "minecraft:petrified_oak_slab",
];

/// Whether a card has something to show, as JEI lists no recipe it can't draw: some slot or fluid filled, its inputs
/// (when it has any) not all empty, nor its outputs, and a mob's card with at least one drop. A recipe whose items
/// no client knows (a special recipe with no ingredients, a result from a missing mod) is left out.
fn shows_something(card: &view::Card) -> bool {
    use view::Role;
    let has = |role: Role| card.slots.iter().any(|s| s.role == role);
    let filled = |role: Role| card.slots.iter().any(|s| s.role == role && !s.stacks.is_empty());
    if card.wells.iter().any(|w| !w.fluid.is_empty() && w.amount > 0) {
        return true;
    }
    if !card.slots.iter().any(|s| !s.stacks.is_empty()) {
        return false;
    }
    let input_ok = !has(Role::Input) || filled(Role::Input);
    let output_ok = !has(Role::Output) || filled(Role::Output);
    input_ok && output_ok && (card.entity.is_none() || filled(Role::Output))
}

/// Makes the index again when the providers' files or the server's recipes changed.
fn ensure_index(host: &mut dyn ClientHost) {
    let version = provider::VERSION.load(std::sync::atomic::Ordering::SeqCst)
        + (api::with(|r| r.version) << 32)
        + (sources::client::VERSION.load(std::sync::atomic::Ordering::SeqCst) << 48);
    // The recipes can reach the client after the first frames of a world, and `recipes_received` does not always
    // follow: the index is also rebuilt when the number of recipes the client lists changes.
    // jeiA: the providers' recipe types count too (AE2's `ae2:inscriber`): a mod's recipes that reach the client
    // after the first index (the server sending the type) used to leave its cards out until the next reload.
    let mut kinds: Vec<String> =
        vanilla::CATEGORIES.iter().flat_map(|(_, _, _, kinds)| kinds.iter()).map(|k| (*k).to_owned()).collect();
    for k in provider::recipe_types() {
        if !kinds.contains(&k) {
            kinds.push(k);
        }
    }
    let listed: u64 = kinds.iter().map(|k| host.recipes_of_type(k).len() as u64).sum();
    let received = with(|j| j.received) ^ (listed << 20);
    let (built, from_tabs) = with(|j| (j.built, j.from_tabs));
    let up_to_date = built == Some((version, received));
    if up_to_date && from_tabs {
        return;
    }
    let catalog = host.creative_tabs(CreativeTabPermissions::Normal).ok();
    let tabs = catalog.as_deref().map_or(&[][..], |catalog| catalog.tabs.as_slice());
    if up_to_date && !tabs.iter().any(|t| t.category) {
        return;
    }
    let keys = host.item_keys();
    let ids: std::collections::HashMap<String, ItemKey> = keys.iter().map(|k| (k.id(host), *k)).collect();
    // The panel lists what the creative inventory does, as JEI's own list is the search tab's items: an item no tab
    // shows (the knowledge book, the petrified oak slab) is left out, and so is the Operator Utilities tab (command
    // and structure blocks, the debug stick, barrier and light) whatever the player's permissions. Before the tabs
    // load, every item but those.
    let shown: std::collections::HashSet<ItemKey> = tabs
        .iter()
        .filter(|t| t.category && t.id != "minecraft:op_blocks")
        .flat_map(|t| t.display_items.iter().map(|s| s.item()))
        .collect();
    let items: Vec<ItemKey> = keys
        .into_iter()
        .filter(|k| {
            let id = k.id(host);
            id != "minecraft:air" && if shown.is_empty() { !TECHNICAL.contains(&id.as_str()) } else { shown.contains(k) }
        })
        .collect();
    let fuels: Vec<ItemKey> = items.iter().copied().filter(|k| host.item_burn_time(*k).unwrap_or(0) > 0).collect();
    // The panel's entries: potions and enchanted books as one stack per potion and per enchantment level.
    let entries = subtypes::entries(host, &items, tabs);
    // Other mods' extra stacks (`api::register_extra_stacks`: AE2's facades), after their anchor items.
    let mut extra_names: std::collections::HashMap<(ItemKey, String), String> = std::collections::HashMap::new();
    let groups: Vec<(Option<ItemKey>, Vec<(ItemKey, String)>)> = api::extras(host)
        .into_iter()
        .map(|(after, list)| {
            let stacks = list
                .into_iter()
                .filter_map(|e| {
                    let k = *ids.get(&e.item)?;
                    if !e.name.is_empty() {
                        extra_names.insert((k, e.components.clone()), e.name.to_lowercase());
                    }
                    Some((k, e.components))
                })
                .collect();
            (ids.get(&after).copied(), stacks)
        })
        .collect();
    let extra_counts = (groups.len(), groups.iter().map(|(_, s)| s.len()).sum::<usize>());
    let entries = api::place_extras(entries, groups);
    let cx = vanilla::Ctx { host, ids: &ids, fuels: &fuels };
    let mut categories = vanilla::categories(&cx);
    let mut cards = vanilla::cards(&cx);
    provider::add(&cx, &mut categories, &mut cards);
    provider::add_api(&cx, &mut categories, &mut cards);
    sources::client::add(&cx, &mut categories, &mut cards);
    cards.retain(shows_something);
    // A category nothing fills is left out of the tabs (it has no cards to show).
    let subtypes: std::collections::HashMap<ItemKey, String> =
        api::with(|r| r.subtypes.clone()).into_iter().filter_map(|(i, c)| ids.get(&i).map(|k| (*k, c))).collect();
    with(|j| {
        j.subtypes = subtypes;
        j.ids = ids;
        if j.items != entries || j.extra_names != extra_names {
            j.names.clear();
        }
        j.items = entries;
        j.extra_names = extra_names;
        j.extra_counts = extra_counts;
        let old = std::mem::replace(&mut j.categories, categories);
        j.cards = cards;
        j.built = Some((version, received));
        j.from_tabs = !shown.is_empty();
        // jeiA: the open window stays, found again in the new index (it used to close).
        j.reopen_after_rebuild(&old);
        j.actions.push(Action::Panel);
    });
}

#[mod_api::sunrise_mod]
impl Hooks for Mod {
    fn register(&self, r: &mut dyn Registrar, _cx: &RegistrationContext) {
        for (id, key, label) in [
            ("show_recipe", "key.keyboard.r", "key.jei.showRecipe"),
            ("show_uses", "key.keyboard.u", "key.jei.showUses"),
            ("bookmark", "key.keyboard.a", "key.jei.bookmark"),
        ] {
            r.register_key_mapping(
                KeyRegistration::new(id, key).label(label).context(KeyConflictContext::Gui),
            );
        }
        // jei:displays both ways: a client's "?" asks for the table, the server answers in parts.
        r.payload_handlers().registrar("1").play_bidirectional(
            PayloadId(displays::CHANNEL.into()),
            std::sync::Arc::new(displays::FromClient),
            Some(std::sync::Arc::new(displays::FromServer)),
        );
        // jei:sources both ways: a client's "?" asks for the sources' index, the server answers in parts.
        r.payload_handlers().registrar("1").play_bidirectional(
            PayloadId(sources::server::CHANNEL.into()),
            std::sync::Arc::new(sources::server::FromClient),
            Some(std::sync::Arc::new(sources::client::FromServer)),
        );
    }

    fn add_client_reload_listeners(&self, _host: &dyn ClientHost, event: &mut AddClientReloadListenersEvent<'_>) {
        let (Ok(id), Ok(conv)) = (Identifier::parse("jei:providers"), FileToIdConverter::json("jei")) else { return };
        if let Err(e) = event.listeners.add_listener(id, conv, provider::listener()) {
            eprintln!("[jei] provider files not read: {e:?}");
        }
    }

    fn add_server_reload_listeners(&self, _host: &dyn Host, event: &mut AddServerReloadListenersEvent<'_>) {
        let (Ok(id), Ok(conv)) = (Identifier::parse("jei:recipe_types"), FileToIdConverter::json("recipe")) else { return };
        if let Err(e) = event.listeners.add_listener(id, conv, displays::listener()) {
            eprintln!("[jei] recipe types not read: {e:?}");
        }
        sources::server::listen(event);
    }

    /// jeiA: the server sends each player its recipes of every type in force. A client's own table has only
    /// vanilla's and the content mods' recipes: without this, no card showed a native mod's recipe (AE2's controller,
    /// processors and inscriber, `/jei show ae2:controller` found nothing). Other mods' answers add to this one.
    ///
    /// Off for now: on the Nightly of 2026-11-20 any type asked for here made rejoining a saved world drop with
    /// "End of stream" (sunrise-org/sunrise#4060). Turn it back on when that is fixed.
    fn datapack_sync(&self, _host: &mut dyn ServerHost, event: &mut DatapackSyncEvent<'_>) {
        if !SYNC_RECIPE_TYPES {
            return;
        }
        for t in displays::sync_types() {
            if !event.recipe_types.contains(&t) {
                event.recipe_types.push(t);
            }
        }
    }

    fn player_logged_in(&self, host: &mut dyn ServerHost, event: &mut PlayerLoggedInEvent<'_>) {
        displays::send(host, event.player);
        sources::server::warm();
    }

    fn client_player_logging_in(&self, _host: &mut dyn ClientHost, _event: &mut ClientPlayerLoggingInEvent) {
        with(|j| {
            j.in_world = true;
            j.built = None;
        });
        sources::client::forget();
    }

    fn client_player_logging_out(&self, _host: &mut dyn ClientHost, _event: &mut ClientPlayerLoggingOutEvent) {
        with(|j| {
            j.in_world = false;
            j.close();
            j.checks.clear();
            j.actions.clear();
        });
        *displays::DISPLAYS.lock().unwrap_or_else(|e| e.into_inner()) = None;
    }

    fn recipes_received(&self, _host: &mut dyn ClientHost, _event: &mut RecipesReceivedEvent<'_>) {
        with(|j| j.received += 1);
    }

    fn render_frame_pre(&self, host: &mut dyn ClientHost, _event: &mut RenderFramePreEvent) {
        if !with(|j| j.in_world) {
            return;
        }
        ensure_index(host);
        let millis = host.millis();
        sources::client::request(host, millis);
        // No display table yet: ask the server, every five seconds.
        if displays::DISPLAYS.lock().unwrap_or_else(|e| e.into_inner()).is_none()
            && with(|j| {
                let due = millis >= j.asked_displays + 5000;
                if due {
                    j.asked_displays = millis;
                }
                due
            })
        {
            displays::request(host);
        }
        with(|j| {
            j.millis = millis;
            bookmarks::load(j, host);
        });
        let actions = with(|j| std::mem::take(&mut j.actions));
        for action in actions {
            match action {
                Action::Panel => show_panel(host),
                // The game's viewer transfer: the open mod menu's `menu_transfer` part first, then the crafting grid.
                Action::Transfer(recipe, max) => host.transfer_recipe_id(&recipe, max),
                // A mod's own screen: the mod's "+" sends its own payload (`api::register_screen_transfer`).
                Action::ScreenTransfer(transfer, recipe, max) => transfer(host, &recipe, max),
                Action::Check(list) => {
                    for (display, recipe) in list {
                        if let Some(check) = host.transfer_recipe_check(&recipe) {
                            with(|j| j.checks.insert(display, check));
                        }
                    }
                }
                Action::Give(item, count) => {
                    let count = host.max_stack_size(item).unwrap_or(64).min(count);
                    if let Err(e) = host.give(item, count) {
                        eprintln!("[jei] give: {e}");
                    }
                }
                Action::SaveBookmarks => with(|j| bookmarks::save(j, host)),
                // jeiA
                Action::Say(line) => {
                    let _ = host.chat_line(Text::literal(line));
                }
            }
        }
    }

    fn panel_event(&self, host: &mut dyn ClientHost, event: &mut PanelEventEvent<'_>) {
        if event.mod_id != ID {
            return;
        }
        // The list goes back while the game still dispatches the search: only then does an empty result keep the
        // panel and its search box (sent later, it took the panel away until the world was rejoined).
        if let PanelEvent::Search(text) = event.event {
            with(|j| j.search = text.clone());
            show_panel(host);
            return;
        }
        let shift = host.modifiers().shift();
        let act = match event.event {
            PanelEvent::Key { key, .. } => key_act(host, key),
            _ => None,
        };
        // W (GuideME's key) over a panel item opens its guide page; the guide sees only the screen's own slots.
        if let PanelEvent::Key { key, stack } = event.event
            && act.is_none()
        {
            guide(host, key, stack.item);
        }
        with(|j| match event.event {
            PanelEvent::Clicked { index, right } => {
                let Some((item, components)) = j.listed.get(*index as usize) else { return };
                let item = *item;
                if shift {
                    // `give` hands out the plain item only: a subtype (a potion, a book) would come out as the
                    // wrong stack, so it is given nothing.
                    if components.is_empty() {
                        j.actions.push(Action::Give(item, 64));
                    }
                } else {
                    // A stack with components (a potion, a book, a mod's extra stack) opens on its subtype.
                    let focus = j.focus_of(item, components);
                    j.open(focus, *right);
                }
            }
            PanelEvent::Search(text) => {
                j.search = text.clone();
            }
            PanelEvent::Key { stack, .. } => {
                if let Some(act) = act {
                    // The stack's components come with it (a screen's slot, the creative screen's): a subtype finds
                    // only its own recipes.
                    let focus = j.focus_of(stack.item, &stack.components);
                    act_on(j, act, focus);
                }
            }
            _ => {}
        });
    }

    /// A container screen's dark background, drawn by JEI edge to edge in one even layer (vanilla's
    /// `renderTransparentBackground` colour): the game's own leaves the item panel's strip lit and doesn't reach
    /// the world beside JEI's window, so it is turned off here and `screen_render_pre` draws one over the window.
    fn screen_transparent_background(&self, _host: &dyn ClientHost, event: &mut ScreenTransparentBackgroundEvent<'_>) -> Flow {
        let container = event.screen == "inventory" || event.screen == "creative";
        if !container {
            return Flow::Continue;
        }
        Flow::Cancel
    }

    /// A tooltip the screen behind hangs from a point under JEI's open window (an inventory slot's) is not drawn:
    /// it would draw under the window. JEI's own tooltips pass.
    fn render_tooltip_pre(&self, _host: &dyn ClientHost, event: &mut RenderTooltipPreEvent<'_>) -> Flow {
        if window::OWN_TOOLTIP.load(std::sync::atomic::Ordering::Relaxed) {
            return Flow::Continue;
        }
        let at = [event.place.x as f32, event.place.y as f32];
        let under = window::OPEN_RECT.lock().unwrap_or_else(|e| e.into_inner()).is_some_and(|r| state::inside(r, at));
        if under { Flow::Cancel } else { Flow::Continue }
    }

    /// The even dark background in its place, before the screen, the panel and JEI's window draw over it.
    fn screen_render_pre(&self, _host: &dyn ClientHost, event: &mut ScreenRenderPreEvent<'_>) -> Flow {
        if event.screen.id == "inventory" || event.screen.id == "creative" {
            let [w, h] = event.draw.size();
            event.draw.fill([0.0, 0.0, w, h], DIM);
        } else {
            // The window draws only over these screens (and a mod's own, which draws itself before the window):
            // another screen's tooltips stay.
            if event.screen.id != "mod" {
                *window::OPEN_RECT.lock().unwrap_or_else(|e| e.into_inner()) = None;
            }
        }
        Flow::Continue
    }

    /// Last of every mod's (`priority = -100`): a mod that draws its own screen here (AE2's item screens) draws it
    /// under JEI's window, not over it.
    #[listener(priority = -100)]
    fn screen_render_post(&self, host: &dyn ClientHost, event: &mut ScreenRenderPostEvent<'_>) {
        let screen = event.screen;
        let container = screen.id == "inventory" || screen.id == "creative";
        // `jei::api::show` and `/jei show` wait for a container screen (or a mod's own screen, `api::show_stack`):
        // the chat a command came from closes first.
        let show = if container || screen.id == "mod" { api::with(|r| r.show.take()) } else { None };
        // The "+" a mod offers on its own screen now (`api::register_screen_transfer`).
        let screen_plus = match host.mod_screen().filter(|_| screen.id == "mod") {
            Some(s) => api::screen_transfers(host, s),
            None => Vec::new(),
        };
        // The guide draws under JEI (it is JEI's dependency): keep the bar off it while it is open.
        let mut guide_open = false;
        mod_api::if_installed!(host, guideme, {
            guide_open = guideme::api::is_open();
        });
        with(|j| {
            if !j.in_world {
                return;
            }
            // jeiA: a show that finds nothing yet waits for the index to be made again (the server's recipes and
            // sources arrive a few seconds into a world), and says so in chat if it still finds nothing.
            let now = j.millis;
            let gave_up = match show {
                Some((item, components, uses)) => j.ask_show(item, components, uses, now),
                None => j.try_show(now),
            };
            if let Some(line) = gave_up {
                j.actions.push(Action::Say(line));
            }
            j.screen_plus = screen_plus;
            j.creative = screen.id == "creative";
            // The open mod menu's type, for "+" on the categories a mod moves into it (`api::register_transfer`).
            if j.menu.as_deref() != screen.menu {
                j.menu = screen.menu.map(str::to_owned);
            }
            j.bar_rect = if container && !guide_open { bookmarks::rect(j, screen.rect) } else { None };
            j.screen_rect = container.then_some(screen.rect);
            window::draw(j, host, &mut *event.draw, screen.mouse);
        });
    }

    fn screen_exclusion_areas(&self, _host: &dyn ClientHost, event: &mut ScreenExclusionAreasEvent<'_>) {
        with(|j| event.areas.extend(j.bar_rect));
    }

    fn screen_mouse_button_pressed_pre(&self, host: &dyn ClientHost, event: &mut ScreenMouseButtonPressedPreEvent<'_>) -> Flow {
        let shift = host.modifiers().shift();
        let at = event.screen.mouse;
        with(|j| {
            if j.window.is_some() {
                let over_window = j.window_rect.is_some_and(|r| state::inside(r, at));
                if !over_window {
                    // The item panel and the bookmarks stay live under JEI's window, as JEI's list does beside its
                    // recipe screen: a click there shows that item's recipes in the open window (the panel's click
                    // comes back as `panel_event`), and only a click on nothing closes it.
                    if bookmarks::press(j, at, event.button, shift) {
                        return Flow::Cancel;
                    }
                    if j.screen_rect.is_some_and(|s| at[0] >= s[0] + s[2]) {
                        return Flow::Continue;
                    }
                }
                window::click(j, at, event.button, shift);
                return Flow::Cancel;
            }
            if bookmarks::press(j, at, event.button, shift) { Flow::Cancel } else { Flow::Continue }
        })
    }

    fn screen_mouse_button_released_pre(&self, _host: &dyn ClientHost, event: &mut ScreenMouseButtonReleasedPreEvent<'_>) -> Flow {
        let at = event.screen.mouse;
        with(|j| {
            if bookmarks::release(j, at) {
                return Flow::Cancel;
            }
            // A release over the item panel goes to it, as its press did.
            let on_panel = j.window_rect.is_none_or(|r| !state::inside(r, at)) && j.screen_rect.is_some_and(|s| at[0] >= s[0] + s[2]);
            if j.window.is_some() && !on_panel { Flow::Cancel } else { Flow::Continue }
        })
    }

    fn screen_mouse_dragged_pre(&self, _host: &dyn ClientHost, _event: &mut ScreenMouseDraggedPreEvent<'_>) -> Flow {
        with(|j| if j.window.is_some() || j.dragging.is_some() { Flow::Cancel } else { Flow::Continue })
    }

    fn screen_mouse_scrolled_pre(&self, _host: &dyn ClientHost, event: &mut ScreenMouseScrolledPreEvent<'_>) -> Flow {
        with(|j| {
            if j.window.is_none() {
                // The item panel is the game's, and Sunrise pages it with the wheel itself (sunrise#1425).
                return Flow::Continue;
            }
            window::scroll(j, event.scroll[1], event.screen.mouse); // jeiA: the wheel over a slot steps it
            Flow::Cancel
        })
    }

    fn screen_key_pressed_pre(&self, host: &dyn ClientHost, event: &mut ScreenKeyPressedPreEvent<'_>) -> Flow {
        let act = key_act(host, event.key);
        let at = event.screen.mouse;
        with(|j| {
            let over = window::hovered_focus(j, at).or_else(|| window::hovered_fluid(j, at).map(Focus::Fluid))
                .or_else(|| match j.target_at(at) {
                    Some(state::Target::Bookmark(i)) => j.bookmarks.get(i).map(|k| Focus::Item(*k)),
                    _ => None,
                });
            if j.window.is_none() {
                // Keys over the bookmark bar; screen slots and the panel come as panel events.
                if let (Some(act), Some(f)) = (act, over.filter(|_| j.bar_rect.is_some_and(|r| state::inside(r, at)))) {
                    act_on(j, act, f);
                    return Flow::Cancel;
                }
                // The creative screen has no panel, so its slots send no panel events: the slot under the pointer
                // is read here, its components (a subtype) included.
                if event.screen.id == "creative"
                    && let Some(act) = act
                    && let Some(f) = creative_focus(host, j, at)
                {
                    act_on(j, act, f);
                    return Flow::Cancel;
                }
                return Flow::Continue;
            }
            match event.key {
                "key.keyboard.escape" => j.close(),
                "key.keyboard.backspace" => {
                    j.window = j.history.pop();
                }
                _ => {
                    if let (Some(act), Some(f)) = (act, over.clone()) {
                        act_on(j, act, f);
                    } else if let Some(item) = over.as_ref().and_then(Focus::item)
                        && guide(host, event.key, item)
                    {
                        j.close();
                    }
                }
            }
            Flow::Cancel
        })
    }

    fn screen_char_typed_pre(&self, _host: &dyn ClientHost, _event: &mut ScreenCharTypedPreEvent<'_>) -> Flow {
        with(|j| if j.window.is_some() { Flow::Cancel } else { Flow::Continue })
    }

    fn register_client_commands(&self, _host: &mut dyn ClientHost, event: &mut RegisterClientCommandsEvent<'_>) {
        for usage in ["jei debug", "jei search [text:text]", "jei show <item:item>", "jei uses <item:item>", "jei why <item:item>", "jei bookmarks clear"] { // jeiA: why
            event.commands.push(Command::new(usage));
        }
    }

    /// `/jei search <text>` sets the panel's filter as typing in its box does; `/jei show|uses <item>` opens the
    /// window when a container screen is next open; `/jei bookmarks clear`.
    fn command(&self, _host: &mut dyn ClientHost, event: &mut CommandEvent<'_>) -> Flow {
        let line = event.command.trim_start_matches('/');
        let Some(rest) = line.strip_prefix("jei") else { return Flow::Continue };
        let rest = rest.trim();
        if let Some(text) = rest.strip_prefix("search") {
            with(|j| {
                j.search = text.trim().to_owned();
                j.actions.push(Action::Panel);
            });
        } else if let Some(item) = rest.strip_prefix("show ").or_else(|| rest.strip_prefix("uses ")) {
            let id = item.trim();
            let id = if id.contains(':') { id.to_owned() } else { format!("minecraft:{id}") };
            api::show(&id, rest.starts_with("uses"));
        } else if let Some(item) = rest.strip_prefix("why ") {
            // jeiA: why R or U finds (or doesn't find) an item: its key, the cards that make and use it by category.
            let id = item.trim();
            let id = if id.contains(':') { id.to_owned() } else { format!("minecraft:{id}") };
            let line = with(|j| why(j, &id));
            for l in line {
                let _ = _host.chat_line(Text::literal(l));
            }
        } else if rest == "debug" {
            // What "+" needs, for testing: the server's display table and the window's answers.
            let table = displays::DISPLAYS.lock().unwrap_or_else(|e| e.into_inner()).as_ref().map(|m| m.len());
            let table_id = displays::of("minecraft:crafting_table");
            let line = with(|j| {
                format!(
                    "[jei] items {} cards {} categories {} types {} displays {:?} ({}) crafting_table {:?} checks {:?}",
                    j.items.len(), j.cards.len(), j.categories.len(), displays::types(), table,
                    displays::LAST.lock().unwrap_or_else(|e| e.into_inner()), table_id,
                    j.checks.iter().map(|(k, v)| (*k, v.result)).collect::<Vec<_>>()
                )
            });
            // Cards per category, and how many recipes the client lists per vanilla type.
            let per = with(|j| {
                j.categories.iter().enumerate()
                    .map(|(i, c)| format!("{} {}", c.id, j.cards.iter().filter(|k| k.category == i).count()))
                    .collect::<Vec<_>>().join(", ")
            });
            let _ = _host.chat_line(Text::literal(format!("[jei] cards: {per}")));
            let kinds = ["minecraft:crafting", "minecraft:smithing", "minecraft:smelting"]
                .iter().map(|k| format!("{k} {}", _host.recipes_of_type(k).len())).collect::<Vec<_>>().join(", ");
            let _ = _host.chat_line(Text::literal(format!("[jei] recipes: {kinds}")));
            let _ = _host.chat_line(Text::literal(format!("[jei] {}", sources::client::status())));
            // Other mods' extra stacks (`api::register_extra_stacks`): how many functions, stacks and entries.
            let extras = with(|j| {
                let with_components = j.items.iter().filter(|(_, c)| !c.is_empty()).count();
                format!(
                    "[jei] extra stacks: {} functions gave {} at the last index; entries {} ({} with components, {} named)",
                    j.extra_counts.0, j.extra_counts.1, j.items.len(), with_components, j.extra_names.len()
                )
            });
            let _ = _host.chat_line(Text::literal(extras));
            let _ = _host.chat_line(Text::literal(line));
        } else if rest == "bookmarks clear" {
            with(|j| {
                j.bookmarks.clear();
                j.actions.push(Action::SaveBookmarks);
            });
        } else {
            return Flow::Continue;
        }
        Flow::Cancel
    }

    fn screen_closing(&self, _host: &dyn ClientHost, _event: &mut ScreenClosingEvent<'_>) {
        with(|j| {
            j.close();
            j.dragging = None;
            j.bar_rect = None;
            j.checks.clear();
        });
    }
}

/// `/jei why <item>`: whether JEI knows the item, and the cards that make and use it by category. // jeiA
fn why(j: &state::Jei, id: &str) -> Vec<String> {
    let Some(k) = j.ids.get(id).copied() else {
        return vec![format!("[jei] {id}: not an item this client knows")];
    };
    let count = |make: bool| -> String {
        let mut per: Vec<(String, usize)> = Vec::new();
        for c in j.cards.iter().filter(|c| if make { c.makes(k) } else { c.uses(k) }) {
            let cat = j.categories.get(c.category).map(|c| c.id.clone()).unwrap_or_default();
            match per.iter_mut().find(|(n, _)| *n == cat) {
                Some((_, n)) => *n += 1,
                None => per.push((cat, 1)),
            }
        }
        if per.is_empty() { "none".to_owned() } else { per.iter().map(|(c, n)| format!("{c} {n}")).collect::<Vec<_>>().join(", ") }
    };
    let made = count(true);
    let used = count(false);
    vec![
        format!("[jei] {id}: made by {made}"),
        format!("[jei] {id}: used by {used}"),
        format!("[jei] index {:?}, {} cards, waiting show {:?}", j.built, j.cards.len(), j.pending_show.as_ref().map(|p| &p.item)),
    ]
}

/// Sends the item list, filtered by the search text, to the game's panel.
fn show_panel(host: &mut dyn ClientHost) {
    let listed = with(|j| {
        if !j.search.is_empty() {
            search::ensure_names(j, host);
        }
        let listed = search::filter(j, host, &j.search.clone());
        j.listed = listed.clone();
        listed
    });
    // The panel reads components in the item syntax (`[k=v]`); JEI keeps them as JSON.
    let stacks = listed
        .into_iter()
        .map(|(item, components)| Stack { item, count: 1, components: api::panel_components(&components) })
        .collect();
    host.panel_items(ID, stacks);
}


#[cfg(test)]
mod card_tests {
    use super::*;
    use view::{Card, Role, Well};

    /// A recipe whose items no client knows is left out; a fluid alone is something to show.
    #[test]
    fn cards_with_nothing_to_show_are_left_out() {
        assert!(!shows_something(&Card::new(0, [100.0, 40.0], "empty")));
        let mut unresolved = Card::new(0, [100.0, 40.0], "minecraft:special");
        unresolved.slot([0.0, 0.0], Role::Input, Vec::new());
        unresolved.slot([40.0, 0.0], Role::Output, Vec::new());
        assert!(!shows_something(&unresolved));
        let mut fluid = Card::new(0, [100.0, 40.0], "melting");
        fluid.slot([0.0, 0.0], Role::Input, Vec::new());
        fluid.wells.push(Well {
            rect: [40.0, 0.0, 16.0, 40.0],
            role: Role::Output,
            fluid: "minecraft:lava".into(),
            amount: 1000,
            capacity: 1000,
            bare: false,
            overlay: None,
        });
        assert!(shows_something(&fluid));
    }
}
