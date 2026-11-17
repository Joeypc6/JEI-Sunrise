//! JEI's recipe window (`RecipesGui`) over the open screen: a row of category tabs, the category's title and page
//! arrows, its catalysts down the left side, and as many recipe cards as fit, each with its "+" where it can go into
//! the open crafting grid. Drawn in `screen_render_post`, which draws over any screen; its input comes in the
//! `screen_*_pre` hooks, which it cancels so nothing reaches the screen under it.

use crate::state::{Action, Jei, Target, Window, inside};
use crate::view::{Card, Focus, Role};
use crate::{bookmarks, displays, tr};
use mod_api::*;

const SLOT: f32 = 18.0;
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
    if !draw.sprite("horizon/jei/slot", [0.0, 0.0, 18.0, 18.0], [at[0], at[1], 18.0, 18.0], None) {
        draw.fill([at[0], at[1], 18.0, 18.0], 0xFF37_3737);
        draw.fill([at[0] + 1.0, at[1] + 1.0, 17.0, 17.0], 0xFFFF_FFFF);
        draw.fill([at[0] + 1.0, at[1] + 1.0, 16.0, 16.0], 0xFF8B_8B8B);
    }
}

fn button(draw: &mut dyn DrawContext, r: [f32; 4], hover: bool, enabled: bool) {
    let name = if !enabled { "horizon/jei/button_disabled_v2" } else if hover { "horizon/jei/button_highlight_v2" } else { "horizon/jei/button_enabled_v2" };
    draw.gui_nine_slice(name, r, 0xFFFF_FFFF);
}

fn arrow_icon(draw: &mut dyn DrawContext, at: [f32; 2], next: bool) {
    let name = if next { "horizon/jei/icons/arrow_next_v2" } else { "horizon/jei/icons/arrow_previous_v2" };
    if !draw.sprite(name, [0.0, 0.0, 9.0, 7.0], [at[0], at[1], 9.0, 7.0], None) {
        draw.text(if next { ">" } else { "<" }, at, 0xFFFF_FFFF, true);
    }
}

/// The stack a slot shows now: it cycles once a second, as JEI's do.
pub(crate) fn shown(stacks: &[(ItemKey, u32)], millis: u64) -> Option<(ItemKey, u32)> {
    (!stacks.is_empty()).then(|| stacks[(millis / 1000) as usize % stacks.len()])
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

pub(crate) fn draw(jei: &mut Jei, host: &dyn ClientHost, draw: &mut dyn DrawContext, mouse: [f32; 2]) {
    jei.targets.clear();
    jei.window_rect = None;
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
        used += h + 8.0;
        fullest = fullest.max(used - 8.0);
    }
    let height = (40.0 + fullest + 8.0).clamp(110.0, most.max(110.0));
    let (x0, y0) = (((sw - width) / 2.0).floor(), (TAB + 4.0 + (sh - TAB - 8.0 - height) / 2.0).floor());
    let r = [x0, y0, width, height];
    jei.window_rect = Some([x0 - 26.0, y0 - TAB, width + 26.0, height + TAB]);
    // Dim the screen under it, but not the item panel right of the container (JEI's list stays lit).
    let dim_right = jei.screen_rect.map(|s| (s[0] + s[2] + 4.0).max(x0 + width + 2.0)).unwrap_or(sw).min(sw);
    draw.fill([0.0, 0.0, dim_right, sh], 0x8010_1010);
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
    let title = tr(host, &category.title, &category.title);
    let tw = draw.text_width(&title);
    draw.text(&title, [x0 + (width - tw) / 2.0, row + 3.0], TEXT, false);
    jei.targets.push(([x0 + (width - tw) / 2.0, row, tw, 12.0], Target::Title));
    // Cards that fit, then the page row.
    let top = row + 34.0;
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
    let label = format!("{}/{}", page + 1, pages.len());
    let lw = draw.text_width(&label);
    draw.text(&label, [x0 + (width - lw) / 2.0, page_row + 3.0], TEXT, false);
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
    for (k, ci) in cards[start..end].iter().enumerate() {
        let card = jei.cards[*ci].clone();
        let h = heights[start + k];
        let x = x0 + ((width - card.size[0]) / 2.0).floor();
        draw_card(jei, host, draw, &card, [x, y], h, mouse, &mut checks);
        y += h + 8.0;
    }
    if !checks.is_empty() {
        jei.actions.push(Action::Check(checks));
    }
    // Tooltips last, over everything.
    tooltip(jei, host, draw, mouse);
}

#[allow(clippy::too_many_arguments)]
fn draw_card(jei: &mut Jei, host: &dyn ClientHost, draw: &mut dyn DrawContext, card: &Card, at: [f32; 2], h: f32, mouse: [f32; 2], checks: &mut Vec<(i32, String)>) {
    let [x, y] = at;
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
    if card.transfer
        && let Some(display) = displays::of(&card.recipe)
    {
        checks.push((display, card.recipe.clone()));
        if let Some(check) = jei.checks.get(&display) {
            if check.result == RecipeTransferResult::Missing {
                missing = check.missing_cells.clone();
            }
            plus = Some((Some(display), check.result));
        }
    } else if jei.menu.as_deref().is_some_and(|menu| crate::api::transfers_into(menu, &jei.categories[card.category].id)) {
        // A mod moves this category into the open menu (`menu_transfer`). There is no dry run for a mod menu, so
        // "+" is offered and the server's part answers the click.
        plus = Some((None, RecipeTransferResult::Moves));
    }
    for s in &card.slots {
        let sx = [x + s.at[0] - 1.0, y + s.at[1] - 1.0];
        if !card.frameless {
            slot_frame(draw, sx);
        }
        if s.cell.is_some_and(|c| missing.contains(&c)) && !s.stacks.is_empty() {
            draw.fill([sx[0] + 1.0, sx[1] + 1.0, 16.0, 16.0], 0x80FF_0000);
        }
        let index = if s.stacks.is_empty() { 0 } else { (jei.millis / 1000) as usize % s.stacks.len() };
        let json = s.components.get(index).filter(|c| !c.is_empty()).cloned();
        if let (Some((item, count)), Some(json)) = (shown(&s.stacks, jei.millis), json) {
            // A stack with components (a subtype): drawn as itself, and its focus is its subtype.
            if let Some(stack) = crate::api::stack_with(item, count, &json) {
                draw.gui_draw_item_with(&stack, [sx[0] + 1.0, sx[1] + 1.0], None);
            }
            jei.targets.push(([sx[0], sx[1], SLOT, SLOT], Target::Sub(item, json)));
        } else if let Some((item, count)) = shown(&s.stacks, jei.millis) {
            match ItemStack::new(item, count.max(1)) {
                Some(stack) if count > 1 => draw.gui_draw_item_with(&stack, [sx[0] + 1.0, sx[1] + 1.0], None),
                _ => {
                    draw.item(item, [sx[0] + 1.0, sx[1] + 1.0], 16.0);
                }
            }
            jei.targets.push(([sx[0], sx[1], SLOT, SLOT], Target::Stack(item)));
        }
        if let Some(c) = s.chance.filter(|c| *c < 1.0) {
            let t = format!("{}%", (c * 1000.0).round() / 10.0);
            draw.text(&t, [sx[0], sx[1] + 19.0], TEXT, false);
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
        let ok = draw.sprite("gui/sprites/container/furnace/burn_progress", [0.0, 0.0, 24.0, 16.0], [x + a[0], y + a[1], 24.0, 16.0], Some(0xFF80_8080))
            || draw.sprite("minecraft:container/furnace/burn_progress", [0.0, 0.0, 24.0, 16.0], [x + a[0], y + a[1], 24.0, 16.0], Some(0xFF80_8080));
        if !ok {
            draw.text("->", [x + a[0] + 6.0, y + a[1] + 4.0], TEXT, false);
        }
    }
    if card.shapeless {
        draw.text("~", [x + card.size[0] - 8.0, y], TEXT, false);
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
            draw.text(&text, [x + p[0], y + p[1]], *colour, false);
        }
    }
    for (r, lines) in &card.hovers {
        jei.targets.push(([x + r[0], y + r[1], r[2], r[3]], Target::Hover(lines.clone())));
    }
    if let Some((display, result)) = plus {
        let r = [x + card.size[0] + 4.0, y + h - 13.0, 13.0, 13.0];
        let ok = result == RecipeTransferResult::Moves;
        let usable = ok || result == RecipeTransferResult::Missing;
        button(draw, r, inside(r, mouse) && ok, usable);
        draw.text("+", [r[0] + 4.0, r[1] + 3.0], if ok { 0xFFFF_FFFF } else { 0xFFFF_6060 }, true);
        jei.targets.push((r, Target::Plus(display, card.recipe.clone())));
    }
}

/// The game's tooltip of the stack under the pointer, with JEI's lines (the mod's name, the keys).
fn tooltip(jei: &Jei, host: &dyn ClientHost, draw: &mut dyn DrawContext, mouse: [f32; 2]) {
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
            let id = item.id(host);
            let ns = id.split(':').next().unwrap_or("minecraft");
            lines.push(Text::literal(format!("§9§o{}", mod_name(host, ns))));
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
        Target::Plus(display, _) => {
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
    let _ = draw.gui_tooltip(&lines, mouse);
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
        Some(Target::Plus(display, recipe)) => {
            if display.is_none_or(|d| jei.checks.get(&d).is_some_and(|c| c.result == RecipeTransferResult::Moves)) {
                jei.actions.push(Action::Transfer(recipe, shift));
                jei.close();
            }
            return true;
        }
        // A card's own tooltip area takes the click as the card does.
        Some(Target::Hover(_)) => return true,
        Some(Target::Bookmark(_)) | Some(Target::Close) | Some(Target::Back) => return false,
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

pub(crate) fn scroll(jei: &mut Jei, dy: f64) {
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
