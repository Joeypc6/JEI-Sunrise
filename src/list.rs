//! JEI's item list (`IngredientListOverlay`), drawn by JEI itself beside a container screen: the page row on top,
//! the grid of the search's matches, the search box at the bottom. The grid skips every cell an extra area covers
//! (`extra_areas::areas`: the game's own regions, AE2's upgrade and view cell columns...) and flows the items into
//! the cells left, as JEI's `IngredientGrid` does around `getGuiExtraAreas`; nothing is drawn under another mod's
//! panel.
//!
//! Clicks on an item show its recipes (right: its uses), Shift gives a stack; R, U and A act on the item under the
//! pointer; the wheel over the list turns its page. The search box takes the keyboard while it has focus.

use crate::extra_areas::Grid;
use crate::state::{Jei, Target, inside};
use mod_api::*;

const CELL: f32 = 18.0;
/// The page row's height above the grid, and the search box's below it.
const NAV: f32 = 20.0;
const SEARCH: f32 = 20.0;
const MARGIN: f32 = 4.0;

/// Where the list sits this frame: its whole rect, the grid and its usable cells, the page row's buttons and the search
/// box. None when there is no room beside the screen.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Layout {
    pub rect: [f32; 4],
    pub grid: Grid,
    pub cells: Vec<usize>,
    pub prev: [f32; 4],
    pub next: [f32; 4],
    pub search: [f32; 4],
}

/// The list's layout right of `screen` on a GUI of size `gui`, around `areas`.
pub(crate) fn layout(screen: [f32; 4], gui: [f32; 2], areas: &[[f32; 4]]) -> Option<Layout> {
    let left = screen[0] + screen[2] + MARGIN;
    let width = gui[0] - MARGIN - left;
    let columns = (width / CELL).floor() as usize;
    let top = MARGIN + NAV;
    let bottom = gui[1] - MARGIN - SEARCH - 2.0;
    let rows = ((bottom - top) / CELL).floor() as usize;
    if columns == 0 || rows == 0 {
        return None;
    }
    let grid_w = columns as f32 * CELL;
    let x = (left + (width - grid_w) / 2.0).floor();
    let grid = Grid { origin: [x, top], columns, rows, cell: CELL };
    let cells = grid.open_cells(areas);
    // The page row keeps clear of the areas too: "<" moves right past any that covers it.
    let mut prev_x = x;
    while let Some(a) = areas.iter().find(|&&a| crate::extra_areas::overlaps([prev_x, MARGIN, 13.0, 13.0], a)) {
        prev_x = (a[0] + a[2] + 1.0).ceil();
        if prev_x > x + grid_w - 40.0 {
            prev_x = x;
            break;
        }
    }
    Some(Layout {
        rect: [x, MARGIN, grid_w, gui[1] - 2.0 * MARGIN],
        grid,
        cells,
        prev: [prev_x, MARGIN, 13.0, 13.0],
        next: [x + grid_w - 13.0, MARGIN, 13.0, 13.0],
        search: [x, gui[1] - MARGIN - SEARCH, grid_w, SEARCH],
    })
}

/// The number of pages `count` items take in `per_page` cells (at least one).
pub(crate) fn pages(count: usize, per_page: usize) -> usize {
    if per_page == 0 { 1 } else { count.div_ceil(per_page).max(1) }
}

/// Draws the list for this frame and records its targets (items, buttons) for clicks, keys and tooltips.
/// `areas` are the screen's extra areas (`extra_areas::areas`), asked by the caller before JEI's state was locked.
pub(crate) fn draw(jei: &mut Jei, draw: &mut dyn DrawContext, screen: &ScreenView<'_>, areas: &[[f32; 4]], mouse: [f32; 2]) {
    let Some(l) = layout(screen.rect, draw.size(), areas) else {
        jei.list = None;
        return;
    };
    let per_page = l.cells.len();
    let pages = pages(jei.listed.len(), per_page);
    jei.list_page = jei.list_page.min(pages - 1);
    let page = jei.list_page;
    // The page row: "<", "page/pages", ">".
    for (r, label, target) in [(l.prev, "<", Target::ListPrev), (l.next, ">", Target::ListNext)] {
        crate::window::button(draw, r, inside(r, mouse), pages > 1);
        let tw = draw.text_width(label);
        draw.text(label, [r[0] + ((r[2] - tw) / 2.0).floor() + 0.5, r[1] + 3.0], 0xFFFF_FFFF, true);
        jei.targets.push((r, target));
    }
    let label = format!("{}/{}", page + 1, pages);
    let tw = draw.text_width(&label);
    // Centred between the two buttons.
    let (from, to) = (l.prev[0] + l.prev[2], l.next[0]);
    draw.text(&label, [(from + (to - from - tw) / 2.0).floor(), l.prev[1] + 3.0], 0xFFFF_FFFF, true);
    // The grid: this page's items, each in the next usable cell.
    let start = page * per_page;
    for (slot, (item, components)) in l.cells.iter().zip(jei.listed.iter().skip(start)) {
        let r = l.grid.cell_rect(*slot);
        let at = [r[0] + 1.0, r[1] + 1.0];
        let hovered = inside(r, mouse);
        if components.is_empty() {
            draw.item(*item, at, 16.0);
            jei.targets.push((r, Target::Stack(*item)));
        } else {
            if let Some(stack) = crate::api::stack_with(*item, 1, components) {
                draw.gui_draw_item_with(&stack, at, None);
            }
            jei.targets.push((r, Target::Sub(*item, components.clone())));
        }
        if hovered {
            draw.fill([at[0], at[1], 16.0, 16.0], 0x80FF_FFFF);
        }
    }
    // The search box.
    let s = l.search;
    draw.fill(s, if jei.search_focus { 0xFFFF_FFFF } else { 0xFFA0_A0A0 });
    draw.fill([s[0] + 1.0, s[1] + 1.0, s[2] - 2.0, s[3] - 2.0], 0xFF00_0000);
    let mut text = jei.search.clone();
    while !text.is_empty() && draw.text_width(&text) > s[2] - 10.0 {
        text.remove(0);
    }
    if jei.search_focus && (jei.millis / 300) % 2 == 0 {
        text.push('_');
    }
    draw.text(&text, [s[0] + 4.0, s[1] + 6.0], 0xFFE0_E0E0, true);
    jei.targets.push((s, Target::ListSearch));
    jei.list = Some(l);
}

/// A press at `at`: true when the list took it. Clicks on an item show its recipes (button 1: its uses; Shift:
/// a stack of it, as JEI's cheat mode gives); the page buttons turn the page; the search box takes focus, a right
/// click on it clears it. Any other press takes the focus away.
pub(crate) fn press(jei: &mut Jei, at: [f32; 2], button: i32, shift: bool) -> bool {
    let Some(l) = jei.list.clone() else {
        jei.search_focus = false;
        return false;
    };
    if !inside(l.rect, at) {
        jei.search_focus = false;
        return false;
    }
    let pages = pages(jei.listed.len(), l.cells.len());
    match jei.target_at(at) {
        Some(Target::ListPrev) => {
            jei.list_page = if jei.list_page == 0 { pages - 1 } else { jei.list_page - 1 };
        }
        Some(Target::ListNext) => {
            jei.list_page = (jei.list_page + 1) % pages;
        }
        Some(Target::ListSearch) => {
            jei.search_focus = true;
            if button == 1 && !jei.search.is_empty() {
                jei.set_search(String::new());
            }
            return true;
        }
        Some(Target::Stack(item)) => {
            if shift {
                jei.actions.push(crate::state::Action::Give(item, 64));
            } else {
                let focus = jei.focus_of(item, "");
                jei.open(focus, button == 1);
            }
        }
        Some(Target::Sub(item, components)) => {
            if !shift {
                let focus = jei.focus_of(item, &components);
                jei.open(focus, button == 1);
            }
        }
        _ => {}
    }
    jei.search_focus = false;
    true
}

/// The wheel over the list turns its page (up: the one before). True when it was over the list.
pub(crate) fn scroll(jei: &mut Jei, dy: f64, at: [f32; 2]) -> bool {
    let Some(l) = jei.list.as_ref() else { return false };
    if !inside(l.rect, at) || dy == 0.0 {
        return false;
    }
    let pages = pages(jei.listed.len(), l.cells.len());
    jei.list_page = if dy > 0.0 { (jei.list_page + pages - 1) % pages } else { (jei.list_page + 1) % pages };
    true
}

/// A key while the search box has focus: editing keys change the text, Escape and Enter leave the box. True: taken
/// (every key is, while the box has focus, so a letter never reaches the screen under it).
pub(crate) fn key(jei: &mut Jei, key: &str) -> bool {
    if !jei.search_focus {
        return false;
    }
    match key {
        "key.keyboard.escape" | "key.keyboard.enter" | "key.keyboard.keypad.enter" => jei.search_focus = false,
        "key.keyboard.backspace" => {
            let mut s = jei.search.clone();
            s.pop();
            jei.set_search(s);
        }
        _ => {}
    }
    true
}

/// A typed character while the search box has focus. True: taken.
pub(crate) fn typed(jei: &mut Jei, ch: char) -> bool {
    if !jei.search_focus {
        return false;
    }
    if !ch.is_control() && jei.search.chars().count() < 128 {
        let mut s = jei.search.clone();
        s.push(ch);
        jei.set_search(s);
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The list fills the room right of the screen: whole cells, centred, the page row above and the search box below;
    /// a column an area covers takes no items.
    #[test]
    fn the_grid_fills_the_room_beside_the_screen_and_skips_areas() {
        let screen = [126.0, 20.0, 195.0, 220.0];
        let l = layout(screen, [448.0, 252.0], &[]).unwrap();
        assert_eq!(l.grid.columns, 6);
        assert_eq!(l.grid.rows, 11);
        assert_eq!(l.cells.len(), 66);
        assert!(l.grid.origin[0] >= screen[0] + screen[2]);
        assert_eq!(l.search[1] + l.search[3], 248.0);
        // AE2's view cell column over the grid's first column.
        let column = [l.grid.origin[0] - 2.0, 20.0, 20.0, 100.0];
        let skipped = layout(screen, [448.0, 252.0], &[column]).unwrap();
        assert!(skipped.cells.len() < 66);
        assert!(skipped.cells.iter().all(|&n| !crate::extra_areas::overlaps(skipped.grid.cell_rect(n), column)));
        // A panel reaching the page row pushes "<" right of it.
        let tall = [l.grid.origin[0] - 2.0, 2.0, 20.0, 100.0];
        let pushed = layout(screen, [448.0, 252.0], &[tall]).unwrap();
        assert!(!crate::extra_areas::overlaps(pushed.prev, tall));
        assert!(pushed.prev[0] < pushed.next[0]);
        assert_eq!(layout([0.0, 0.0, 440.0, 252.0], [448.0, 252.0], &[]), None);
    }

    #[test]
    fn pages_round_up() {
        assert_eq!([pages(0, 60), pages(60, 60), pages(61, 60), pages(5, 0)], [1, 1, 2, 1]);
    }
}
