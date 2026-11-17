//! JEI's bookmarks: A over a stack (a screen slot, the panel, the recipe window) bookmarks it or takes it away. The
//! bar sits at the screen's top left, two columns of twelve: left click opens a stack's recipes, right click its
//! uses, Shift-click or the middle button takes it away, and dragging one onto another moves it there. Kept in
//! `config/jei/bookmarks.txt` (the fork's `config_files`), one id a line.

use crate::state::{Jei, Target, inside};
use crate::view::Focus;
use crate::window::slot_frame;
use mod_api::*;

const FILE: &str = "jei/bookmarks.txt";
const ROWS: usize = 12;
const COLUMNS: usize = 2;

pub(crate) fn load(jei: &mut Jei, host: &dyn Host) {
    if jei.bookmarks_loaded {
        return;
    }
    jei.bookmarks_loaded = true;
    let Ok(path) = ConfigPath::new(FILE) else { return };
    if let Ok(bytes) = host.config_files().read(&path) {
        jei.bookmarks = String::from_utf8_lossy(&bytes)
            .lines()
            .filter_map(|l| jei.ids.get(l.trim()).copied())
            .collect();
    }
}

pub(crate) fn save(jei: &Jei, host: &dyn Host) {
    let text: String = jei.bookmarks.iter().map(|k| format!("{}\n", k.id(host))).collect();
    if let Ok(path) = ConfigPath::new(FILE)
        && let Err(e) = host.config_files().write(&path, text.as_bytes())
    {
        eprintln!("[jei] bookmarks not saved: {e:?}");
    }
}

/// The bar's place: left of the screen's window, from the top.
pub(crate) fn rect(jei: &Jei, screen: [f32; 4]) -> Option<[f32; 4]> {
    if jei.bookmarks.is_empty() || !jei.show_bar {
        return None;
    }
    let n = jei.bookmarks.len().min(ROWS * COLUMNS);
    let cols = n.div_ceil(ROWS).min(COLUMNS);
    let rows = n.min(ROWS);
    let w = cols as f32 * 18.0;
    // Left of the window when there is room, else at the screen's edge.
    let x = (screen[0] - w - 6.0).max(2.0);
    Some([x, 4.0, w, rows as f32 * 18.0])
}

pub(crate) fn draw(jei: &mut Jei, _host: &dyn ClientHost, draw: &mut dyn DrawContext, mouse: [f32; 2]) {
    let Some(r) = jei.bar_rect else { return };
    for (i, item) in jei.bookmarks.iter().take(ROWS * COLUMNS).enumerate() {
        let at = [r[0] + (i / ROWS) as f32 * 18.0, r[1] + (i % ROWS) as f32 * 18.0];
        slot_frame(draw, at);
        if jei.dragging == Some(i) {
            continue;
        }
        draw.item(*item, [at[0] + 1.0, at[1] + 1.0], 16.0);
        let cell = [at[0], at[1], 18.0, 18.0];
        if inside(cell, mouse) {
            draw.fill([at[0] + 1.0, at[1] + 1.0, 16.0, 16.0], 0x80FF_FFFF);
        }
        jei.targets.push((cell, Target::Bookmark(i)));
    }
    if let Some(i) = jei.dragging
        && let Some(item) = jei.bookmarks.get(i)
    {
        draw.item(*item, [mouse[0] - 8.0, mouse[1] - 8.0], 16.0);
    }
}

/// A press on the bar: true when it took it.
pub(crate) fn press(jei: &mut Jei, at: [f32; 2], button: i32, shift: bool) -> bool {
    let Some(Target::Bookmark(i)) = jei.target_at(at) else { return false };
    let Some(item) = jei.bookmarks.get(i).copied() else { return false };
    if shift || button == 2 {
        jei.toggle_bookmark(item);
    } else if button == 0 {
        // Left: wait for the release; a drag onto another bookmark moves it there, else it opens the recipes.
        jei.dragging = Some(i);
    } else {
        jei.open(Focus::Item(item), true);
    }
    true
}

pub(crate) fn release(jei: &mut Jei, at: [f32; 2]) -> bool {
    let Some(from) = jei.dragging.take() else { return false };
    let to = match jei.target_at(at) {
        Some(Target::Bookmark(j)) => Some(j),
        _ => None,
    };
    match to {
        Some(j) if j != from => {
            let item = jei.bookmarks.remove(from);
            jei.bookmarks.insert(j.min(jei.bookmarks.len()), item);
            jei.actions.push(crate::state::Action::SaveBookmarks);
        }
        Some(_) => {
            let item = jei.bookmarks[from];
            jei.open(Focus::Item(item), false);
        }
        None => {}
    }
    true
}
