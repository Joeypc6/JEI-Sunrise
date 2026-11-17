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

use mod_api::Host;
use std::sync::Mutex;

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
    pub show: Option<(String, bool)>,
    /// (item, component): stacks of the item told apart by that component's value (`register_subtype`).
    pub subtypes: Vec<(String, String)>,
    /// (menu type, category): "+" on the category's cards moves into that menu (`register_transfer`).
    pub transfers: Vec<(String, String)>,
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
                None => v.to_owned(),
            });
        }
    }
    None
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
    with(|r| r.show = Some((item.to_owned(), uses)));
}

/// An argument of [`RecipeDef::text_tr`] (or a JSON text's `tr_args`) that is itself translated on the client.
pub fn tr_arg(key: &str) -> String {
    format!("\u{1}{key}")
}
