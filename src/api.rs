//! JEI's API for other fork mods, in Rust: a mod names `jei` under `[dependencies]` in its `horizon-mod.toml`
//! (`jei = { version = "^0.3", optional = true }`) and calls these from its hooks inside
//! `mod_api::if_installed!(host, jei, { ... })`. The launcher links the two crates, so the calls reach this crate's
//! statics. The same can be said in files (`assets/<ns>/jei/`, see `provider.rs`); use whichever fits: files for
//! recipes the data packs hold, code for recipes worked out at run time (a mod's own registries, configs).
//!
//! ```rust,ignore
//! mod_api::if_installed!(host, jei, {
//!     jei::api::register_category(jei::api::CategoryDef::new("powah:energizing", "jei.powah.energizing", "powah:energizing_orb")
//!         .catalyst("powah:energizing_rod"));
//!     jei::api::register_recipes("powah", |host| vec![("powah:energizing".into(),
//!         jei::api::RecipeDef::new("powah:energizing/x", [120.0, 40.0])
//!             .input(1.0, 1.0, &[("minecraft:redstone", 1)])
//!             .output(90.0, 12.0, &[("powah:energized_steel", 1)])
//!             .text(1.0, 30.0, "10000 FE"))]);
//! });
//! ```

use mod_api::{ClientHost, Host};
use std::sync::Mutex;

// Extra areas: where a screen draws beyond its window, which JEI's item list keeps clear of (see `extra_areas`).
pub use crate::extra_areas::{ExtraAreasFn, register_global_extra_areas, register_gui_extra_areas};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SlotRole {
    Input,
    Output,
    /// Shown but neither used up nor made (a fuel, a tool that stays).
    Other,
}

#[derive(Clone, Debug)]
pub struct SlotDef {
    pub x: f32,
    pub y: f32,
    pub role: SlotRole,
    /// Item ids and counts the slot cycles through.
    pub items: Vec<(String, u32)>,
    /// A chance under 1 is written under the slot.
    pub chance: Option<f32>,
}

#[derive(Clone, Debug)]
pub struct FluidDef {
    pub rect: [f32; 4],
    pub role: SlotRole,
    pub fluid: String,
    /// mB.
    pub amount: u32,
    /// What a full well holds, mB.
    pub capacity: u32,
}

/// One recipe card: its size and parts, placed from its top left in GUI pixels.
#[derive(Clone, Debug)]
pub struct RecipeDef {
    pub id: String,
    pub size: [f32; 2],
    pub slots: Vec<SlotDef>,
    pub fluids: Vec<FluidDef>,
    /// (where, text, ARGB colour). The text is a translation key or literal.
    pub texts: Vec<([f32; 2], String, u32)>,
    pub arrow: Option<[f32; 2]>,
    pub shapeless: bool,
    /// A GUI sprite under the card: (sprite, [u, v, w, h]).
    pub background: Option<(String, [f32; 4])>,
    /// Places with a tooltip of their own: [x, y, w, h] from the card's top left, and its lines (translation keys
    /// or text, `§` codes allowed).
    pub hovers: Vec<([f32; 4], Vec<String>)>,
    /// GUI sprites over the slots (a slot's empty icon, a status icon): (sprite, [u, v, w, h], [x, y] on the card).
    pub sprites: Vec<(String, [f32; 4], [f32; 2])>,
    /// Fluids drawn bare (no frame, no grey fill: the background has its own), each with an optional sprite part
    /// drawn over it (a tank's gauge): (index into `fluids`, overlay).
    pub bare_fluids: Vec<(usize, Option<(String, [f32; 4])>)>,
    /// Items drawn as plain icons, not slots (a station block under a cast): (item id, [x, y]).
    pub icons: Vec<(String, [f32; 2])>,
    /// Sprite parts revealed left to right over a period in ms, again and again (JEI's animated arrow):
    /// (sprite, [u, v, w, h], [x, y], period).
    pub animated: Vec<(String, [f32; 4], [f32; 2], u32)>,
    /// Lines centred on x: (where, text, ARGB colour), as `texts`.
    pub centered: Vec<([f32; 2], String, u32)>,
    /// No slot frames: the background draws its own (or none, as Java's cast slot).
    pub frameless: bool,
    /// GUI sprite parts drawn under the slots, over the background: as `sprites`.
    pub sprites_under: Vec<(String, [f32; 4], [f32; 2])>,
}

impl RecipeDef {
    pub fn new(id: &str, size: [f32; 2]) -> Self {
        Self { id: id.to_owned(), size, slots: Vec::new(), fluids: Vec::new(), texts: Vec::new(), arrow: None, shapeless: false, background: None, hovers: Vec::new(), sprites: Vec::new(), bare_fluids: Vec::new(), icons: Vec::new(), animated: Vec::new(), centered: Vec::new(), frameless: false, sprites_under: Vec::new() }
    }
    fn slot(mut self, x: f32, y: f32, role: SlotRole, items: &[(&str, u32)]) -> Self {
        self.slots.push(SlotDef { x, y, role, items: items.iter().map(|(i, n)| ((*i).to_owned(), *n)).collect(), chance: None });
        self
    }
    pub fn input(self, x: f32, y: f32, items: &[(&str, u32)]) -> Self {
        self.slot(x, y, SlotRole::Input, items)
    }
    pub fn output(self, x: f32, y: f32, items: &[(&str, u32)]) -> Self {
        self.slot(x, y, SlotRole::Output, items)
    }
    pub fn other(self, x: f32, y: f32, items: &[(&str, u32)]) -> Self {
        self.slot(x, y, SlotRole::Other, items)
    }
    /// The last slot's chance (0..1).
    pub fn chance(mut self, chance: f32) -> Self {
        if let Some(s) = self.slots.last_mut() {
            s.chance = Some(chance);
        }
        self
    }
    pub fn fluid(mut self, rect: [f32; 4], role: SlotRole, fluid: &str, amount: u32, capacity: u32) -> Self {
        self.fluids.push(FluidDef { rect, role, fluid: fluid.to_owned(), amount, capacity: capacity.max(1) });
        self
    }
    pub fn text(mut self, x: f32, y: f32, text: &str) -> Self {
        self.texts.push(([x, y], text.to_owned(), 0xFF40_4040));
        self
    }
    /// A line from a translation key with `%s` per argument; an argument written with [`tr_arg`] is translated
    /// too (`text_tr(x, y, "%s %s", &[&tr_arg("enchantment.minecraft.sharpness"), &tr_arg("enchantment.level.2")])`
    /// shows "Sharpness II"). A key with no translation is used as written.
    pub fn text_tr(mut self, x: f32, y: f32, key: &str, args: &[&str]) -> Self {
        let mut t = key.to_owned();
        for a in args {
            t.push('\u{3}');
            t.push_str(a);
        }
        self.texts.push(([x, y], t, 0xFF40_4040));
        self
    }
    pub fn arrow(mut self, x: f32, y: f32) -> Self {
        self.arrow = Some([x, y]);
        self
    }
    pub fn shapeless(mut self) -> Self {
        self.shapeless = true;
        self
    }
    /// A tooltip over `rect` ([x, y, w, h]): its lines are translation keys or text.
    pub fn hover(mut self, rect: [f32; 4], lines: &[&str]) -> Self {
        self.hovers.push((rect, lines.iter().map(|l| (*l).to_owned()).collect()));
        self
    }
    /// GUI sprite `sprite`'s part `uv` ([u, v, w, h] in its pixels) drawn at its own size at (x, y), over the
    /// slots and fluids and under the texts (added 2026-11-14; Tinkers' slot and requirement icons).
    pub fn sprite(mut self, sprite: &str, uv: [f32; 4], x: f32, y: f32) -> Self {
        self.sprites.push((sprite.to_owned(), uv, [x, y]));
        self
    }
    pub fn background(mut self, sprite: &str, uv: [f32; 4]) -> Self {
        self.background = Some((sprite.to_owned(), uv));
        self
    }
    /// The last `fluid` drawn bare (no frame or grey fill, for a background that draws its own), with sprite part
    /// `overlay` ([u, v, w, h], stretched over the fluid's rect) on top: a tank's gauge (added 2026-11-14).
    pub fn fluid_bare(mut self, overlay: Option<(&str, [f32; 4])>) -> Self {
        if !self.fluids.is_empty() {
            self.bare_fluids.push((self.fluids.len() - 1, overlay.map(|(s, uv)| (s.to_owned(), uv))));
        }
        self
    }
    /// Item `item`'s icon at (x, y), 16 square, with no slot frame (Tinkers' table under the cast; added 2026-11-14).
    pub fn icon(mut self, item: &str, x: f32, y: f32) -> Self {
        self.icons.push((item.to_owned(), [x, y]));
        self
    }
    /// Sprite part `uv` drawn at (x, y), revealed left to right over `period_ms`, then again (JEI's
    /// `buildAnimated(.., StartDirection.LEFT, false)`; added 2026-11-14).
    pub fn animated(mut self, sprite: &str, uv: [f32; 4], x: f32, y: f32, period_ms: u32) -> Self {
        self.animated.push((sprite.to_owned(), uv, [x, y], period_ms.max(1)));
        self
    }
    /// As `sprite`, but drawn under the slots (a slot frame of the mod's own; added 2026-11-14).
    pub fn sprite_under(mut self, sprite: &str, uv: [f32; 4], x: f32, y: f32) -> Self {
        self.sprites_under.push((sprite.to_owned(), uv, [x, y]));
        self
    }
    /// Slots drawn with no frame, for a background that has its own (added 2026-11-14).
    pub fn frameless(mut self) -> Self {
        self.frameless = true;
        self
    }
    /// A line centred on x, in `argb`: a translation key or literal, with `text_tr`'s arguments (added 2026-11-14).
    pub fn text_centered(mut self, x: f32, y: f32, text: &str, argb: u32) -> Self {
        self.centered.push(([x, y], text.to_owned(), argb));
        self
    }
}

#[derive(Clone, Debug)]
pub struct CategoryDef {
    pub id: String,
    /// A translation key or the text.
    pub title: String,
    pub icon: String,
    pub catalysts: Vec<String>,
}

impl CategoryDef {
    pub fn new(id: &str, title: &str, icon: &str) -> Self {
        Self { id: id.to_owned(), title: title.to_owned(), icon: icon.to_owned(), catalysts: vec![icon.to_owned()] }
    }
    pub fn catalyst(mut self, item: &str) -> Self {
        if !self.catalysts.iter().any(|c| c == item) {
            self.catalysts.push(item.to_owned());
        }
        self
    }
}

/// A function giving a mod's recipes, asked each time JEI makes its index (joining a world, a resource or data
/// reload): (category id, card) pairs.
pub type RecipesFn = fn(&dyn Host) -> Vec<(String, RecipeDef)>;

#[derive(Default)]
pub(crate) struct Registry {
    pub categories: Vec<CategoryDef>,
    pub catalysts: Vec<(String, String)>,
    pub recipes: Vec<(String, RecipesFn)>,
    pub info: Vec<(Vec<String>, Vec<String>)>,
    /// `show`/`show_stack`: (item, components' JSON, uses).
    pub show: Option<(String, String, bool)>,
    /// (item, component): stacks of the item told apart by that component's value (`register_subtype`).
    pub subtypes: Vec<(String, String)>,
    /// (menu type, category): "+" on the category's cards moves into that menu (`register_transfer`).
    pub transfers: Vec<(String, String)>,
    /// (owner, the item they follow, the function): extra stacks in the item list (`register_extra_stacks`).
    pub extras: Vec<(String, String, ExtrasFn)>,
    /// (mod screen, category, whether it applies, the mod's "+"): `register_screen_transfer`.
    pub screen_transfers: Vec<(String, String, ScreenOpenFn, ScreenTransferFn)>,
    /// (mod menu, its "+" dry run): `register_transfer_state`.
    pub transfer_states: Vec<(String, TransferStateFn)>,
    pub version: u64,
}

pub(crate) static REGISTRY: Mutex<Option<Registry>> = Mutex::new(None);

pub(crate) fn with<T>(f: impl FnOnce(&mut Registry) -> T) -> T {
    let mut g = REGISTRY.lock().unwrap_or_else(|e| e.into_inner());
    f(g.get_or_insert_with(Registry::default))
}

/// Adds a category (or, for an id already there such as `minecraft:smelting`, its catalysts).
pub fn register_category(def: CategoryDef) {
    with(|r| {
        r.categories.retain(|c| c.id != def.id);
        r.categories.push(def);
        r.version += 1;
    });
}

/// Another block or item that does a category's recipes (vanilla's included: `minecraft:smelting`).
pub fn add_catalyst(category: &str, item: &str) {
    with(|r| {
        r.catalysts.push((category.to_owned(), item.to_owned()));
        r.version += 1;
    });
}

/// A mod's recipes, by function; one per `owner` (a second call replaces the first).
pub fn register_recipes(owner: &str, recipes: RecipesFn) {
    with(|r| {
        r.recipes.retain(|(o, _)| o != owner);
        r.recipes.push((owner.to_owned(), recipes));
        r.version += 1;
    });
}

/// JEI's `ISubtypeInterpreter`: stacks of `item` are told apart by the value of their data component `component`
/// (`hostilenetworks:prediction` by `hostilenetworks:data_model`). R and U on such a stack (a screen's slot, the
/// creative screen's, a card's) then find only the recipes whose slot holds a stack with the same value; a slot that
/// names the item without components still matches every subtype.
///
/// A recipe's slot names a stack with components by appending them to the id in the game's JSON form:
/// `hostilenetworks:prediction{"hostilenetworks:data_model":"hostilenetworks:zombie"}`. The slot draws that stack
/// (a tinted prediction) and its tooltip is that stack's.
pub fn register_subtype(item: &str, component: &str) {
    with(|r| {
        if !r.subtypes.iter().any(|(i, c)| i == item && c == component) {
            r.subtypes.push((item.to_owned(), component.to_owned()));
            r.version += 1;
        }
    });
}

/// JEI's `addRecipeTransferHandler`: while a screen of mod menu type `menu` (`tconstruct:part_builder`) is open, the
/// cards of `category` get "+". A click sends the card's recipe id through the game's viewer transfer
/// (`RecipesClientHost::transfer_recipe_id`, Shift for as many sets as the items allow); the server asks the menu
/// type's `MenuPart::menu_transfer`, which moves the ingredients and answers. The mod's part does all the checking:
/// there is no dry run for a mod menu, so "+" is always offered there and a refusal shows when it is clicked.
pub fn register_transfer(menu: &str, category: &str) {
    with(|r| {
        if !r.transfers.iter().any(|(m, c)| m == menu && c == category) {
            r.transfers.push((menu.to_owned(), category.to_owned()));
        }
    });
}

/// How "+" stands for one recipe on a mod's menu, worked out on the client (JEI's `IRecipeTransferError` from a
/// handler's dry run): the cells to mark while the pointer is on "+", the tooltip, and whether "+" works.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TransferState {
    /// "+" can't be used (a user-facing error, JEI's `createUserErrorWithTooltip`): drawn inactive, the click ignored.
    pub blocked: bool,
    /// Cells to mark (`TransferCell::cell`) and their ARGB colour, drawn over the slot while the pointer is on "+"
    /// (`IRecipeSlotView.drawHighlight`).
    pub marks: Vec<(u32, u32)>,
    /// The tooltip's lines, in place of "Move Items"; `§` colour codes as the game's text takes them. Empty: JEI's.
    pub lines: Vec<String>,
    /// An ARGB colour laid over "+" (JEI's tint for a cosmetic error); None for none.
    pub tint: Option<u32>,
}

/// One input cell of a recipe card: its number (the recipe's own: width by height for a shaped recipe, else its
/// ingredients in order) and the items it takes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TransferCell {
    pub cell: u32,
    pub items: Vec<mod_api::ItemKey>,
}

/// A mod's dry run of "+" for a recipe of `category` into its open menu `menu`: the client's own reading of what the
/// click would do (AE2: what the terminal's network and the inventory hold or can craft). None: plain "+".
pub type TransferStateFn = fn(&dyn ClientHost, menu: &str, category: &str, cells: &[TransferCell]) -> Option<TransferState>;

/// JEI's dry run for a mod menu's "+" (0.5.5): while a screen of mod menu type `menu` is open, `state` is asked each
/// frame for every card "+" shows on (`register_transfer`), and its answer marks the cells, sets the tooltip and
/// blocks or tints "+" as a JEI transfer handler's `IRecipeTransferError` does. The click still goes to the server's
/// `MenuPart::menu_transfer`, which decides. One function per menu type (a second call replaces the first).
pub fn register_transfer_state(menu: &str, state: TransferStateFn) {
    with(|r| {
        r.transfer_states.retain(|(m, _)| m != menu);
        r.transfer_states.push((menu.to_owned(), state));
    });
}

pub(crate) fn transfer_state_for(menu: &str) -> Option<TransferStateFn> {
    with(|r| r.transfer_states.iter().find(|(m, _)| m == menu).map(|(_, f)| *f))
}

/// One more stack for JEI's item list (`register_extra_stacks`): an item with data components, listed beside the
/// plain item as JEI's `IModIngredientRegistration` lists a mod's own ingredients (AE2's one facade per block).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExtraStack {
    /// The item id (`ae2:facade`).
    pub item: String,
    /// Its components in the game's JSON form (`{"minecraft:custom_data":{"item":"minecraft:stone"}}`); empty for
    /// none. The panel draws the stack with them, and R and U on it find its own recipes when the item has a
    /// subtype registered for one of them (`register_subtype`).
    pub components: String,
    /// More text the search matches as the stack's name (its own tooltip is searched too): "Cable Facade - Stone"
    /// when the game's tooltip says only "Cable Facade". Empty for none.
    pub name: String,
}

impl ExtraStack {
    pub fn new(item: &str, components: &str) -> Self {
        Self { item: item.to_owned(), components: components.to_owned(), name: String::new() }
    }
    /// Text the search box matches as this stack's name.
    pub fn name(mut self, name: &str) -> Self {
        self.name = name.to_owned();
        self
    }
}

/// A function giving a mod's extra stacks, asked each time JEI makes its index (client side; it may translate).
pub type ExtrasFn = fn(&dyn ClientHost) -> Vec<ExtraStack>;

/// Extra stacks for JEI's item list (0.5.1): `stacks` gives them, in order, each time the list is made; they are
/// listed right after the entries of item `after` (after its last, when it has several), or at the end when the list
/// has no such item. A stack already listed (same item and components) is not listed twice. They are searched by
/// their tooltip and `ExtraStack::name`, and R/U, clicks and bookmarks treat them as any entry. One function per
/// `owner` (a second call replaces the first).
pub fn register_extra_stacks(owner: &str, after: &str, stacks: ExtrasFn) {
    with(|r| {
        r.extras.retain(|(o, _, _)| o != owner);
        r.extras.push((owner.to_owned(), after.to_owned(), stacks));
        r.version += 1;
    });
}

/// Whether a mod's screen that `register_screen_transfer` names is the one open now (one mod screen can stand for
/// several, as AE2's item screen does for its terminals). Client side, asked each frame while it is open.
pub type ScreenOpenFn = fn(&dyn ClientHost) -> bool;
/// A mod's "+" on its own screen: the recipe's id, and Shift (as many sets as the items allow). Client side, called
/// on the next frame with the mutable host; the mod sends its own payload, and its server checks everything.
pub type ScreenTransferFn = fn(&mut dyn ClientHost, &str, bool);

/// "+" on a mod's own screen (0.5.1), for a screen with no menu (`ClientHost::mod_screen_open`), which
/// `register_transfer` can't name: while mod screen `screen` is open (`ClientHost::mod_screen() == Some(screen)`)
/// and `open` says so, the cards of `category` get "+", and a click calls `transfer` with the card's recipe id and
/// Shift. As for a mod menu there is no dry run: "+" is always offered there, and the mod's server answers the click.
/// A second call for the same screen and category replaces the first.
pub fn register_screen_transfer(screen: &str, category: &str, open: ScreenOpenFn, transfer: ScreenTransferFn) {
    with(|r| {
        r.screen_transfers.retain(|(s, c, _, _)| !(s == screen && c == category));
        r.screen_transfers.push((screen.to_owned(), category.to_owned(), open, transfer));
    });
}

/// The screen transfers that apply on mod screen `screen` now: (category, the mod's "+").
pub(crate) fn screen_transfers(host: &dyn ClientHost, screen: &str) -> Vec<(String, ScreenTransferFn)> {
    let list: Vec<(String, ScreenOpenFn, ScreenTransferFn)> = with(|r| {
        r.screen_transfers.iter().filter(|(s, _, _, _)| s == screen).map(|(_, c, o, t)| (c.clone(), *o, *t)).collect()
    });
    // Asked outside the registry's lock: the mod's function may call back into this API.
    list.into_iter().filter(|(_, open, _)| open(host)).map(|(c, _, t)| (c, t)).collect()
}

/// The "+" for `category` among the screen transfers in force (`screen_transfers`), if any.
pub(crate) fn screen_transfer_for<F: Copy>(active: &[(String, F)], category: &str) -> Option<F> {
    active.iter().find(|(c, _)| c == category).map(|(_, f)| *f)
}

/// The extra stacks placed into the list (pure): each group goes right after the last entry of its anchor item, in
/// registration order, or at the end when the anchor isn't listed; a stack already in the list is left out.
pub(crate) fn place_extras<K: Copy + PartialEq>(mut items: Vec<(K, String)>, groups: Vec<(Option<K>, Vec<(K, String)>)>) -> Vec<(K, String)> {
    for (anchor, stacks) in groups {
        let mut fresh: Vec<(K, String)> = Vec::new();
        for s in stacks {
            if !items.contains(&s) && !fresh.contains(&s) {
                fresh.push(s);
            }
        }
        let at = anchor.and_then(|a| items.iter().rposition(|(k, _)| *k == a)).map_or(items.len(), |i| i + 1);
        items.splice(at..at, fresh);
    }
    items
}

/// The extra stacks of every mod, made now: (anchor item id, stacks), in registration order.
pub(crate) fn extras(host: &dyn ClientHost) -> Vec<(String, Vec<ExtraStack>)> {
    let fns: Vec<(String, ExtrasFn)> = with(|r| r.extras.iter().map(|(_, a, f)| (a.clone(), *f)).collect());
    fns.into_iter().map(|(a, f)| (a, f(host))).collect()
}

/// JEI's key for an item's recipes or uses, for a mod screen that draws its own stacks (0.5.1): `Some(false)` when
/// `key` (`key.keyboard.r`) is bound to Show Recipes, `Some(true)` for Show Uses, else None. The mod then calls
/// [`show_stack`] with the stack under the pointer. Client side.
pub fn lookup_key(host: &dyn ClientHost, key: &str) -> Option<bool> {
    match crate::key_act(host, key)? {
        crate::KeyAct::Recipes => Some(false),
        crate::KeyAct::Uses => Some(true),
        crate::KeyAct::Bookmark => None,
    }
}

/// As [`show`], for a stack with components (their JSON, empty for none): a subtype opens on its own recipes (a
/// facade of one block). Opens over a container screen or a mod's own screen (0.5.1).
pub fn show_stack(item: &str, components: &str, uses: bool) {
    with(|r| r.show = Some((item.to_owned(), components.to_owned(), uses)));
}

/// Whether a mod moves `category`'s recipes into menu type `menu` (`register_transfer`).
pub(crate) fn transfers_into(menu: &str, category: &str) -> bool {
    with(|r| r.transfers.iter().any(|(m, c)| m == menu && c == category))
}

/// An item spec as a slot names it: the id, and the components' JSON (empty for none).
pub(crate) fn split_spec(spec: &str) -> (&str, &str) {
    match spec.find('{') {
        Some(i) => (&spec[..i], &spec[i..]),
        None => (spec, ""),
    }
}

/// The value, as compact JSON, of `component` in a stack's components JSON (`Stack::components`).
pub(crate) fn component_value(components: &str, component: &str) -> Option<String> {
    if components.is_empty() {
        return None;
    }
    // A screen's slot gives its components in vanilla's item syntax (`[ns:name="value",other={...}]`).
    if components.starts_with('[') {
        return syntax_value(components, component);
    }
    let v: mod_api::serde_json::Value = mod_api::serde_json::from_str(components).ok()?;
    v.get(component).map(|x| x.to_string())
}

/// `component`'s value in vanilla's item syntax, as `component_value` gives a JSON one: a quoted string as a JSON
/// string, anything else as written.
fn syntax_value(components: &str, component: &str) -> Option<String> {
    let inner = components.strip_prefix('[')?.strip_suffix(']')?;
    let bytes: Vec<char> = inner.chars().collect();
    let mut i = 0;
    while i < bytes.len() {
        // A key, up to `=`.
        let start = i;
        while i < bytes.len() && bytes[i] != '=' {
            i += 1;
        }
        let key: String = bytes[start..i].iter().collect();
        i += 1;
        // Its value, up to a `,` outside brackets and quotes.
        let vstart = i;
        let (mut depth, mut quoted, mut escaped) = (0i32, false, false);
        while i < bytes.len() {
            let c = bytes[i];
            if quoted {
                if escaped {
                    escaped = false;
                } else if c == '\\' {
                    escaped = true;
                } else if c == '"' {
                    quoted = false;
                }
            } else if c == '"' {
                quoted = true;
            } else if c == '{' || c == '[' {
                depth += 1;
            } else if c == '}' || c == ']' {
                depth -= 1;
            } else if c == ',' && depth == 0 {
                break;
            }
            i += 1;
        }
        let value: String = bytes[vstart..i.min(bytes.len())].iter().collect();
        i += 1;
        if key.trim() == component {
            let v = value.trim();
            return Some(match v.strip_prefix('"').and_then(|x| x.strip_suffix('"')) {
                Some(text) => mod_api::serde_json::Value::String(text.replace("\\\"", "\"")).to_string(),
                // A compound or list (`{item:"minecraft:stone"}`) as the JSON a recipe's slot writes, so the two
                // compare (a facade in an inventory slot finds its own recipe).
                None => snbt_json(v).unwrap_or_else(|| v.to_owned()),
            });
        }
    }
    None
}

/// A compound or list in vanilla's item syntax as compact JSON: bare keys quoted, numbers' type letters dropped
/// (`{item:"minecraft:stone",n:1b}` is `{"item":"minecraft:stone","n":1}`). None when it isn't one.
pub(crate) fn snbt_json(text: &str) -> Option<String> {
    let t = text.trim();
    if !(t.starts_with('{') || t.starts_with('[')) {
        return None;
    }
    let chars: Vec<char> = t.chars().collect();
    let mut out = String::with_capacity(t.len() + 16);
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c == '"' || c == '\'' {
            // A string: copied as a JSON string.
            let quote = c;
            let mut s = String::new();
            i += 1;
            while i < chars.len() && chars[i] != quote {
                if chars[i] == '\\' && i + 1 < chars.len() {
                    i += 1;
                }
                s.push(chars[i]);
                i += 1;
            }
            out.push_str(&mod_api::serde_json::Value::String(s).to_string());
            i += 1;
        } else if c.is_ascii_alphanumeric() || c == '_' || c == '-' || c == '.' || c == '+' {
            // A bare word: a key (before `:`), or a number with an optional type letter, or true/false.
            let start = i;
            while i < chars.len() && (chars[i].is_ascii_alphanumeric() || "_-.+".contains(chars[i])) {
                i += 1;
            }
            let word: String = chars[start..i].iter().collect();
            let mut j = i;
            while j < chars.len() && chars[j].is_whitespace() {
                j += 1;
            }
            if chars.get(j) == Some(&':') {
                out.push_str(&mod_api::serde_json::Value::String(word).to_string());
            } else if word == "true" || word == "false" {
                out.push_str(&word);
            } else {
                let num = word.trim_end_matches(['b', 'B', 's', 'S', 'l', 'L', 'f', 'F', 'd', 'D']);
                num.parse::<f64>().ok()?;
                out.push_str(num);
            }
        } else if c.is_whitespace() {
            i += 1;
        } else {
            out.push(c);
            i += 1;
        }
    }
    let v: mod_api::serde_json::Value = mod_api::serde_json::from_str(&out).ok()?;
    Some(v.to_string())
}

/// Components' JSON (`{"minecraft:custom_data":{"item":"minecraft:stone"}}`) in the item syntax the game's panel
/// reads (`[minecraft:custom_data={item:"minecraft:stone"}]`, as `/give` writes them); text already in that syntax,
/// or not JSON, is kept. The panel (`panel_items`) reads only this syntax: JSON drew every subtype as its plain item.
pub(crate) fn panel_components(json: &str) -> String {
    use mod_api::serde_json::Value;
    fn snbt(v: &Value, out: &mut String) {
        match v {
            Value::Null => out.push_str("\"\""),
            Value::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
            Value::Number(n) => out.push_str(&n.to_string()),
            Value::String(s) => quoted(s, out),
            Value::Array(a) => {
                out.push('[');
                for (i, x) in a.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    snbt(x, out);
                }
                out.push(']');
            }
            Value::Object(o) => {
                out.push('{');
                for (i, (k, x)) in o.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    key(k, out);
                    out.push(':');
                    snbt(x, out);
                }
                out.push('}');
            }
        }
    }
    fn quoted(s: &str, out: &mut String) {
        out.push('"');
        for c in s.chars() {
            if c == '"' || c == '\\' {
                out.push('\\');
            }
            out.push(c);
        }
        out.push('"');
    }
    fn key(k: &str, out: &mut String) {
        if !k.is_empty() && k.chars().all(|c| c.is_ascii_alphanumeric() || "_-.+".contains(c)) {
            out.push_str(k);
        } else {
            quoted(k, out);
        }
    }
    let t = json.trim();
    if t.is_empty() || t.starts_with('[') {
        return t.to_owned();
    }
    let Ok(Value::Object(o)) = mod_api::serde_json::from_str::<Value>(t) else { return t.to_owned() };
    let mut out = String::from("[");
    for (i, (k, v)) in o.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        out.push_str(k);
        out.push('=');
        snbt(v, &mut out);
    }
    out.push(']');
    out
}

/// A stack of `item` with the components in `json` (the game's JSON form, as `Stack::components` gives it).
pub(crate) fn stack_with(item: mod_api::ItemKey, count: u32, json: &str) -> Option<mod_api::ItemStack> {
    // Nested: the launcher makes every crate-level fn an injection point, and its generated file names a
    // signature's types by their crate path, which a mod's `serde_json` (reached through `mod_api`) has none of.
    fn to_tag(v: &mod_api::serde_json::Value) -> mod_api::Tag {
        use mod_api::serde_json::Value;
        use mod_api::Tag;
        match v {
            Value::Null => Tag::String("".into()),
            Value::Bool(b) => Tag::Byte(i8::from(*b)),
            Value::Number(n) => match n.as_i64() {
                Some(i) => i32::try_from(i).map_or(Tag::Long(i), Tag::Int),
                None => Tag::Double(n.as_f64().unwrap_or(0.0).to_bits()),
            },
            Value::String(s) => Tag::String(s.as_str().into()),
            Value::Array(a) => Tag::List(a.iter().map(to_tag).collect()),
            Value::Object(o) => Tag::Compound(o.iter().map(|(k, v)| (k.as_str().into(), to_tag(v))).collect()),
        }
    }
    let mut stack = mod_api::ItemStack::new(item, count.max(1))?;
    if json.is_empty() {
        return Some(stack);
    }
    let v: mod_api::serde_json::Value = mod_api::serde_json::from_str(json).ok()?;
    if let mod_api::Tag::Compound(c) = to_tag(&v) {
        stack.set_components(c).ok()?;
    }
    Some(stack)
}

/// JEI's Information page for items: lines are translation keys or text.
pub fn add_info(items: &[&str], lines: &[&str]) {
    with(|r| {
        r.info.push((items.iter().map(|s| (*s).to_owned()).collect(), lines.iter().map(|s| (*s).to_owned()).collect()));
        r.version += 1;
    });
}

/// Opens the recipe window on an item's recipes (or its uses) on the next frame, as R (U) does.
pub fn show(item: &str, uses: bool) {
    show_stack(item, "", uses);
}

/// An argument of [`RecipeDef::text_tr`] (or a JSON text's `tr_args`) that is itself translated on the client.
pub fn tr_arg(key: &str) -> String {
    format!("\u{1}{key}")
}

/// The text in JEI's search box now (what the player last typed there; empty before the first search). AE2's
/// terminal reads it for its "Use JEI" and "Sync with JEI search" settings (`IJeiRuntime.getIngredientFilter().
/// getFilterText()`). Client-side.
pub fn search_text() -> String {
    crate::state::with(|j| j.search.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn e(k: u32, c: &str) -> (u32, String) {
        (k, c.to_owned())
    }

    /// Extra stacks go right after their anchor's last entry, in order; no anchor (or none listed) puts them last;
    /// a stack already listed isn't listed again.
    #[test]
    fn extras_follow_their_anchor() {
        let items = vec![e(1, ""), e(2, ""), e(3, "{a}"), e(3, "{b}"), e(4, "")];
        let placed = place_extras(items.clone(), vec![(Some(2), vec![e(9, "{x}"), e(9, "{y}")])]);
        assert_eq!(placed, vec![e(1, ""), e(2, ""), e(9, "{x}"), e(9, "{y}"), e(3, "{a}"), e(3, "{b}"), e(4, "")]);
        // After the last of an item's entries (potions, books).
        let placed = place_extras(items.clone(), vec![(Some(3), vec![e(9, "{x}")])]);
        assert_eq!(placed[4], e(9, "{x}"));
        assert_eq!(placed[5], e(4, ""));
        // An anchor not in the list, or none: at the end.
        assert_eq!(place_extras(items.clone(), vec![(Some(7), vec![e(9, "{x}")])]).last(), Some(&e(9, "{x}")));
        assert_eq!(place_extras(items.clone(), vec![(None, vec![e(9, "{x}")])]).last(), Some(&e(9, "{x}")));
        // Duplicates of the list and of each other are dropped; two groups keep registration order.
        let placed = place_extras(items, vec![(Some(1), vec![e(3, "{a}"), e(8, ""), e(8, "")]), (Some(1), vec![e(7, "")])]);
        assert_eq!(placed, vec![e(1, ""), e(7, ""), e(8, ""), e(2, ""), e(3, "{a}"), e(3, "{b}"), e(4, "")]);
    }

    #[test]
    fn screen_transfer_by_category() {
        let active = vec![("minecraft:crafting".to_owned(), 1u8), ("ae2:x".to_owned(), 2u8)];
        assert_eq!(screen_transfer_for(&active, "minecraft:crafting"), Some(1));
        assert_eq!(screen_transfer_for(&active, "minecraft:smelting"), None);
        assert_eq!(screen_transfer_for::<u8>(&[], "minecraft:crafting"), None);
    }

    #[test]
    fn extra_stack_builder() {
        let s = ExtraStack::new("ae2:facade", "{}").name("Cable Facade - Stone");
        assert_eq!((s.item.as_str(), s.components.as_str(), s.name.as_str()), ("ae2:facade", "{}", "Cable Facade - Stone"));
    }

    /// A facade in an inventory slot (vanilla's item syntax) and on a recipe card (JSON) give the same value.
    #[test]
    fn slot_syntax_compounds_compare_with_json() {
        let card = r#"{"minecraft:custom_data":{"item":"minecraft:stone"},"minecraft:item_model":"ae2:hz_facade_stone"}"#;
        let slot = r#"[minecraft:custom_data={item:"minecraft:stone"},minecraft:item_model="ae2:hz_facade_stone"]"#;
        let a = component_value(card, "minecraft:custom_data");
        assert_eq!(a.as_deref(), Some(r#"{"item":"minecraft:stone"}"#));
        assert_eq!(component_value(slot, "minecraft:custom_data"), a);
        assert_eq!(snbt_json(r"{n:1b,s:'a\'b',l:[1,2L]}").as_deref(), Some(r#"{"l":[1,2],"n":1,"s":"a'b"}"#));
        assert_eq!(snbt_json("plain"), None);
        // The panel's syntax, and back: the same value JEI compares.
        let panel = panel_components(card);
        assert_eq!(panel, r#"[minecraft:custom_data={item:"minecraft:stone"},minecraft:item_model="ae2:hz_facade_stone"]"#);
        assert_eq!(component_value(&panel, "minecraft:custom_data"), a);
        assert_eq!(
            panel_components(r#"{"minecraft:stored_enchantments":{"minecraft:sharpness":5}}"#),
            r#"[minecraft:stored_enchantments={"minecraft:sharpness":5}]"#
        );
        assert_eq!(panel_components(""), "");
        assert_eq!(panel_components("[a=1]"), "[a=1]");
        assert_eq!(panel_components(r#"{"s":"a\"b"}"#), r#"[s="a\"b"]"#);
    }
}
