//! What the recipe window shows: categories (JEI's `IRecipeCategory`) and their recipe cards (`IRecipeLayout`), each
//! card its slots, fluid wells, lines of text and arrow, placed from the card's top left in GUI pixels. Vanilla's
//! categories are made in `vanilla.rs`, the other mods' from their `assets/<ns>/jei/` files in `provider.rs`.

use mod_api::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Role {
    Input,
    Output,
    /// A slot that is neither used up nor made (a fuel shown beside a furnace recipe, a catalyst on the card).
    Other,
}

/// One slot of a card: the stacks it cycles through, a second each.
#[derive(Clone, Debug)]
pub(crate) struct Slot {
    pub at: [f32; 2],
    pub role: Role,
    pub stacks: Vec<(ItemKey, u32)>,
    /// Each stack's components as JSON, by `stacks` index; shorter (or empty) means none.
    pub components: Vec<String>,
    /// The crafting grid cell this slot stands for (`RecipeTransferCheck::missing_cells`' numbering).
    pub cell: Option<u32>,
    /// A chance under 1 is written under the slot (an output that drops sometimes).
    pub chance: Option<f32>,
    /// JEI's large output frame (26x26 around the 16x16 item), as the crafting category draws its result.
    pub big: bool,
}

/// A fluid well: the fluid drawn as its texture (never a bucket), filled to `amount` of `capacity`.
#[derive(Clone, Debug)]
pub(crate) struct Well {
    pub rect: [f32; 4],
    pub role: Role,
    pub fluid: String,
    pub amount: u32,
    pub capacity: u32,
    /// Drawn with no frame or grey fill (`RecipeDef::fluid_bare`), and this sprite part over it.
    pub bare: bool,
    pub overlay: Option<(String, [f32; 4])>,
}

#[derive(Clone, Debug)]
pub(crate) struct Card {
    pub category: usize,
    pub size: [f32; 2],
    pub slots: Vec<Slot>,
    pub wells: Vec<Well>,
    /// Lines of text: where, the text, its colour.
    pub texts: Vec<([f32; 2], String, u32)>,
    pub arrow: Option<[f32; 2]>,
    pub shapeless: bool,
    /// A GUI sprite drawn under the card, and its part (u, v, w, h) and texture size.
    pub background: Option<(String, [f32; 4], [f32; 2])>,
    /// The recipe's id, for "+" (vanilla's crafting grid) and the tooltip on the arrow.
    pub recipe: String,
    /// "+" moves it into the open crafting grid (`RecipesClientHost::transfer_recipe`).
    pub transfer: bool,
    /// Places with a tooltip of their own (`RecipeDef::hover`).
    pub hovers: Vec<([f32; 4], Vec<String>)>,
    /// Sprites over the slots (`RecipeDef::sprite`).
    pub sprites: Vec<(String, [f32; 4], [f32; 2])>,
    /// Plain item icons (`RecipeDef::icon`).
    pub icons: Vec<(ItemKey, [f32; 2])>,
    /// Animated sprite parts (`RecipeDef::animated`).
    pub animated: Vec<(String, [f32; 4], [f32; 2], u32)>,
    /// Lines centred on their x (`RecipeDef::text_centered`).
    pub centered: Vec<([f32; 2], String, u32)>,
    /// No slot frames (`RecipeDef::frameless`).
    pub frameless: bool,
    /// Sprites under the slots (`RecipeDef::sprite_under`).
    pub sprites_under: Vec<(String, [f32; 4], [f32; 2])>,
    /// A bar graph (World Gen's ore heights): its box, the values from the bottom of the world up (bars scaled to
    /// the largest), and the Y each end stands for.
    pub graph: Option<Graph>,
    /// A living entity drawn in a box (Mob Drops' mob, a trade's villager): its type, its saved fields as SNBT
    /// (a villager's profession), the box.
    pub entity: Option<(String, String, [f32; 4])>,
}

#[derive(Clone, Debug)]
pub(crate) struct Graph {
    pub rect: [f32; 4],
    pub values: Vec<f32>,
    pub bottom: i32,
    pub top: i32,
    pub peak: i32,
}

impl Focus {
    /// The item the focus is on, if any.
    pub(crate) fn item(&self) -> Option<ItemKey> {
        match self {
            Focus::Item(k) | Focus::Sub(k, _, _) => Some(*k),
            _ => None,
        }
    }
}

impl Card {
    pub(crate) fn new(category: usize, size: [f32; 2], recipe: &str) -> Self {
        Self {
            category,
            size,
            slots: Vec::new(),
            wells: Vec::new(),
            texts: Vec::new(),
            arrow: None,
            shapeless: false,
            background: None,
            recipe: recipe.to_owned(),
            transfer: false,
            hovers: Vec::new(),
            sprites: Vec::new(),
            icons: Vec::new(),
            animated: Vec::new(),
            centered: Vec::new(),
            frameless: false,
            sprites_under: Vec::new(),
            graph: None,
            entity: None,
        }
    }
    pub(crate) fn slot(&mut self, at: [f32; 2], role: Role, stacks: Vec<(ItemKey, u32)>) -> &mut Slot {
        self.slots.push(Slot { at, role, stacks, components: Vec::new(), cell: None, chance: None, big: false });
        self.slots.last_mut().expect("just pushed")
    }
    pub(crate) fn makes(&self, item: ItemKey) -> bool {
        self.slots.iter().any(|s| s.role == Role::Output && s.stacks.iter().any(|(k, _)| *k == item))
    }
    pub(crate) fn uses(&self, item: ItemKey) -> bool {
        self.slots.iter().any(|s| s.role != Role::Output && s.stacks.iter().any(|(k, _)| *k == item))
    }
    /// A stack of `item` whose `component` has `value` (a slot's stack without components matches any value).
    pub(crate) fn has_sub(&self, uses: bool, item: ItemKey, component: &str, value: &str) -> bool {
        self.slots.iter().filter(|s| (s.role == Role::Output) != uses).any(|s| {
            s.stacks.iter().enumerate().any(|(i, (k, _))| {
                *k == item
                    && match s.components.get(i).filter(|c| !c.is_empty()) {
                        None => true,
                        Some(c) => crate::api::component_value(c, component).as_deref() == Some(value),
                    }
            })
        })
    }
    pub(crate) fn makes_fluid(&self, fluid: &str) -> bool {
        self.wells.iter().any(|w| w.role == Role::Output && w.fluid == fluid)
    }
    pub(crate) fn uses_fluid(&self, fluid: &str) -> bool {
        self.wells.iter().any(|w| w.role != Role::Output && w.fluid == fluid)
    }
}

#[derive(Clone, Debug)]
pub(crate) struct Category {
    pub id: String,
    /// A translation key or the text itself.
    pub title: String,
    pub icon: Option<ItemKey>,
    /// The blocks and items that do this category's recipes (JEI's recipe catalysts): U on one opens the category.
    pub catalysts: Vec<ItemKey>,
}

/// What the window is looking up.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Focus {
    Item(ItemKey),
    /// A stack of an item with a registered subtype (`api::register_subtype`): the item, the component, its value.
    Sub(ItemKey, String, String),
    Fluid(String),
    /// Every recipe of a category (U on a catalyst, a click on a tab's icon).
    Category(usize),
}
