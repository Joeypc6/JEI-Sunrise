//! What the client keeps: the item list and its names, the categories and cards, the open recipe window, the
//! bookmarks and the actions a screen hook queued for the next frame (screen hooks get a read-only host; the frame
//! hook gets the mutable one that `panel_items`, `transfer_recipe` and `give` need).

use crate::view::{Card, Category, Focus};
use mod_api::*;
use std::collections::HashMap;
use std::sync::Mutex;

pub(crate) enum Action {
    /// Show the panel's items again (after a search, a reload, a new world).
    Panel,
    /// "+": the recipe's id, and Shift (as many sets as the items allow).
    Transfer(String, bool),
    /// Ask the server what "+" would do for these display ids.
    /// (the card's display number, its recipe id): the server's dry run is asked by recipe id.
    Check(Vec<(i32, String)>),
    Give(ItemKey, u32),
    SaveBookmarks,
}

#[derive(Clone, Debug)]
pub(crate) struct Window {
    pub focus: Focus,
    /// Uses (U) rather than recipes (R).
    pub uses: bool,
    /// Each tab: its category and its cards' indices.
    pub tabs: Vec<(usize, Vec<usize>)>,
    pub tab: usize,
    pub page: usize,
}

/// A place drawn last frame that the pointer can act on.
#[derive(Clone, Debug)]
pub(crate) enum Target {
    Stack(ItemKey),
    /// A card's stack with components (their JSON).
    Sub(ItemKey, String),
    /// A card's own tooltip (`RecipeDef::hover`).
    Hover(Vec<String>),
    Fluid(String, u32),
    Tab(usize),
    PrevTab,
    NextTab,
    PrevPage,
    NextPage,
    /// "+" on a card: its recipe-book display id when the grid's dry run answers for it (vanilla crafting), and its
    /// recipe's id.
    Plus(Option<i32>, String),
    Bookmark(usize),
    Close,
    Back,
    Title,
}

#[derive(Default)]
pub(crate) struct Jei {
    pub in_world: bool,
    pub ids: HashMap<String, ItemKey>,
    /// Items told apart by a component (`api::register_subtype`): the component.
    pub subtypes: HashMap<ItemKey, String>,
    /// Every entry, in the order the panel lists them: an item, and its components' JSON for a subtype (a potion, an
    /// enchanted book; empty for the plain item). See `subtypes::entries`.
    pub items: Vec<(ItemKey, String)>,
    /// Lower-case names and tooltips for the search, by `items` index, made on the first search.
    pub names: Vec<(String, String)>,
    /// What the panel shows now (the search's matches).
    pub listed: Vec<(ItemKey, String)>,
    pub search: String,
    pub categories: Vec<Category>,
    pub cards: Vec<Card>,
    /// (providers' version, recipes received) the index was made from.
    pub built: Option<(u64, u64)>,
    /// Whether the panel's items came from the creative tabs; until they do, each check looks for the tabs again.
    pub from_tabs: bool,
    pub received: u64,
    pub window: Option<Window>,
    pub history: Vec<Window>,
    pub bookmarks: Vec<ItemKey>,
    pub bookmarks_loaded: bool,
    pub show_bar: bool,
    pub actions: Vec<Action>,
    /// The open screen's mod menu type (`ScreenView::menu`).
    pub menu: Option<String>,
    /// The server's answers for "+": display id -> (result, missing cells).
    pub checks: HashMap<i32, RecipeTransferCheck>,
    /// The targets drawn last frame, front last.
    pub targets: Vec<([f32; 4], Target)>,
    /// A bookmark being dragged: its index.
    pub dragging: Option<usize>,
    /// The window's rect last frame, so a click there never reaches the screen under it.
    pub window_rect: Option<[f32; 4]>,
    /// The bookmark bar's rect last frame.
    pub bar_rect: Option<[f32; 4]>,
    /// The container screen's window rect this frame: the dimming stops at its right, so the item panel there
    /// (and its tooltips, which the game draws before JEI's window) stays bright.
    pub screen_rect: Option<[f32; 4]>,
    /// The time base for cycling a slot's stacks.
    pub millis: u64,
    pub creative: bool,
    /// When the display table was last asked for (millis).
    pub asked_displays: u64,
}

static JEI: Mutex<Option<Jei>> = Mutex::new(None);

pub(crate) fn with<T>(f: impl FnOnce(&mut Jei) -> T) -> T {
    let mut g = JEI.lock().unwrap_or_else(|e| e.into_inner());
    f(g.get_or_insert_with(|| Jei { show_bar: true, ..Jei::default() }))
}

pub(crate) fn inside(r: [f32; 4], at: [f32; 2]) -> bool {
    at[0] >= r[0] && at[0] < r[0] + r[2] && at[1] >= r[1] && at[1] < r[1] + r[3]
}

impl Jei {
    pub(crate) fn target_at(&self, at: [f32; 2]) -> Option<Target> {
        self.targets.iter().rev().find(|(r, _)| inside(*r, at)).map(|(_, t)| t.clone())
    }

    /// The focus a stack gives: its item, or its item and subtype when the item has one registered and the stack
    /// carries that component.
    pub(crate) fn focus_of(&self, item: ItemKey, components: &str) -> Focus {
        if let Some(component) = self.subtypes.get(&item)
            && let Some(value) = crate::api::component_value(components, component)
        {
            return Focus::Sub(item, component.clone(), value);
        }
        Focus::Item(item)
    }

    /// The window for a focus, or None when nothing makes or uses it.
    pub(crate) fn window_for(&self, focus: Focus, uses: bool) -> Option<Window> {
        let mut tabs: Vec<(usize, Vec<usize>)> = Vec::new();
        let mut push = |cat: usize, card: usize| match tabs.iter_mut().find(|(c, _)| *c == cat) {
            Some((_, list)) => list.push(card),
            None => tabs.push((cat, vec![card])),
        };
        match &focus {
            Focus::Category(cat) => {
                for (i, c) in self.cards.iter().enumerate().filter(|(_, c)| c.category == *cat) {
                    push(c.category, i);
                }
            }
            Focus::Item(item) => {
                // U on a station opens its categories whole first (JEI's catalyst lookup).
                if uses {
                    for (ci, cat) in self.categories.iter().enumerate() {
                        if cat.catalysts.contains(item) {
                            for (i, c) in self.cards.iter().enumerate().filter(|(_, c)| c.category == ci) {
                                push(ci, i);
                            }
                        }
                    }
                }
                let mut seen: std::collections::HashSet<usize> = std::collections::HashSet::new();
                if uses {
                    for (ci, cat) in self.categories.iter().enumerate() {
                        if cat.catalysts.contains(item) {
                            seen.extend(self.cards.iter().enumerate().filter(|(_, c)| c.category == ci).map(|(i, _)| i));
                        }
                    }
                }
                for (i, c) in self.cards.iter().enumerate() {
                    let hit = if uses { c.uses(*item) } else { c.makes(*item) };
                    if hit && !seen.contains(&i) {
                        push(c.category, i);
                    }
                }
            }
            Focus::Sub(item, component, value) => {
                // As an item, but only the cards with a stack of the same subtype (JEI's subtype interpreter).
                if uses {
                    for (ci, cat) in self.categories.iter().enumerate() {
                        if cat.catalysts.contains(item) {
                            for (i, c) in self.cards.iter().enumerate().filter(|(_, c)| c.category == ci) {
                                push(ci, i);
                            }
                        }
                    }
                }
                for (i, c) in self.cards.iter().enumerate() {
                    let station = uses && self.categories[c.category].catalysts.contains(item);
                    if !station && c.has_sub(uses, *item, component, value) {
                        push(c.category, i);
                    }
                }
            }
            Focus::Fluid(fluid) => {
                for (i, c) in self.cards.iter().enumerate() {
                    if if uses { c.uses_fluid(fluid) } else { c.makes_fluid(fluid) } {
                        push(c.category, i);
                    }
                }
            }
        }
        // Categories in their own order (vanilla's first), as JEI sorts its tabs.
        // U on a station shows its own categories first, as JEI's catalyst lookup does.
        let station = |cat: usize| match &focus {
            Focus::Item(item) | Focus::Sub(item, _, _) if uses => !self.categories[cat].catalysts.contains(item),
            _ => true,
        };
        tabs.sort_by_key(|(c, _)| (station(*c), *c));
        (!tabs.is_empty()).then_some(Window { focus, uses, tabs, tab: 0, page: 0 })
    }

    /// Opens the window on a focus; the one open now goes on the history (Backspace).
    pub(crate) fn open(&mut self, focus: Focus, uses: bool) -> bool {
        let Some(w) = self.window_for(focus, uses) else { return false };
        if let Some(old) = self.window.take() {
            self.history.push(old);
            if self.history.len() > 64 {
                self.history.remove(0);
            }
        }
        self.window = Some(w);
        true
    }

    pub(crate) fn close(&mut self) {
        self.window = None;
        self.history.clear();
    }

    pub(crate) fn toggle_bookmark(&mut self, item: ItemKey) {
        match self.bookmarks.iter().position(|b| *b == item) {
            Some(i) => {
                self.bookmarks.remove(i);
            }
            None => self.bookmarks.push(item),
        }
        self.actions.push(Action::SaveBookmarks);
    }
}
