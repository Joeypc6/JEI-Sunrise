//! Extra areas: where a screen draws beyond its window, so JEI's item list keeps clear of it (NeoForge JEI's
//! `IGuiContainerHandler.getGuiExtraAreas` and `IGlobalGuiHandler.getGuiExtraAreas`).
//!
//! # For other mods
//!
//! A screen that draws panels outside its window (a machine's upgrade-card column on its right, a row of config
//! buttons on its left, tabs above it, a popup that folds out) reports them here, and JEI's list leaves the cells
//! under them empty. Nothing is hidden: the items move to the cells left, and a page holds fewer.
//!
//! Name `jei` as an optional dependency (`jei = { version = "^0.5", optional = true }`) and register once, from a
//! client hook, inside `mod_api::if_installed!(host, jei, { ... })`:
//!
//! ```rust,ignore
//! mod_api::if_installed!(host, jei, {
//!     // One screen kind: by its menu type (`ScreenView::menu`).
//!     jei::api::register_gui_extra_areas("mymod:crusher", |_host, screen| {
//!         let [x, y, w, _] = screen.rect;
//!         vec![[x + w + 2.0, y, 32.0, 86.0]] // the upgrade column, 2 pixels right of the window
//!     });
//!     // Any screen at all (vanilla ones you draw onto, screens with no menu): look at `screen` and answer.
//!     jei::api::register_global_extra_areas("mymod", |host, screen| {
//!         if screen.id == "inventory" && screen.menu.is_none() && my_tabs_shown(host) {
//!             vec![[screen.rect[0], screen.rect[1] - 28.0, 140.0, 28.0]] // tabs above the player's inventory
//!         } else {
//!             Vec::new()
//!         }
//!     });
//! });
//! ```
//!
//! - **Coordinates:** `[x, y, width, height]` in GUI pixels, the ones `DrawContext` draws in. `screen.rect` is the
//!   window (vanilla's `leftPos`, `topPos`, `imageWidth`, `imageHeight`).
//! - **Every frame:** the functions are asked each time the list is laid out, so report what is drawn now: a popup
//!   while it is open, nothing once it closes. Keep them cheap (rectangles only, no allocation-heavy work).
//! - **Every screen kind:** a per-menu function is asked for screens whose `menu` is its menu type (a mod's menu
//!   opened for a block, an item or nothing); a global one is asked for every screen, vanilla's included, and tells
//!   them apart by `screen.id` and `screen.menu`.
//! - **Tight areas:** an area larger than what is drawn costs the list cells for nothing. Empty or non-finite
//!   rectangles are dropped.
//! - **The game's own regions** (the recipe book's panel, the creative tabs, the effects beside the inventory) come
//!   from the game itself ([`ClientHost::screen_exclusion`]); a mod reports only what it draws.

use mod_api::*;
use std::sync::Mutex;

/// A function that reports the areas a screen draws beyond its window, `[x, y, width, height]` in GUI pixels.
pub type ExtraAreasFn = fn(&dyn ClientHost, &ScreenView<'_>) -> Vec<[f32; 4]>;

/// The registered functions: per menu type, and for every screen (by owner, for logs).
struct Handlers {
    by_menu: Vec<(String, ExtraAreasFn)>,
    global: Vec<(String, ExtraAreasFn)>,
}

static HANDLERS: Mutex<Handlers> = Mutex::new(Handlers { by_menu: Vec::new(), global: Vec::new() });

/// Reports the extra areas of the screens of menu type `menu` (`ScreenView::menu`, such as `ae2:inscriber`).
/// JEI's `IGuiContainerHandler.getGuiExtraAreas`. Registering the same menu type and function again does nothing.
pub fn register_gui_extra_areas(menu: &str, areas: ExtraAreasFn) {
    let mut h = HANDLERS.lock().unwrap_or_else(|e| e.into_inner());
    if !h.by_menu.iter().any(|(m, f)| m == menu && std::ptr::fn_addr_eq(*f, areas)) {
        h.by_menu.push((menu.to_owned(), areas));
    }
}

/// Reports extra areas on any screen: asked for every screen JEI's list shows beside. JEI's
/// `IGlobalGuiHandler.getGuiExtraAreas`. `owner` is the registering mod's id; registering the same owner and
/// function again does nothing.
pub fn register_global_extra_areas(owner: &str, areas: ExtraAreasFn) {
    let mut h = HANDLERS.lock().unwrap_or_else(|e| e.into_inner());
    if !h.global.iter().any(|(o, f)| o == owner && std::ptr::fn_addr_eq(*f, areas)) {
        h.global.push((owner.to_owned(), areas));
    }
}

/// Every area JEI's list keeps clear of on `screen`: the game's own regions, then each mod's for its menu type,
/// then each global function's. Empty and non-finite rectangles are left out.
pub(crate) fn areas(host: &dyn ClientHost, screen: &ScreenView<'_>) -> Vec<[f32; 4]> {
    let (by_menu, global): (Vec<ExtraAreasFn>, Vec<ExtraAreasFn>) = {
        let h = HANDLERS.lock().unwrap_or_else(|e| e.into_inner());
        let by_menu = screen
            .menu
            .map(|menu| h.by_menu.iter().filter(|(m, _)| m == menu).map(|(_, f)| *f).collect())
            .unwrap_or_default();
        (by_menu, h.global.iter().map(|(_, f)| *f).collect())
    };
    let mut out = host.screen_exclusion();
    for f in by_menu.into_iter().chain(global) {
        out.extend(f(host, screen));
    }
    out.retain(|a| a.iter().all(|v| v.is_finite()) && a[2] > 0.0 && a[3] > 0.0);
    out
}

/// Whether two `[x, y, width, height]` rectangles share any area (touching edges do not count).
pub fn overlaps(a: [f32; 4], b: [f32; 4]) -> bool {
    a[0] < b[0] + b[2] && b[0] < a[0] + a[2] && a[1] < b[1] + b[3] && b[1] < a[1] + a[3]
}

/// A grid of square cells for the item list: its top left corner, its size in cells and the cell's side.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Grid {
    pub origin: [f32; 2],
    pub columns: usize,
    pub rows: usize,
    pub cell: f32,
}

impl Grid {
    /// Cell `n` (in reading order: row by row, left to right) as `[x, y, width, height]`.
    pub fn cell_rect(&self, n: usize) -> [f32; 4] {
        let columns = self.columns.max(1);
        [
            self.origin[0] + (n % columns) as f32 * self.cell,
            self.origin[1] + (n / columns) as f32 * self.cell,
            self.cell,
            self.cell,
        ]
    }

    /// The cells that hold items, in reading order: every cell but those overlapping one of `areas`. A page's `n`th
    /// item goes in the `n`th of them, so an area costs the page its cells and hides no item. Empty when every cell
    /// is covered (the list then has no room on this screen).
    pub fn open_cells(&self, areas: &[[f32; 4]]) -> Vec<usize> {
        (0..self.columns * self.rows)
            .filter(|&n| {
                let rect = self.cell_rect(n);
                !areas.iter().any(|&a| overlaps(rect, a))
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CELL: f32 = 18.0;

    fn grid() -> Grid {
        Grid { origin: [400.0, 30.0], columns: 9, rows: 20, cell: CELL }
    }

    /// No area: every cell, in order.
    #[test]
    fn no_area_keeps_every_cell() {
        let g = grid();
        assert_eq!(g.open_cells(&[]), (0..180).collect::<Vec<_>>());
    }

    /// A column over the grid's first two columns, from its third row, 70 pixels tall: the cells under it are left
    /// out, the third row's items start past it, and no open cell overlaps it.
    #[test]
    fn a_side_column_costs_its_cells_and_hides_nothing() {
        let g = grid();
        let column = [400.0, 30.0 + 2.0 * CELL, 2.0 * CELL - 1.0, 70.0];
        let open = g.open_cells(&[column]);
        let covered_rows = (70.0f32 / CELL).ceil() as usize;
        assert_eq!(open.len(), 180 - 2 * covered_rows);
        // The first two rows are whole; row 2's first open cell is its third.
        assert_eq!(&open[..18], &(0..18).collect::<Vec<_>>()[..]);
        assert_eq!(open[18], 2 * 9 + 2);
        for &n in &open {
            assert!(!overlaps(g.cell_rect(n), column), "cell {n} is under the column");
        }
    }

    /// Touching an edge is not overlapping: a panel that ends where the grid starts costs nothing.
    #[test]
    fn touching_is_not_overlapping() {
        let g = grid();
        assert_eq!(g.open_cells(&[[300.0, 0.0, 100.0, 500.0]]).len(), 180);
    }

    /// An area over the whole grid leaves no cell: the list has no room.
    #[test]
    fn a_covered_grid_has_no_cell() {
        assert!(grid().open_cells(&[[0.0, 0.0, 2000.0, 2000.0]]).is_empty());
    }
}
