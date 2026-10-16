/** A catalog export shaped like the WASM `catalog()` output, for decoder and UI tests. */
export function catalogJson(revision = "3"): Record<string, unknown> {
  return {
    format: "mmorpg.catalog",
    version: 6,
    itemCatalogRevision: "3",
    items: [
      { id: 1, name: "Torn Fur", maxStack: 20, slot: null, stats: { stamina: 0, strength: 0, agility: 0, intellect: 0 }, sellPrice: 1 },
      { id: 2, name: "Worn Dagger", maxStack: 1, slot: "mainHand", stats: { stamina: 0, strength: 2, agility: 2, intellect: 0 }, sellPrice: 2 },
      { id: 9, name: "Worn Boots", maxStack: 1, slot: "feet", stats: { stamina: 1, strength: 0, agility: 2, intellect: 0 }, sellPrice: 3 },
      { id: 10, name: "Wolf Pelt", maxStack: 10, slot: null, stats: { stamina: 0, strength: 0, agility: 0, intellect: 0 }, sellPrice: 1 },
    ],
    contentRevision: revision,
    contentFingerprint: "3cbc808bbe89b29c",
    creatureTemplates: [
      { id: 1, name: "Timber Wolf", family: "wolf", behaviour: "aggressive", minLevel: 1, maxLevel: 2, elite: false, halfExtents: [40, 45, 40] },
      { id: 2, name: "Young Boar", family: "boar", behaviour: "neutral", minLevel: 1, maxLevel: 2, elite: false, halfExtents: [45, 45, 45] },
    ],
    npcs: [
      { id: 1, name: "Marshal Elden Greywatch", role: "quest_giver", level: 10 },
      { id: 2, name: "Tanner Hilda Brook", role: "quest_giver", level: 5 },
      { id: 3, name: "Innkeeper Bram Tolliver", role: "vendor", level: 5 },
      { id: 5, name: "Brother Aldous", role: "spirit_healer", level: 10 },
      { id: 6, name: "Greyhaven Guard", role: "guard", level: 10 },
    ],
    areas: [{ id: 1, name: "Greyhaven Outpost" }],
    classes: [
      { id: 0, name: "warden", resource: "rage" },
      { id: 1, name: "ranger", resource: "focus" },
      { id: 2, name: "arcanist", resource: "mana" },
    ],
    vendorCatalogRevision: "2",
    vendors: [{ npc: 3, offers: [{ item: 2, price: 9 }] }],
    questCatalogRevision: "1",
    quests: [
      {
        id: 1, name: "Trouble in the Woods", text: "Slay six wolves.", giver: 1, ender: 1, prerequisite: null,
        objectives: [{ kind: "kill", target: 1, source: null, count: 6 }], experience: 150, copper: 10, choices: [],
      },
      {
        id: 2, name: "Pelts for the Tanner", text: "Bring me five wolf pelts.", giver: 2, ender: 2, prerequisite: 1,
        objectives: [{ kind: "collect", target: 10, source: 1, count: 5 }], experience: 200, copper: 15, choices: [9, 2],
      },
      {
        id: 3, name: "The Missing Farmhand", text: "Ask at the outpost.", giver: 2, ender: 1, prerequisite: 2,
        objectives: [{ kind: "talk", target: 1, source: null, count: 1 }, { kind: "explore", target: 1, source: null, count: 1 }],
        experience: 100, copper: 5, choices: [],
      },
    ],
    abilityCatalogRevision: "1",
    abilities: [
      { id: 1, name: "Heroic Strike", user: "warden", level: 1, cost: 15, castTicks: 0, channel: false, cooldown: 0, aura: null },
      { id: 2, name: "Shield Bash", user: "warden", level: 2, cost: 10, castTicks: 0, channel: false, cooldown: 360, aura: 6 },
      { id: 9, name: "Firebolt", user: "arcanist", level: 1, cost: 25, castTicks: 60, channel: false, cooldown: 0, aura: null },
      { id: 13, name: "Muck Bolt", user: "creature", level: 1, cost: 0, castTicks: 45, channel: false, cooldown: 240, aura: null },
    ],
  };
}
