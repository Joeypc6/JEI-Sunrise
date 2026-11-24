//! JEI's recipe window (`RecipesGui`) over the open screen: a row of category tabs, the category's title and page
//! arrows, its catalysts down the left side, and as many recipe cards as fit, each with its "+" where it can go into
//! the open crafting grid. Drawn in `screen_render_post`, which draws over any screen; its input comes in the
//! `screen_*_pre` hooks, which it cancels so nothing reaches the screen under it.

use crate::state::{Action, Hold, Jei, SlotAt, Target, Window, inside};
use crate::view::{Card, Focus, Role};
use crate::{bookmarks, displays, tr};
use mod_api::*;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

/// The open window's rectangle this frame, for `render_tooltip_pre`: a tooltip the screen behind hangs from a slot
/// under the window would draw under it, so it is not drawn (as JEI's own recipe screen leaves the inventory
/// unhovered). Kept apart from `Jei`, whose lock is held while the window draws.
pub(crate) static OPEN_RECT: Mutex<Option<[f32; 4]>> = Mutex::new(None);
/// Set while the window draws its own tooltip, which the hook must let through.
pub(crate) static OWN_TOOLTIP: AtomicBool = AtomicBool::new(false);

const SLOT: f32 = 18.0;
/// A recipe's box: its padding round the card, and the room between two boxes.
const PAD: f32 = 5.0;
const GAP: f32 = 2.0 * PAD + 6.0;
const TAB: f32 = 24.0;
const BORDER: u32 = 0xFF00_0000;
const FACE: u32 = 0xFFC6_C6C6;
const LIGHT: u32 = 0xFFFF_FFFF;
const DARK: u32 = 0xFF55_5555;
const TEXT: u32 = 0xFF40_4040;

/// A vanilla-style panel: black outline, light top-left and dark bottom-right bevel.
pub(crate) fn panel(draw: &mut dyn DrawContext, r: [f32; 4]) {
    let [x, y, w, h] = r;
    draw.fill([x + 1.0, y, w - 2.0, h], BORDER);
    draw.fill([x, y + 1.0, w, h - 2.0], BORDER);
    draw.fill([x + 1.0, y + 1.0, w - 2.0, h - 2.0], FACE);
    draw.fill([x + 1.0, y + 1.0, w - 3.0, 2.0], LIGHT);
    draw.fill([x + 1.0, y + 1.0, 2.0, h - 3.0], LIGHT);
    draw.fill([x + 3.0, y + h - 3.0, w - 4.0, 2.0], DARK);
    draw.fill([x + w - 3.0, y + 3.0, 2.0, h - 4.0], DARK);
    draw.fill([x + 3.0, y + 3.0, w - 6.0, h - 6.0], FACE);
}

pub(crate) fn slot_frame(draw: &mut dyn DrawContext, at: [f32; 2]) {
    if !draw.sprite("sunrise/jei/slot", [0.0, 0.0, 18.0, 18.0], [at[0], at[1], 18.0, 18.0], None) {
        draw.fill([at[0], at[1], 18.0, 18.0], 0xFF37_3737);
        draw.fill([at[0] + 1.0, at[1] + 1.0, 17.0, 17.0], 0xFFFF_FFFF);
        draw.fill([at[0] + 1.0, at[1] + 1.0, 16.0, 16.0], 0xFF8B_8B8B);
    }
}

/// World Gen's graph: one bar per block of height, the world's bottom on the left, scaled to the fullest layer;
/// the pointer over it says the Y and the share there.
fn draw_graph(draw: &mut dyn DrawContext, g: &crate::view::Graph, at: [f32; 2], mouse: [f32; 2], targets: &mut Vec<([f32; 4], Target)>) {
    // Just Enough Resources' graph: red bars over the card, an axis on the left and the bottom, Y labels under it.
    let [gx, gy, gw, gh] = g.rect;
    let (gx, gy) = (at[0] + gx, at[1] + gy);
    let most = g.values.iter().copied().fold(0.0f32, f32::max);
    draw.fill([gx, gy, 1.0, gh], 0xFF40_4040);
    draw.fill([gx, gy + gh, gw, 1.0], 0xFF40_4040);
    if most <= 0.0 || g.values.is_empty() {
        return;
    }
    let n = g.values.len() as f32;
    let cols = (gw - 2.0).floor().max(1.0) as usize;
    for c in 0..cols {
        // Each column takes the fullest layer it covers.
        let from = (c as f32 / cols as f32 * n) as usize;
        let to = (((c + 1) as f32 / cols as f32 * n) as usize).max(from + 1).min(g.values.len());
        let v = g.values[from..to].iter().copied().fold(0.0f32, f32::max);
        let h = (v / most * (gh - 2.0)).round();
        if h > 0.0 {
            draw.fill([gx + 2.0 + c as f32, gy + gh - h, 1.0, h], 0xFFE0_1010);
        }
    }
    // Y under the axis, at a few even steps.
    let span = (g.top - g.bottom).max(1) as f32;
    let steps = 4;
    for i in 0..=steps {
        let t = i as f32 / steps as f32;
        let yv = g.bottom + (t * span).round() as i32;
        let label = format!("{yv}");
        let lw = draw.text_width(&label);
        let lx = (gx + 2.0 + t * (gw - 2.0) - lw / 2.0).clamp(gx, gx + gw - lw);
        draw.fill([gx + 2.0 + t * (gw - 3.0), gy + gh + 1.0, 1.0, 2.0], 0xFF40_4040);
        draw.text(&label, [lx, gy + gh + 4.0], TEXT, false);
    }
    if inside([gx, gy, gw, gh], mouse) {
        let i = ((((mouse[0] - gx - 2.0) / (gw - 2.0)) * n) as usize).min(g.values.len() - 1);
        targets.push(([gx, gy, gw, gh], Target::Hover(vec![format!("Y {}", g.bottom + i as i32), format!("{:.3}% of blocks", g.values[i])])));
    }
}

/// The grey strip behind the category's title and the page count (JEI's title bars).
fn bar(draw: &mut dyn DrawContext, r: [f32; 4]) {
    let [x, y, w, h] = r;
    draw.fill([x, y, w, h], 0xFF37_3737);
    draw.fill([x + 1.0, y + 1.0, w - 2.0, h - 2.0], 0xFF8B_8B8B);
}

/// A recipe's box: a light inset frame round the card.
fn card_box(draw: &mut dyn DrawContext, r: [f32; 4]) {
    let [x, y, w, h] = r;
    draw.fill([x, y, w, h], 0xFF9E_9E9E);
    draw.fill([x + 1.0, y + 1.0, w - 1.0, h - 1.0], 0xFFE8_E8E8);
    draw.fill([x + 1.0, y + 1.0, w - 2.0, h - 2.0], 0xFFC6_C6C6);
}

/// JEI's shapeless icon: two crossed arrows, 9x7.
fn shapeless_icon(draw: &mut dyn DrawContext, at: [f32; 2]) {
    let c = 0xFF70_7070;
    for i in 0..7 {
        let f = i as f32;
        draw.fill([at[0] + f, at[1] + f, 1.0, 1.0], c);
        draw.fill([at[0] + f, at[1] + 6.0 - f, 1.0, 1.0], c);
    }
    // The arrow heads on the right.
    for (dx, dy) in [(5.0, 0.0), (6.0, 0.0), (6.0, 1.0), (5.0, 6.0), (6.0, 6.0), (6.0, 5.0)] {
        draw.fill([at[0] + dx, at[1] + dy, 1.0, 1.0], c);
    }
}

/// JEI's recipe arrow, 22x15: a flat grey shaft and head, as the crafting table's own arrow. Drawn as fills: the
/// furnace's progress sprite, tinted, filled its transparent box too (a dark square behind the arrow).
fn recipe_arrow(draw: &mut dyn DrawContext, at: [f32; 2]) {
    let c = 0xFF8B_8B8B;
    draw.fill([at[0], at[1] + 5.0, 14.0, 5.0], c);
    for k in 0..8 {
        let f = k as f32;
        draw.fill([at[0] + 14.0 + f, at[1] + f, 1.0, 15.0 - 2.0 * f], c);
    }
}

/// JEI's large output slot: a 26x26 frame (the crafting result's).
fn big_frame(draw: &mut dyn DrawContext, at: [f32; 2]) {
    draw.fill([at[0], at[1], 26.0, 26.0], 0xFF37_3737);
    draw.fill([at[0] + 1.0, at[1] + 1.0, 25.0, 25.0], 0xFFFF_FFFF);
    draw.fill([at[0] + 1.0, at[1] + 1.0, 24.0, 24.0], 0xFF8B_8B8B);
}

pub(crate) fn button(draw: &mut dyn DrawContext, r: [f32; 4], hover: bool, enabled: bool) {
    // JEI's button: a dark outline, a light bevel, a grey face (blue-tinted under the pointer).
    let [x, y, w, h] = r;
    draw.fill([x, y, w, h], 0xFF00_0000);
    let (face, light, dark) = if !enabled {
        (0xFF6C_6C6C, 0xFF80_8080, 0xFF50_5050)
    } else if hover {
        (0xFF8C_96D8, 0xFFBF_C8FF, 0xFF5A_639E)
    } else {
        (0xFF9C_9C9C, 0xFFE0_E0E0, 0xFF58_5858)
    };
    draw.fill([x + 1.0, y + 1.0, w - 2.0, h - 2.0], dark);
    draw.fill([x + 1.0, y + 1.0, w - 3.0, h - 3.0], light);
    draw.fill([x + 2.0, y + 2.0, w - 4.0, h - 4.0], face);
}

#[allow(dead_code)]
fn button_sprite(draw: &mut dyn DrawContext, r: [f32; 4], hover: bool, enabled: bool) {
    let name = if !enabled { "sunrise/jei/button_disabled_v2" } else if hover { "sunrise/jei/button_highlight_v2" } else { "sunrise/jei/button_enabled_v2" };
    draw.gui_nine_slice(name, r, 0xFFFF_FFFF);
}

fn arrow_icon(draw: &mut dyn DrawContext, at: [f32; 2], next: bool) {
    let name = if next { "sunrise/jei/icons/arrow_next_v2" } else { "sunrise/jei/icons/arrow_previous_v2" };
    if !draw.sprite(name, [0.0, 0.0, 9.0, 7.0], [at[0], at[1], 9.0, 7.0], None) {
        draw.text(if next { ">" } else { "<" }, at, 0xFFFF_FFFF, true);
    }
}

// jeiA: a slot's stack is chosen by `shown_index` (its cycle, once a second as JEI's, or its recipe's pick).

/// A slot's stack `i` with its components' JSON (empty for none). // jeiA
fn stack_at<'a>(stacks: &[(ItemKey, u32)], components: &'a [String], i: usize) -> (ItemKey, &'a str) {
    (stacks[i].0, components.get(i).map(String::as_str).unwrap_or(""))
}

/// The index of the stack a slot shows: the latest pick of its recipe it holds (so slots holding the same tag show
/// the same choice, as JEI's focus links them), else its cycle at `millis`. // jeiA
pub(crate) fn shown_index(stacks: &[(ItemKey, u32)], components: &[String], millis: u64, picks: &[(ItemKey, String)]) -> Option<usize> {
    if stacks.is_empty() {
        return None;
    }
    for (k, c) in picks.iter().rev() {
        if let Some(i) = (0..stacks.len()).find(|i| stack_at(stacks, components, *i) == (*k, c.as_str())) {
            return Some(i);
        }
    }
    Some((millis / 1000) as usize % stacks.len())
}

/// Steps a slot `by` places from the stack it shows (`shown`), and picks that stack for its recipe: the picks this
/// slot could show give way, so the new one is shown here and in every slot that can hold it. // jeiA
pub(crate) fn step(picks: &mut Vec<(ItemKey, String)>, stacks: &[(ItemKey, u32)], components: &[String], shown: usize, by: i32) {
    let n = stacks.len() as i32;
    if n < 2 {
        return;
    }
    let next = (shown as i32 + by).rem_euclid(n) as usize;
    let (item, json) = stack_at(stacks, components, next);
    let pick = (item, json.to_owned());
    picks.retain(|(k, c)| !(0..stacks.len()).any(|i| stack_at(stacks, components, i) == (*k, c.as_str())));
    picks.push(pick);
}

fn text_of(host: &dyn ClientHost, raw: &str) -> String {
    if let Some(rest) = raw.strip_prefix('\u{2}') {
        let mut parts = rest.split('\u{3}');
        let key = parts.next().unwrap_or("");
        let args: Vec<&str> = parts.filter(|a| !a.is_empty()).collect();
        let mut text = host.translate(key).unwrap_or_else(|| key.to_owned());
        for (i, a) in args.iter().enumerate() {
            // An argument marked `\u{1}` is a translation key of its own (text_tr / tr_args).
            let a = match a.strip_prefix('\u{1}') {
                Some(k) => host.translate(k).unwrap_or_else(|| k.to_owned()),
                None => (*a).to_owned(),
            };
            // Vanilla's positional `%1$s` as well as `%s`.
            let positional = format!("%{}$s", i + 1);
            if text.contains(&positional) {
                text = text.replace(&positional, &a);
            } else {
                text = text.replacen("%s", &a, 1);
            }
        }
        text
    } else if let Some(key) = raw.strip_prefix('\u{1}') {
        host.translate(key).unwrap_or_else(|| key.to_owned())
    } else {
        raw.to_owned()
    }
}

/// Words wrapped to a width.
pub(crate) fn wrap(draw: &dyn DrawContext, text: &str, width: f32) -> Vec<String> {
    let mut lines = Vec::new();
    for para in text.split('\n') {
        let mut line = String::new();
        for word in para.split_whitespace() {
            let candidate = if line.is_empty() { word.to_owned() } else { format!("{line} {word}") };
            if draw.text_width(&candidate) > width && !line.is_empty() {
                lines.push(std::mem::replace(&mut line, word.to_owned()));
            } else {
                line = candidate;
            }
        }
        lines.push(line);
    }
    lines
}

fn card_height(card: &Card, draw: &dyn DrawContext, host: &dyn ClientHost) -> f32 {
    let mut h = card.size[1];
    // Information cards grow with their text.
    let info: Vec<&str> = card.texts.iter().filter(|t| t.1.starts_with('\u{1}')).map(|t| t.1.as_str()).collect();
    if !info.is_empty() {
        let lines: usize = info.iter().map(|t| wrap(draw, &text_of(host, t), card.size[0]).len()).sum();
        h = 24.0 + lines as f32 * 10.0;
    }
    h
}

/// The recipe window over the screen. The caller clears `jei.targets` first (the item list adds its own before this).
pub(crate) fn draw(jei: &mut Jei, host: &dyn ClientHost, draw: &mut dyn DrawContext, mouse: [f32; 2]) {
    jei.window_rect = None;
    *OPEN_RECT.lock().unwrap_or_else(|e| e.into_inner()) = None;
    bookmarks::draw(jei, host, draw, mouse);
    let Some(w) = jei.window.clone() else { return };
    let [sw, sh] = draw.size();
    let widest = w.tabs.iter().flat_map(|(_, cards)| cards.iter()).map(|i| jei.cards[*i].size[0]).fold(160.0, f32::max);
    let width = (widest + 40.0).max(196.0).min(sw - 50.0);
    // The tabs sit above the window: keep them on screen.
    let most = (sh - TAB - 12.0).min(260.0);
    // The window is as tall as its fullest page (the cards from 40 below its top, 8 above its bottom), up to
    // `most`, so one small card doesn't sit in a big empty window.
    let (cat, cards) = &w.tabs[w.tab.min(w.tabs.len() - 1)];
    let heights: Vec<f32> = cards.iter().map(|i| card_height(&jei.cards[*i], draw, host)).collect();
    let room = most - 8.0 - 40.0;
    let mut pages: Vec<usize> = vec![0];
    let mut used = 0.0f32;
    let mut fullest = 0.0f32;
    for (i, h) in heights.iter().enumerate() {
        if used > 0.0 && used + h > room {
            pages.push(i);
            used = 0.0;
        }
        used += h + GAP;
        fullest = fullest.max(used - 8.0);
    }
    let mut height = (40.0 + fullest + 8.0).clamp(110.0, most.max(110.0));
    let mut width = width;
    let (mut x0, mut y0) = (((sw - width) / 2.0).floor(), (TAB + 4.0 + (sh - TAB - 8.0 - height) / 2.0).floor());
    // JEI's recipe screen takes the container's place: the window covers the container (the screen under it stays
    // drawn, so the game's item panel beside it stays live), centred on it.
    if let Some([sx, sy, sw_, sh_]) = jei.screen_rect {
        width = width.max(sw_ + 8.0).min(sw - 50.0);
        height = height.max(sh_ + 8.0).min(sh - TAB - 8.0);
        x0 = (sx + sw_ / 2.0 - width / 2.0).floor().max(26.0);
        y0 = (sy + sh_ / 2.0 - height / 2.0).floor().max(TAB + 4.0);
    }
    let r = [x0, y0, width, height];
    jei.window_rect = Some([x0 - 26.0, y0 - TAB, width + 26.0, height + TAB]);
    *OPEN_RECT.lock().unwrap_or_else(|e| e.into_inner()) = jei.window_rect;
    // The window covers the container; the game's background dim and `screen_render_background`'s strip dim the
    // world behind it, and the item panel stays lit beside it.
    // Tabs above the window, as many as fit; the selected one joins the window.
    let per = (((width - 8.0) / TAB).floor() as usize).max(1);
    let first = (w.tab / per) * per;
    for (n, (cat, _)) in w.tabs.iter().enumerate().skip(first).take(per) {
        let tr_ = [x0 + 4.0 + (n - first) as f32 * TAB, y0 - TAB + 3.0, TAB - 1.0, TAB];
        let sel = n == w.tab;
        panel(draw, [tr_[0], tr_[1] + if sel { 0.0 } else { 3.0 }, tr_[2], tr_[3]]);
        if let Some(icon) = jei.categories[*cat].icon {
            draw.item(icon, [tr_[0] + 3.5, tr_[1] + if sel { 4.0 } else { 6.0 }], 16.0);
        }
        jei.targets.push((tr_, Target::Tab(n)));
    }
    panel(draw, r);
    // Join the selected tab to the window.
    if w.tab >= first && w.tab < first + per {
        let tx = x0 + 4.0 + (w.tab - first) as f32 * TAB;
        draw.fill([tx + 1.0, y0, TAB - 3.0, 4.0], FACE);
    }
    let category = &jei.categories[*cat];
    // Title row: category arrows and its name.
    let row = y0 + 6.0;
    for (rect, next, target) in [
        ([x0 + 6.0, row, 13.0, 13.0], false, Target::PrevTab),
        ([x0 + width - 19.0, row, 13.0, 13.0], true, Target::NextTab),
    ] {
        button(draw, rect, inside(rect, mouse), w.tabs.len() > 1);
        arrow_icon(draw, [rect[0] + 2.0, rect[1] + 3.0], next);
        jei.targets.push((rect, target));
    }
    bar(draw, [x0 + 20.0, row, width - 40.0, 13.0]);
    let title = tr(host, &category.title, &category.title);
    let tw = draw.text_width(&title);
    draw.text(&title, [x0 + (width - tw) / 2.0, row + 3.0], 0xFFFF_FFFF, true);
    jei.targets.push(([x0 + (width - tw) / 2.0, row, tw, 12.0], Target::Title));
    // Cards that fit, then the page row.
    let top = row + 34.0 + PAD;
    let page = w.page.min(pages.len() - 1);
    if let Some(win) = jei.window.as_mut() {
        win.page = page;
    }
    let page_row = row + 16.0;
    for (rect, next, target) in [
        ([x0 + 6.0, page_row, 13.0, 13.0], false, Target::PrevPage),
        ([x0 + width - 19.0, page_row, 13.0, 13.0], true, Target::NextPage),
    ] {
        button(draw, rect, inside(rect, mouse), pages.len() > 1);
        arrow_icon(draw, [rect[0] + 2.0, rect[1] + 3.0], next);
        jei.targets.push((rect, target));
    }
    bar(draw, [x0 + 20.0, page_row, width - 40.0, 13.0]);
    let label = format!("{}/{}", page + 1, pages.len());
    let lw = draw.text_width(&label);
    draw.text(&label, [x0 + (width - lw) / 2.0, page_row + 3.0], 0xFFFF_FFFF, true);
    // Catalysts down the left side (JEI's `RecipeCatalysts`).
    if !category.catalysts.is_empty() {
        let n = category.catalysts.len().min(((height - 8.0) / SLOT) as usize);
        panel(draw, [x0 - 25.0, y0 + 4.0, 26.0, n as f32 * SLOT + 8.0]);
        for (i, c) in category.catalysts.iter().take(n).enumerate() {
            let at = [x0 - 21.0, y0 + 8.0 + i as f32 * SLOT];
            slot_frame(draw, at);
            draw.item(*c, [at[0] + 1.0, at[1] + 1.0], 16.0);
            jei.targets.push(([at[0], at[1], SLOT, SLOT], Target::Stack(*c)));
        }
    }
    let start = pages[page];
    let end = pages.get(page + 1).copied().unwrap_or(cards.len());
    let mut y = top;
    let mut checks = Vec::new();
    jei.slots_at.clear();
    // jeiA: each recipe's box with its "+" beside it: the pointer leaving the held recipe's lets its slots cycle again.
    let mut areas: Vec<(usize, [f32; 4])> = Vec::new();
    for (k, ci) in cards[start..end].iter().enumerate() {
        let card = jei.cards[*ci].clone();
        let h = heights[start + k];
        let x = x0 + ((width - card.size[0]) / 2.0).floor();
        // Each recipe in a box of its own, as JEI draws them.
        card_box(draw, [x - PAD, y - PAD, card.size[0] + 2.0 * PAD, h + 2.0 * PAD]);
        areas.push((*ci, [x - PAD, y - PAD, card.size[0] + 2.0 * PAD + 16.0, h + 2.0 * PAD]));
        draw_card(jei, host, draw, &card, *ci, [x, y], h, mouse, &mut checks);
        y += h + GAP;
    }
    if let Some(win) = jei.window.as_mut()
        && let Some(hold) = &win.hold
        && !areas.iter().any(|(ci, r)| *ci == hold.card && inside(*r, mouse))
    {
        win.hold = None;
    }
    if !checks.is_empty() {
        jei.actions.push(Action::Check(checks));
    }
    // Tooltips last, over everything.
    tooltip(jei, host, draw, mouse);
}

#[allow(clippy::too_many_arguments)]
fn draw_card(jei: &mut Jei, host: &dyn ClientHost, draw: &mut dyn DrawContext, card: &Card, ci: usize, at: [f32; 2], h: f32, mouse: [f32; 2], checks: &mut Vec<(i32, String)>) {
    let [x, y] = at;
    // jeiA: a held recipe's slots stay at the time it was held and show its picks.
    let (time, picks) = match jei.window.as_ref().and_then(|w| w.hold.as_ref()).filter(|h| h.card == ci) {
        Some(h) => (h.since, h.picks.clone()),
        None => (jei.millis, Vec::new()),
    };
    if let Some((sprite, uv, tex)) = &card.background {
        draw.sprite(sprite, *uv, [x, y, uv[2], uv[3]], None);
        let _ = tex;
    }
    for (sprite, uv, p) in &card.sprites_under {
        draw.sprite(sprite, *uv, [x + p[0], y + p[1], uv[2], uv[3]], None);
    }
    // "+": the server's dry run says what it would do; missing inputs are marked red.
    let mut missing: Vec<u32> = Vec::new();
    let mut plus = None;
    // A mod's own screen offers "+" for this category (`api::register_screen_transfer`): the mod answers the click.
    let screen_plus = crate::api::screen_transfer_for(&jei.screen_plus, &jei.categories[card.category].id);
    if screen_plus.is_some() {
        plus = Some((None, RecipeTransferResult::Moves));
    } else if card.transfer
        && let Some(display) = displays::of(&card.recipe)
    {
        checks.push((display, card.recipe.clone()));
        if let Some(check) = jei.checks.get(&display) {
            if check.result == RecipeTransferResult::Missing {
                missing = check.missing_cells.clone();
            }
            plus = Some((Some(display), check.result));
        }
    } else if let Some(menu) = jei.menu.clone().filter(|menu| crate::api::transfers_into(menu, &jei.categories[card.category].id)) {
        // A mod moves this category into the open menu (`menu_transfer`). The server's part answers the click; the
        // mod's client-side dry run (`api::register_transfer_state`), if it has one, marks the cells and the tooltip.
        let _ = menu;
        plus = Some((None, RecipeTransferResult::Moves));
    }
    // The open mod menu's own dry run of "+" (`api::register_transfer_state`), whichever way "+" came: a mod menu's
    // category, or the game's transfer into a mod menu that takes crafting recipes (AE2's crafting terminal).
    jei.plus_states.remove(&card.recipe);
    if plus.is_some()
        && screen_plus.is_none()
        && let Some(menu) = jei.menu.clone()
        && let Some(state_fn) = crate::api::transfer_state_for(&menu)
    {
        let category = jei.categories[card.category].id.clone();
        let cells: Vec<crate::api::TransferCell> = card
            .slots
            .iter()
            .filter(|s| s.role == Role::Input && !s.stacks.is_empty())
            .filter_map(|s| Some(crate::api::TransferCell { cell: s.cell?, items: s.stacks.iter().map(|(k, _)| *k).collect() }))
            .collect();
        if let Some(state) = state_fn(host, &menu, &category, &cells) {
            jei.plus_states.insert(card.recipe.clone(), state);
        }
    }
    let marks: Vec<(u32, u32)> = jei.plus_states.get(&card.recipe).map(|s| s.marks.clone()).unwrap_or_default();
    // JEI marks the missing ingredients only while the pointer is on "+".
    let plus_rect = [x + card.size[0] + PAD + 3.0, y + h + PAD - 13.0, 13.0, 13.0];
    let on_plus = plus.is_some() && inside(plus_rect, mouse);
    if !on_plus {
        missing.clear();
    }
    for (si, s) in card.slots.iter().enumerate() {
        let sx = [x + s.at[0] - 1.0, y + s.at[1] - 1.0];
        if s.big {
            big_frame(draw, [sx[0] - 4.0, sx[1] - 4.0]);
        } else if !card.frameless {
            slot_frame(draw, sx);
        }
        if s.cell.is_some_and(|c| missing.contains(&c)) && !s.stacks.is_empty() {
            draw.fill([sx[0] + 1.0, sx[1] + 1.0, 16.0, 16.0], 0x80FF_0000);
        }
        if on_plus
            && !s.stacks.is_empty()
            && let Some(&(_, colour)) = s.cell.and_then(|c| marks.iter().find(|(m, _)| *m == c))
        {
            draw.fill([sx[0] + 1.0, sx[1] + 1.0, 16.0, 16.0], colour);
        }
        // jeiA: the stack shown is the recipe's pick when it holds one, else the cycle's; the tooltip, R/U and "+"
        // all take that one.
        let index = shown_index(&s.stacks, &s.components, time, &picks);
        let json = index.and_then(|i| s.components.get(i)).filter(|c| !c.is_empty()).cloned();
        let now = index.map(|i| s.stacks[i]);
        if let Some(i) = index {
            jei.slots_at.push(SlotAt { rect: [sx[0], sx[1], SLOT, SLOT], card: ci, slot: si, index: i, count: s.stacks.len() });
        }
        if let (Some((item, count)), Some(json)) = (now, json) {
            // A stack with components (a subtype): drawn as itself, and its focus is its subtype.
            if let Some(stack) = crate::api::stack_with(item, count, &json) {
                draw.gui_draw_item_with(&stack, [sx[0] + 1.0, sx[1] + 1.0], None);
            }
            jei.targets.push(([sx[0], sx[1], SLOT, SLOT], Target::Sub(item, json)));
        } else if let Some((item, count)) = now {
            match ItemStack::new(item, count.max(1)) {
                Some(stack) if count > 1 => draw.gui_draw_item_with(&stack, [sx[0] + 1.0, sx[1] + 1.0], None),
                _ => {
                    draw.item(item, [sx[0] + 1.0, sx[1] + 1.0], 16.0);
                }
            }
            jei.targets.push(([sx[0], sx[1], SLOT, SLOT], Target::Stack(item)));
        }
        if let Some(c) = s.chance.filter(|c| *c < 1.0) {
            // Short, as JER writes them: whole percents from 10 up, one decimal under.
            let pct = c * 100.0;
            let t = if pct >= 9.95 { format!("{pct:.0}%") } else if pct >= 0.95 { format!("{pct:.1}%") } else if pct >= 0.1 { format!("{pct:.1}%").trim_start_matches('0').to_owned() } else { "<.1%".to_owned() };
            let tw = draw.text_width(&t);
            draw.text(&t, [sx[0] + ((18.0 - tw) / 2.0).floor(), sx[1] + 19.0], TEXT, false);
        }
    }
    for wl in &card.wells {
        let [wx, wy, ww, wh] = wl.rect;
        let (wx, wy) = (x + wx, y + wy);
        if !wl.bare {
            draw.fill([wx - 1.0, wy - 1.0, ww + 2.0, wh + 2.0], 0xFF37_3737);
            draw.fill([wx, wy, ww, wh], 0xFF8B_8B8B);
        }
        // The fluid's own texture, tiled up from the bottom to its share of the well in squares (the host draws a
        // fluid only square): rows as tall as what is left (16 at most), each row's squares edge to edge.
        let filled = (wh * (wl.amount as f32 / wl.capacity as f32).min(1.0)).max(1.0).round();
        let mut yy = wy + wh;
        while yy > wy + wh - filled + 0.01 {
            let step = (yy - (wy + wh - filled)).min(16.0);
            let mut xx = wx;
            while xx < wx + ww - 0.01 {
                let size = (wx + ww - xx).min(step);
                // A square narrower than the row (the row's last) is drawn as a column of squares.
                let mut ty = yy - size;
                loop {
                    // The top square overlaps the one under it rather than spill out of the row.
                    draw.fluid(&wl.fluid, [xx, ty.max(yy - step)], size);
                    if ty <= yy - step + 0.01 {
                        break;
                    }
                    ty -= size;
                }
                xx += size;
            }
            yy -= step;
        }
        if let Some((sprite, uv)) = &wl.overlay {
            draw.sprite(sprite, *uv, [wx, wy, ww, wh], None);
        }
        jei.targets.push(([wx, wy, ww, wh], Target::Fluid(wl.fluid.clone(), wl.amount)));
    }
    for (item, p) in &card.icons {
        draw.item(*item, [x + p[0], y + p[1]], 16.0);
    }
    for (sprite, uv, p, period) in &card.animated {
        let shown = ((jei.millis % *period as u64) as f32 / *period as f32 * uv[2]).floor();
        if shown > 0.0 {
            draw.sprite(sprite, [uv[0], uv[1], shown, uv[3]], [x + p[0], y + p[1], shown, uv[3]], None);
        }
    }
    for (p, t, colour) in &card.centered {
        let text = text_of(host, t);
        let tw = draw.text_width(&text);
        draw.text(&text, [x + p[0] - (tw / 2.0).floor(), y + p[1]], *colour, false);
    }
    for (sprite, uv, p) in &card.sprites {
        draw.sprite(sprite, *uv, [x + p[0], y + p[1], uv[2], uv[3]], None);
    }
    if let Some(a) = card.arrow {
        recipe_arrow(draw, [x + a[0] + 1.0, y + a[1]]);
    }
    if card.shapeless {
        shapeless_icon(draw, [x + card.size[0] - 6.0, y - PAD + 2.0]);
    }
    let mut info_y = y + 24.0;
    for (p, t, colour) in &card.texts {
        let text = text_of(host, t);
        if t.starts_with('\u{1}') {
            for line in wrap(draw, &text, card.size[0]) {
                draw.text(&line, [x, info_y], *colour, false);
                info_y += 10.0;
            }
        } else {
            // A line wider than the card (a long loot table's name) ends in "…" at its edge; the whole line shows
            // under the pointer.
            let room = card.size[0] - p[0];
            if draw.text_width(&text) <= room {
                draw.text(&text, [x + p[0], y + p[1]], *colour, false);
            } else {
                let mut cut: String = text.clone();
                while !cut.is_empty() && draw.text_width(&format!("{cut}…")) > room {
                    cut.pop();
                }
                draw.text(&format!("{}…", cut.trim_end()), [x + p[0], y + p[1]], *colour, false);
                jei.targets.push(([x + p[0], y + p[1] - 1.0, room, 10.0], Target::Hover(vec![text.clone()])));
            }
        }
    }
    if let Some(g) = &card.graph {
        draw_graph(draw, g, [x, y], mouse, &mut jei.targets);
    }
    if let Some((kind, snbt, r)) = &card.entity {
        let rect = [x + r[0], y + r[1], x + r[0] + r[2], y + r[1] + r[3]];
        let data = mod_api::Compound::new();
        let entity = if snbt.is_empty() { GuiEntity::kind(kind.clone()) } else { GuiEntity::with_data(kind.clone(), crate::sources::client::snbt(snbt).unwrap_or(data)) };
        // Feet a little above the box's bottom (small mobs' legs stay in), the tallest mobs still fit.
        let size = (r[3] * 0.34).max(8.0);
        let _ = draw.entity(&entity, GuiEntityView::new(rect, size, -0.35, GuiEntityLook::At(mouse)));
    }
    for (r, lines) in &card.hovers {
        jei.targets.push(([x + r[0], y + r[1], r[2], r[3]], Target::Hover(lines.clone())));
    }
    if let Some((display, result)) = plus {
        let r = plus_rect;
        let state = jei.plus_states.get(&card.recipe);
        let ok = result == RecipeTransferResult::Moves;
        let usable = (ok || result == RecipeTransferResult::Missing) && !state.is_some_and(|s| s.blocked);
        button(draw, r, inside(r, mouse) && usable, usable);
        if let Some(tint) = state.and_then(|s| s.tint).filter(|_| usable) {
            draw.fill([r[0] + 1.0, r[1] + 1.0, r[2] - 2.0, r[3] - 2.0], tint);
        }
        let tw = draw.text_width("+");
        draw.text("+", [r[0] + ((r[2] - tw) / 2.0).floor() + 0.5, r[1] + 3.0], if usable { 0xFFFF_FFFF } else { 0xFFA0_A0A0 }, true);
        jei.targets.push((r, Target::Plus(display, card.recipe.clone(), screen_plus)));
    }
}

/// The game's tooltip of the stack under the pointer, with JEI's lines (the mod's name, the keys).
pub(crate) fn tooltip(jei: &Jei, host: &dyn ClientHost, draw: &mut dyn DrawContext, mouse: [f32; 2]) {
    let Some(target) = jei.target_at(mouse) else { return };
    let mut lines: Vec<Text> = Vec::new();
    match target {
        Target::Stack(_) | Target::Sub(_, _) => {
            let (item, stack) = match &target {
                Target::Sub(item, json) => (*item, crate::api::stack_with(*item, 1, json).unwrap_or_else(|| ItemStack::of(*item))),
                Target::Stack(item) => (*item, ItemStack::of(*item)),
                _ => unreachable!(),
            };
            let advanced = host.gui_options().advanced_item_tooltips;
            for e in ItemResource::of(&stack).tooltip(host, advanced, jei.creative).unwrap_or_default() {
                if let TooltipElement::Text(t, _) = e {
                    lines.push(t);
                }
            }
            let id = item.id(host).unwrap_or_default();
            let ns = id.split(':').next().unwrap_or("minecraft");
            lines.push(Text::literal(format!("§9§o{}", mod_name(host, ns))));
            // jeiA: a slot with alternatives says how to step through them, and which one this is.
            if let Some(sa) = jei.slots_at.iter().rev().find(|s| inside(s.rect, mouse)).filter(|s| s.count > 1) {
                let hint = tr(host, "jei.tooltip.cycle", "Scroll for the others");
                lines.push(Text::literal(format!("§8{hint} ({}/{})", sa.index + 1, sa.count)));
            }
        }
        Target::Fluid(fluid, amount) => {
            lines.push(Text::literal(host.fluid_name(&fluid)));
            lines.push(Text::literal(format!("§7{}", fmt_amount(amount))));
            let ns = fluid.split(':').next().unwrap_or("minecraft");
            lines.push(Text::literal(format!("§9§o{}", mod_name(host, ns))));
        }
        Target::Hover(own) => {
            for l in own {
                lines.push(Text::literal(host.translate(&l).unwrap_or(l)));
            }
        }
        Target::Plus(_, recipe, _) if jei.plus_states.get(&recipe).is_some_and(|s| !s.lines.is_empty()) => {
            for l in &jei.plus_states[&recipe].lines {
                lines.push(Text::literal(l.clone()));
            }
        }
        Target::Plus(display, _, _) => {
            let key = match display.map_or(Some(RecipeTransferResult::Moves), |d| jei.checks.get(&d).map(|c| c.result)) {
                Some(RecipeTransferResult::Moves) => "jei.tooltip.transfer",
                Some(RecipeTransferResult::Missing) => "jei.tooltip.error.recipe.transfer.missing",
                Some(RecipeTransferResult::NoRoom) => "jei.tooltip.error.recipe.transfer.inventory.full",
                _ => "jei.tooltip.error.recipe.transfer.unsupported",
            };
            lines.push(Text::literal(tr(host, key, "Move Items")));
        }
        Target::Bookmark(_) | Target::Tab(_) | Target::Title => {
            if let Target::Tab(n) = target
                && let Some(w) = &jei.window
                && let Some((cat, _)) = w.tabs.get(n)
            {
                let c = &jei.categories[*cat];
                lines.push(Text::literal(tr(host, &c.title, &c.title)));
            } else {
                return;
            }
        }
        _ => return,
    }
    OWN_TOOLTIP.store(true, Ordering::Relaxed);
    let _ = draw.gui_tooltip(&lines, mouse);
    OWN_TOOLTIP.store(false, Ordering::Relaxed);
}

/// JEI writes amounts in buckets and mB: 1.5 B, 250 mB.
pub(crate) fn fmt_amount(mb: u32) -> String {
    if mb >= 1000 && mb % 100 == 0 {
        let b = mb as f32 / 1000.0;
        format!("{} B", if b.fract() == 0.0 { format!("{b:.0}") } else { format!("{b}") })
    } else {
        format!("{mb} mB")
    }
}

fn mod_name(host: &dyn ClientHost, ns: &str) -> String {
    if ns == "minecraft" {
        return "Minecraft".to_owned();
    }
    host.translate(&format!("jei.modname.{ns}")).unwrap_or_else(|| {
        let mut s = ns.replace('_', " ");
        if let Some(c) = s.get_mut(0..1) {
            c.make_ascii_uppercase();
        }
        s
    })
}

/// A click on the window: true when it took it (every click over the window does).
pub(crate) fn click(jei: &mut Jei, at: [f32; 2], button: i32, shift: bool) -> bool {
    let Some(rect) = jei.window_rect else { return false };
    let Some(mut w) = jei.window.clone() else { return false };
    let target = jei.target_at(at);
    match target {
        Some(Target::Tab(n)) => {
            w.tab = n;
            w.page = 0;
        }
        Some(Target::PrevTab) => {
            w.tab = (w.tab + w.tabs.len() - 1) % w.tabs.len();
            w.page = 0;
        }
        Some(Target::NextTab) => {
            w.tab = (w.tab + 1) % w.tabs.len();
            w.page = 0;
        }
        Some(Target::PrevPage) => w.page = w.page.saturating_sub(1),
        Some(Target::NextPage) => w.page += 1,
        Some(Target::Title) => {
            // The title shows the whole category.
            let cat = w.tabs[w.tab].0;
            jei.open(Focus::Category(cat), false);
            return true;
        }
        Some(Target::Sub(item, json)) => {
            let focus = jei.focus_of(item, &json);
            jei.open(focus, button == 1);
            return true;
        }
        Some(Target::Stack(item)) => {
            if shift && jei.creative {
                jei.actions.push(Action::Give(item, 64));
            } else {
                jei.open(Focus::Item(item), button == 1);
            }
            return true;
        }
        Some(Target::Fluid(fluid, _)) => {
            jei.open(Focus::Fluid(fluid), button == 1);
            return true;
        }
        Some(Target::Plus(display, recipe, screen_plus)) => {
            if let Some(transfer) = screen_plus {
                jei.actions.push(Action::ScreenTransfer(transfer, recipe, shift));
                jei.close();
            } else if jei.plus_states.get(&recipe).is_some_and(|s| s.blocked) {
                // The mod's dry run blocks "+" (nothing to move): the click does nothing, as JEI's inactive button.
            } else if display.is_none_or(|d| jei.checks.get(&d).is_some_and(|c| c.result == RecipeTransferResult::Moves)) {
                jei.actions.push(Action::Transfer(recipe, shift));
                jei.close();
            }
            return true;
        }
        // A card's own tooltip area takes the click as the card does.
        Some(Target::Hover(_)) => return true,
        Some(Target::Bookmark(_)) | Some(Target::Close) | Some(Target::Back) => return false,
        // The item list's own controls: `list::press` takes them before the window is asked.
        Some(Target::ListPrev) | Some(Target::ListNext) | Some(Target::ListSearch) => return false,
        // A card's own tooltip area: nothing to press.
        Some(Target::Hover(_)) => return true,
        None => {
            if !inside(rect, at) {
                // A click beside the window closes it, as clicking off JEI's recipe screen goes back.
                jei.close();
            }
            return true;
        }
    }
    // Keep the page in range: the draw clamps it, so do the same here.
    jei.window = Some(Window { ..w });
    true
}

pub(crate) fn scroll(jei: &mut Jei, dy: f64, at: [f32; 2]) {
    // jeiA: the wheel over a slot with alternatives steps through them (up: the one before), holding its recipe
    // still until the pointer leaves it; elsewhere it turns the page.
    if let Some(sa) = jei.slots_at.iter().rev().find(|s| inside(s.rect, at)).filter(|s| s.count > 1).copied() {
        let by = if dy > 0.0 { -1 } else if dy < 0.0 { 1 } else { return };
        let millis = jei.millis;
        let Some(slot) = jei.cards.get(sa.card).and_then(|c| c.slots.get(sa.slot)).cloned() else { return };
        let recipe = jei.cards[sa.card].recipe.clone();
        if let Some(w) = jei.window.as_mut() {
            let hold = match &mut w.hold {
                Some(h) if h.card == sa.card => h,
                other => other.insert(Hold { card: sa.card, recipe, since: millis, picks: Vec::new() }),
            };
            step(&mut hold.picks, &slot.stacks, &slot.components, sa.index, by);
        }
        return;
    }
    if let Some(w) = jei.window.as_mut() {
        if dy > 0.0 {
            w.page = w.page.saturating_sub(1);
        } else if dy < 0.0 {
            w.page += 1;
        }
    }
}

/// The card under the pointer: none here, kept for the stack keys.
pub(crate) fn hovered_stack(jei: &Jei, at: [f32; 2]) -> Option<ItemKey> {
    match jei.target_at(at) {
        Some(Target::Stack(k)) | Some(Target::Sub(k, _)) => Some(k),
        _ => None,
    }
}

/// The focus of the card stack under the pointer, its subtype included.
pub(crate) fn hovered_focus(jei: &Jei, at: [f32; 2]) -> Option<crate::view::Focus> {
    match jei.target_at(at) {
        Some(Target::Stack(k)) => Some(crate::view::Focus::Item(k)),
        Some(Target::Sub(k, json)) => Some(jei.focus_of(k, &json)),
        _ => None,
    }
}

pub(crate) fn hovered_fluid(jei: &Jei, at: [f32; 2]) -> Option<String> {
    match jei.target_at(at) {
        Some(Target::Fluid(f, _)) => Some(f),
        _ => None,
    }
}

#[allow(dead_code)]
pub(crate) fn role_name(r: Role) -> &'static str {
    match r {
        Role::Input => "input",
        Role::Output => "output",
        Role::Other => "other",
    }
}

// jeiA: stepping through a slot's alternatives and the links between a recipe's slots.
#[cfg(test)]
mod cycle_tests {
    use super::*;

    fn k(n: u32) -> ItemKey {
        mod_api::__private::parts_installation::item_key(n)
    }

    fn list(ns: &[u32]) -> Vec<(ItemKey, u32)> {
        ns.iter().map(|n| (k(*n), 1)).collect()
    }

    /// Unheld, a slot cycles once a second; a pick it holds wins over the cycle.
    #[test]
    fn a_pick_wins_over_the_cycle() {
        let rubbers = list(&[1, 2, 3]);
        assert_eq!(shown_index(&rubbers, &[], 0, &[]), Some(0));
        assert_eq!(shown_index(&rubbers, &[], 2_500, &[]), Some(2));
        assert_eq!(shown_index(&[], &[], 0, &[]), None);
        assert_eq!(shown_index(&rubbers, &[], 0, &[(k(3), String::new())]), Some(2));
        // A pick the slot can't hold leaves it cycling.
        assert_eq!(shown_index(&rubbers, &[], 1_000, &[(k(9), String::new())]), Some(1));
    }

    /// The wheel steps both ways round the list, and slots holding the same tag show the same choice.
    #[test]
    fn stepping_links_the_recipes_slots() {
        let rubbers = list(&[1, 2, 3]);
        let sticks = list(&[7]);
        let mut picks = Vec::new();
        step(&mut picks, &rubbers, &[], 0, 1);
        assert_eq!(picks, vec![(k(2), String::new())]);
        // Every rubber slot of the recipe shows mod B's rubber now; the stick slot is left alone.
        assert_eq!(shown_index(&rubbers, &[], 0, &picks), Some(1));
        assert_eq!(shown_index(&rubbers, &[], 5_000, &picks), Some(1));
        assert_eq!(shown_index(&sticks, &[], 0, &picks), Some(0));
        // Back past the first goes round to the last; the old rubber pick gives way.
        step(&mut picks, &rubbers, &[], 0, -1);
        assert_eq!(picks, vec![(k(3), String::new())]);
        // A slot with one stack has nothing to step.
        step(&mut picks, &sticks, &[], 0, 1);
        assert_eq!(picks, vec![(k(3), String::new())]);
        // Another list's pick stays beside it.
        let ingots = list(&[4, 5]);
        step(&mut picks, &ingots, &[], 0, 1);
        assert_eq!(picks, vec![(k(3), String::new()), (k(5), String::new())]);
        assert_eq!(shown_index(&rubbers, &[], 0, &picks), Some(2));
        assert_eq!(shown_index(&ingots, &[], 0, &picks), Some(1));
    }

    /// Stacks of one item told apart by components (potions) step one by one.
    #[test]
    fn subtypes_step_by_their_components() {
        let potions = list(&[6, 6, 6]);
        let comps: Vec<String> = ["{\"p\":\"a\"}", "{\"p\":\"b\"}", "{\"p\":\"c\"}"].iter().map(|s| (*s).to_owned()).collect();
        let mut picks = Vec::new();
        step(&mut picks, &potions, &comps, 0, 1);
        assert_eq!(shown_index(&potions, &comps, 0, &picks), Some(1));
        step(&mut picks, &potions, &comps, 1, 1);
        assert_eq!(shown_index(&potions, &comps, 0, &picks), Some(2));
        assert_eq!(picks.len(), 1);
    }
}
