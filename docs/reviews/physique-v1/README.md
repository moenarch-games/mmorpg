# Physique V1 browser evidence (#635)

This records the supported body-morph stage, not final acceptance of every requirement in #635.
The actual female knight outfit's rolling publication and final published-input acceptance remain
open under [asset-tooling #630](https://github.com/moritzbrantner/asset-tooling/issues/630).
The client selects that outfit only if its real descriptor/catalog provides it; a catalog without
it keeps the archer outfit and discloses the gap. The separate corrected-candidate check below
does not claim that armor is already published.

The images below are direct Chrome captures of the production browser client. Each preset series
keeps the same selection camera and neutral idle pose; the world images use the current game
camera. They show real geometry, clothing, hair and socket items through the existing loader,
not illustrations or a separate renderer. Shape views use the female default mesh whose glTF/bin
match the published female bundle; the final world/save check uses its published clips too.

## Inputs and capability audit

Acquired the actual rolling releases. Their `export.ref.json` asset-bundle hashes are:

- Male: `9f512bc8c5732e42386d4a40a0108a61f09f38fb469770171b484a31d5a99478`.
- Female: `7e1be2008546d98a385450a5be26a4d044989010b7d72e76f80fdb2d310bdc84`.
- Hair kit: `2ff756cff0bc26d9818e64da362ec0c4b571301be995b3cff05369ac5e3b06b7`.

Both bases have 65 joints and 179 clip descriptors. The male descriptor names 27 full-detail
variants with seven new bipolar controls; the female names four with eight, including bust.
Both also have the existing build, face and height controls. New named targets include
`frame.narrow/broad`, `muscularity.soft/defined`, `shoulders.narrow/broad`, `waist.narrow/wide`,
`hips.narrow/wide`, `glutes.small/full`, `jaw.receding/projected`, and female `bust.small/full`.
The updated hair kit carries the physique targets too. No new reduced-detail physique variant
is adopted. See the [per-base/per-renderer capability table](../../RANGER_ASSETS.md#player-physique-635).

## Observed browser path

The focused production-browser journey exercised six presets, continuous slider input, one
keyboard slider adjustment, cosmetic stat neutrality, female face choices, Cancel, Confirm,
creation, local-world entry, movement/traversal, leaving, selecting the saved character after
reload, and restoration of the same physique/face values. The local character's five Build values
stayed neutral; cosmetic changes left both displayed modifiers at 0%. The saved cosmetic values
were bust 85%, waist 26%, hips 80%, glutes 85%, jaw width 65%, jaw projection 70%.
No uncaught page exception occurred on the completed path.

An initial reload assertion selected the built-in Aelric rather than the saved Lyra; the narrowed
check explicitly reselected Lyra and passed. An earlier female load requested absent cap/helm
entries; the loader and disabled picker choices now follow actual entries. T3 preview automation
was unavailable because the host reported its browser sandbox blocked by AppArmor, so this check
used the repository's existing Playwright harness with installed Chrome and its existing GPU flags.

These samples establish visible silhouette changes and the saved local-world path. They do not
establish clearance for every clip/control combination, final female armor fit, native physique,
online transport parity, or final owner art approval. Native/online physique remains average and
unavailable in the editor; no collision, reach, stat coefficient or event-timing change is involved.

## Actual same-camera views

| Preset | Tunic | Warden armor |
| --- | --- | --- |
| Average | [View](male-tunic-average.jpg) | [View](male-warden-average.jpg) |
| Lean | [View](male-tunic-lean.jpg) | [View](male-warden-lean.jpg) |
| Athletic | [View](male-tunic-athletic.jpg) | [View](male-warden-athletic.jpg) |
| Muscular | [View](male-tunic-muscular.jpg) | [View](male-warden-muscular.jpg) |
| Stocky | [View](male-tunic-stocky.jpg) | [View](male-warden-stocky.jpg) |
| Heavyset | [View](male-tunic-heavyset.jpg) | [View](male-warden-heavyset.jpg) |

Female tunic proportions: [neutral](female-tunic-neutral.jpg), [lower cosmetic limits](female-tunic-lower.jpg),
[upper cosmetic limits](female-tunic-upper.jpg), and [combined](female-tunic-combined.jpg),
[profile](female-tunic-combined-profile.jpg), [back](female-tunic-combined-back.jpg).

Existing face-key compositions: [Refined](female-face-refined.jpg), [Bright](female-face-bright.jpg),
[Resolute](female-face-resolute.jpg); jaw [narrow/receding](female-jaw-narrow.jpg) and
[broad/projecting](female-jaw-prominent.jpg).

Saved published-base path: [game camera](female-world-game-camera.jpg),
[walking](female-world-walk.jpg), [jumping](female-world-jump.jpg),
[saved character reselected after reload](female-saved-reloaded.jpg).

## Corrected female knight candidate, not a published-armor claim

The owning producer's completed #630 artifact has tar SHA-256
`e03978d7dd019cf6e7d60d3bf85ce0ecc51153133414394c3e12062fd6238776`.
It provides `outfit.knight`/`outfit.knight.skin.light`, a tabard and 15 body plates;
the unfitted bascinet is omitted. Its `physiqueFit.bodyMorphScale = 0.35` is already
baked into the authored target deltas. The client applies ordinary normalized weights;
it adds no second multiplier or deformer.

The focused consumer check drew [neutral](female-knight-neutral-candidate.jpg),
[all-lower](female-knight-lower-candidate.jpg), [all-upper](female-knight-upper-candidate.jpg)
and [combined curvy](female-knight-curvy-candidate.jpg) looks. Actual images were inspected:
the earlier combined-limit pinch/inversion was not seen in these samples, the 15 plates
stayed attached, and the source's angular collar/tabard remains. This is an observation
of the sampled poses, not a general clearance or art-quality certification.

With motion enabled, the real local-world check captured
[walking](female-knight-walk-candidate.jpg) (`Jog_Fwd_Loop`, 0.3959 s),
[sword attack](female-knight-attack-candidate.jpg) (`Sword_Slash_Mid_L`, 0.0666 s),
and [jump](female-knight-jump-candidate.jpg) (`Jump_Start`, 0.0333 s), with the existing
weapon/shield grip layers and no uncaught page exceptions. The final rolling armor
release remains an external prerequisite; #635 stays open.
