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
    /// "+" on a mod's own screen: the mod's function, the recipe's id and Shift.
    ScreenTransfer(crate::api::ScreenTransferFn, String, bool),
    /// A line in chat for the player (`/jei show` that found nothing). // jeiA
    Say(String),
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
    /// The recipe whose alternatives the player is stepping through (the wheel over a slot), if any. // jeiA
    pub hold: Option<Hold>,
}

/// A recipe held still while the player steps through a slot's alternatives (JEI's focus linking): its slots stop
/// cycling, and every slot of it that can hold a picked stack shows that stack. Let go when the pointer leaves the
/// recipe's box. // jeiA
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Hold {
    /// The card (an index into `Jei::cards`).
    pub card: usize,
    /// Its recipe's id, to find the card again when the index is made again. // jeiA
    pub recipe: String,
    /// The time the card's other slots stay at (their cycle stopped there).
    pub since: u64,
    /// The stacks picked (item and components' JSON, empty for none), the latest last: a slot shows the latest one
    /// it holds.
    pub picks: Vec<(ItemKey, String)>,
}

/// A slot drawn last frame: its rect, the card and slot index, the stack index it shows and how many it has. // jeiA
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct SlotAt {
    pub rect: [f32; 4],
    pub card: usize,
    pub slot: usize,
    pub index: usize,
    pub count: usize,
}

/// `/jei show` (or a mod's `show`) that found nothing yet: tried again when the index is made again, given up after
/// a while with a line in chat. // jeiA
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct PendingShow {
    pub item: String,
    pub components: String,
    pub uses: bool,
    /// When it was asked (millis).
    pub since: u64,
    /// The index it last failed on.
    pub built: Option<(u64, u64)>,
}

/// How long a show that found nothing waits for the index to change (the server's recipes and sources can arrive
/// a few seconds into a world). // jeiA
pub(crate) const SHOW_WAIT: u64 = 10_000;

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
    /// "+" on a card: its recipe-book display id when the grid's dry run answers for it (vanilla crafting), its
    /// recipe's id, and the mod's own "+" when a mod screen offers it (`api::register_screen_transfer`).
    Plus(Option<i32>, String, Option<crate::api::ScreenTransferFn>),
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
    /// Extra stacks' search names (`api::ExtraStack::name`), lower-case, by entry.
    pub extra_names: HashMap<(ItemKey, String), String>,
    /// (extra-stack functions, stacks they gave) at the last index, for `/jei debug`.
    pub extra_counts: (usize, usize),
    /// The open mod screen's "+" by category (`api::register_screen_transfer`), this frame.
    pub screen_plus: Vec<(String, crate::api::ScreenTransferFn)>,
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
    /// The recipe slots drawn last frame (the wheel steps through their alternatives). // jeiA
    pub slots_at: Vec<SlotAt>,
    /// A show waiting for the index (`PendingShow`). // jeiA
    pub pending_show: Option<PendingShow>,
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
        (!tabs.is_empty()).then_some(Window { focus, uses, tabs, tab: 0, page: 0, hold: None }) // jeiA: hold
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

    /// The open window and its history after the index was made again (a resource reload, the server's recipes or
    /// sources arriving): each found again for the new cards, on the same category (by id) and page when it is
    /// still there. One whose focus finds nothing now goes. Making the index used to close the window, so a window
    /// opened in the frames before the server's data arrived (`/jei show` right after joining) vanished. // jeiA
    pub(crate) fn reopen_after_rebuild(&mut self, old: &[Category]) {
        let again = |j: &Jei, w: &Window| -> Option<Window> {
            let focus = match &w.focus {
                Focus::Category(c) => {
                    let id = &old.get(*c)?.id;
                    Focus::Category(j.categories.iter().position(|n| n.id == *id)?)
                }
                f => f.clone(),
            };
            let mut n = j.window_for(focus, w.uses)?;
            // The held recipe stays held: its card found again by recipe id. // jeiA
            n.hold = w.hold.as_ref().and_then(|h| {
                let card = j.cards.iter().position(|c| !h.recipe.is_empty() && c.recipe == h.recipe)?;
                Some(Hold { card, ..h.clone() })
            });
            let tab_id = w.tabs.get(w.tab).and_then(|(c, _)| old.get(*c)).map(|c| c.id.as_str());
            if let Some(t) = tab_id.and_then(|id| n.tabs.iter().position(|(c, _)| j.categories[*c].id == id)) {
                n.tab = t;
                n.page = w.page;
            }
            Some(n)
        };
        self.window = self.window.take().and_then(|w| again(self, &w));
        let history = std::mem::take(&mut self.history);
        self.history = history.iter().filter_map(|w| again(self, w)).collect();
    }

    /// Asks for a show (`/jei show`, a mod's `api::show`): it opens now, or waits for the index to be made again
    /// (`try_show`). // jeiA
    pub(crate) fn ask_show(&mut self, item: String, components: String, uses: bool, now: u64) -> Option<String> {
        self.pending_show = Some(PendingShow { item, components, uses, since: now, built: None });
        self.try_show(now)
    }

    /// The waiting show, tried again when the index changed since it last failed (or the first time): opened, kept
    /// waiting, or after `SHOW_WAIT` given up with the line to say. // jeiA
    pub(crate) fn try_show(&mut self, now: u64) -> Option<String> {
        let p = self.pending_show.take()?;
        let late = now.saturating_sub(p.since) >= SHOW_WAIT;
        if p.built.is_some() && p.built == self.built && !late {
            self.pending_show = Some(p);
            return None;
        }
        if let Some(k) = self.ids.get(&p.item).copied() {
            let focus = self.focus_of(k, &p.components);
            if self.open(focus, p.uses) {
                return None;
            }
        }
        if late {
            let what = if p.uses { "uses" } else { "recipes" };
            return Some(if self.ids.contains_key(&p.item) {
                format!("[jei] No {what} for {}", p.item)
            } else {
                format!("[jei] Unknown item {}", p.item)
            });
        }
        self.pending_show = Some(PendingShow { built: self.built, ..p });
        None
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

// jeiA: `/jei show` on a mod's output, and the window kept when the index is made again.
#[cfg(test)]
mod show_tests {
    use super::*;
    use crate::view::Role;

    fn k(n: u32) -> ItemKey {
        mod_api::__private::parts_installation::item_key(n)
    }

    fn cat(id: &str) -> Category {
        Category { id: id.to_owned(), title: id.to_owned(), icon: None, catalysts: Vec::new() }
    }

    /// AE2's inscriber card for the logic processor: printed silicon, redstone and the printed circuit in, the
    /// processor out.
    fn inscriber(category: usize) -> Card {
        let mut c = Card::new(category, [97.0, 64.0], "ae2:inscriber/logic_processor");
        c.slot([1.0, 1.0], Role::Input, vec![(k(11), 1)]);
        c.slot([19.0, 24.0], Role::Input, vec![(k(12), 1)]);
        c.slot([1.0, 47.0], Role::Input, vec![(k(10), 1)]);
        c.slot([69.0, 25.0], Role::Output, vec![(k(20), 1)]);
        c
    }

    fn jei() -> Jei {
        let mut j = Jei::default();
        j.ids.insert("ae2:printed_silicon".into(), k(10));
        j.ids.insert("ae2:printed_logic_processor".into(), k(11));
        j.ids.insert("minecraft:redstone".into(), k(12));
        j.ids.insert("ae2:logic_processor".into(), k(20));
        j.categories = vec![cat("minecraft:crafting"), cat("ae2:inscriber")];
        j.built = Some((1, 1));
        j
    }

    #[test]
    fn show_opens_on_a_mods_output() {
        let mut j = jei();
        j.cards = vec![inscriber(1)];
        assert_eq!(j.ask_show("ae2:logic_processor".into(), String::new(), false, 0), None);
        let w = j.window.clone().expect("the window opens on the processor's recipe");
        assert_eq!(w.tabs, vec![(1, vec![0])]);
        // Its inputs' uses find the same card.
        j.close();
        assert_eq!(j.ask_show("ae2:printed_silicon".into(), String::new(), true, 0), None);
        assert!(j.window.is_some());
    }

    /// A show asked before the server's recipes arrived waits for the index and opens once the card is there; one
    /// that still finds nothing gives up after `SHOW_WAIT` with a line to say.
    #[test]
    fn show_waits_for_the_index() {
        let mut j = jei();
        assert_eq!(j.ask_show("ae2:logic_processor".into(), String::new(), false, 1_000), None);
        assert!(j.window.is_none());
        assert!(j.pending_show.is_some());
        // The same index: not tried again.
        assert_eq!(j.try_show(2_000), None);
        assert!(j.pending_show.is_some());
        // The recipes arrived: the index is made again with the card, and the show opens.
        j.cards = vec![inscriber(1)];
        j.built = Some((1, 2));
        assert_eq!(j.try_show(3_000), None);
        assert!(j.window.is_some());
        assert!(j.pending_show.is_none());

        let mut j = jei();
        assert_eq!(j.ask_show("ae2:logic_processor".into(), String::new(), false, 0), None);
        assert_eq!(j.try_show(SHOW_WAIT), Some("[jei] No recipes for ae2:logic_processor".to_owned()));
        assert!(j.pending_show.is_none());
        assert_eq!(j.ask_show("ae2:nothing".into(), String::new(), false, 0), None);
        assert_eq!(j.try_show(SHOW_WAIT + 1), Some("[jei] Unknown item ae2:nothing".to_owned()));
    }

    /// Making the index again keeps the open window on its category and page, its cards found again.
    #[test]
    fn the_window_stays_through_a_rebuild() {
        let mut j = jei();
        j.cards = vec![inscriber(1), inscriber(1)];
        assert!(j.open(Focus::Item(k(20)), false));
        j.window.as_mut().unwrap().page = 1;
        // The sources arrive: a category is added before the inscriber's and the cards move.
        let old = std::mem::replace(&mut j.categories, vec![cat("minecraft:crafting"), cat("jei:world_gen"), cat("ae2:inscriber")]);
        j.cards = vec![Card::new(1, [10.0, 10.0], "x"), inscriber(2), inscriber(2)];
        j.reopen_after_rebuild(&old);
        let w = j.window.clone().expect("still open");
        assert_eq!(w.tabs, vec![(2, vec![1, 2])]);
        // A held recipe stays held on its card's new index.
        j.window.as_mut().unwrap().hold = Some(Hold { card: 1, recipe: "ae2:inscriber/logic_processor".into(), since: 5, picks: Vec::new() });
        let old2 = j.categories.clone();
        j.cards.insert(0, Card::new(1, [10.0, 10.0], "y"));
        j.reopen_after_rebuild(&old2);
        assert_eq!(j.window.as_ref().and_then(|w| w.hold.as_ref()).map(|h| (h.card, h.since)), Some((2, 5)));
        assert_eq!((w.tab, w.page), (0, 1));
        // A focus that finds nothing in the new index closes.
        let old = std::mem::replace(&mut j.categories, vec![cat("minecraft:crafting")]);
        j.cards = Vec::new();
        j.reopen_after_rebuild(&old);
        assert!(j.window.is_none());
    }
}
