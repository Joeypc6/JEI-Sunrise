# Changelog

## 0.5.1 (2026-11-19)

- Every mod's recipes now show: JEI asks the server for every recipe type, so AE2's, Tinkers', Ender IO's and other
  native mods' crafting, inscriber, charger and machine recipes reach the client (9193 cards with the test pack, up
  from 5768).
- Scroll over a slot that holds several items (a tag such as any rubber, or several outputs) to step through them;
  slots holding the same list in a recipe step together, and cycling resumes when the pointer leaves the recipe.
- `/jei show` waits for the recipe index instead of dropping the request, and says so in chat when nothing makes the
  item; the open window survives an index rebuild. New `/jei why <item>` names what makes and uses an item.
- A launcher icon (`icon.png`).
- Pinned to the Nightly of 2026-11-19 (`ed3ac19`).

## 0.5.0 (2026-11-17)

- New tabs for where things come from, read from the server's data: World Gen (ore height graphs), Block Drops,
  Mob Drops (the mob drawn, its spawns, XP and drops), Chest Loot, Archaeology, Fishing, Piglin Bartering, Shearing,
  Villager Trades, Other Sources and How To Get It.
- Villager trades go to the professions that offer them (26.3's tags), with the profession's clothes, work station
  and level name; structure-only mobs name their structure (shulkers: End Cities).
- The item list is the creative inventory's: no operator or technical items, no uncraftable potions or tipped arrows.
- Recipes with nothing to show, and blocks that simply drop themselves, have no card.
- Smithing trims show the trimmed armour; loot counts are per stack (not times the rolls); concrete shows its powder
  and water.
- The recipe arrow is drawn clean (no dark box); long card lines end in "…"; inventory tooltips no longer draw under
  the recipe window.
- Known, in Sunrise: on the pinned Nightly some mobs drawn in the window lose faces (chicken legs) or show their body
  through their clothes (villagers). Fixed in Sunrise by sunrise-org/sunrise#2365; JEI needs no change for it.

## 0.4.0 (2026-11-17)

- First release for Sunrise: pinned to the Nightly of 2026-11-16 (game commit `234db8df62`), built from Sunrise's
  mod API instead of the arismoko/horizon fork's.
- The "+" check asks the server by recipe id (`transfer_recipe_check`), as Sunrise's API now takes it.
- The item panel pages with the mouse wheel through Sunrise itself; the fork's internals injection is gone.

## 0.3.1 and earlier

Releases for the arismoko/horizon fork (`horizon-jei`).
