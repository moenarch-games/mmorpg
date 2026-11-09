//! The instructions handed to the reconciliation agent after a paint import.
use serde::{Deserialize, Serialize};

/// The report of `mmorpg-world-studio import`.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportReport {
    pub height: HeightReport,
    pub biome: BiomeReport,
    pub changed_bounds: Option<Rectangle>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HeightReport {
    pub changed: u32,
    pub painted: u32,
    pub skipped_locked: u32,
    pub lowest_metres: f64,
    pub highest_metres: f64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BiomeReport {
    pub changed: u32,
    pub painted: u32,
}

/// An XZ rectangle in metres.
#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Rectangle {
    pub min_x: f64,
    pub min_z: f64,
    pub max_x: f64,
    pub max_z: f64,
}

/// Everything the agent is told about one edit.
pub struct Brief<'a> {
    pub report: &'a ImportReport,
    pub selection: Rectangle,
    pub branch: &'a str,
    pub before_images: &'a str,
    pub direction: &'a str,
}

impl Brief<'_> {
    /// The complete prompt.
    #[must_use]
    pub fn prompt(&self) -> String {
        let Self {
            report,
            selection,
            branch,
            before_images,
            direction,
        } = self;
        let changed = report.changed_bounds.map_or_else(
            || "nowhere".to_owned(),
            |at| {
                format!(
                    "x {} to {} m, z {} to {} m",
                    at.min_x, at.max_x, at.min_z, at.max_z
                )
            },
        );
        let direction = if direction.trim().is_empty() {
            "None given: make the place feel deliberate and consistent with its surroundings."
        } else {
            direction.trim()
        };
        format!(
            "You are reconciling Greyhaven Vale after its owner repainted the ground in Krita. \
             The painted terrain is already applied in this worktree, on branch `{branch}`, in \
             `content/greyhaven/paint/height.json` and `biome.json`.\n\n\
             ## What changed\n\
             - Selected area: x {sx0} to {sx1} m, z {sz0} to {sz1} m (+X east, +Z south; 100 units per metre).\n\
             - Changed samples lie at: {changed}.\n\
             - Height: {height} samples changed, between {low} and {high} m; {skipped} locked samples were not moved.\n\
             - Biome: {biome} samples changed.\n\
             - The world before this edit, as layered top-down images: `{before_images}`.\n\n\
             ## Creative direction from the owner\n{direction}\n\n\
             ## Your task\n\
             Reconcile what now disagrees with the new ground. Look at what stands in or near the \
             changed area: roads and paths in `content/greyhaven/landscape.json`, forests, spawns, \
             NPC and quest positions, region colliders under `crates/mmorpg-core/src/content/greyhaven_vale/`, \
             and scenery. Decide what the new shape of the land asks for and make those authored changes, \
             keeping anything that still works. Make creative choices where the data leaves them open, \
             and say what you chose.\n\n\
             ## Rules\n\
             - Read `AGENTS.md` first. Its authority and determinism sections apply.\n\
             - Treat the owner's paint as intended: do not revert `content/greyhaven/paint/`.\n\
             - Run `cargo test -p mmorpg-scenery --lib paint`, plus one focused check for what you change.\n\
             - Stage everything, including the paint files, and commit to this branch when finished.\n\
             - Do not push, open a pull request, or touch anything outside this worktree.\n\
             - End with a short summary of what you changed, what you chose, and anything unverified.\n",
            sx0 = selection.min_x,
            sx1 = selection.max_x,
            sz0 = selection.min_z,
            sz1 = selection.max_z,
            height = report.height.changed,
            low = report.height.lowest_metres,
            high = report.height.highest_metres,
            skipped = report.height.skipped_locked,
            biome = report.biome.changed,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::{BiomeReport, Brief, HeightReport, ImportReport, Rectangle};

    fn report() -> ImportReport {
        ImportReport {
            height: HeightReport {
                changed: 52,
                painted: 52,
                skipped_locked: 3,
                lowest_metres: 0.1,
                highest_metres: 5.7,
            },
            biome: BiomeReport {
                changed: 16,
                painted: 16,
            },
            changed_bounds: Some(Rectangle {
                min_x: -140.,
                min_z: 116.,
                max_x: -112.,
                max_z: 144.,
            }),
        }
    }

    #[test]
    fn prompt_states_the_edit_the_direction_and_the_limits() {
        let report = report();
        let prompt = Brief {
            report: &report,
            selection: Rectangle {
                min_x: -160.,
                min_z: 100.,
                max_x: -100.,
                max_z: 160.,
            },
            branch: "world-edit/e1",
            before_images: "/tmp/export/selection",
            direction: "Turn the hill into a lookout.",
        }
        .prompt();
        for expected in [
            "world-edit/e1",
            "52 samples changed, between 0.1 and 5.7 m",
            "3 locked samples",
            "x -140 to -112 m",
            "Turn the hill into a lookout.",
            "/tmp/export/selection",
            "Do not push",
        ] {
            assert!(prompt.contains(expected), "{expected}");
        }
    }

    #[test]
    fn an_empty_direction_still_gives_the_agent_a_goal() {
        let report = report();
        let prompt = Brief {
            report: &report,
            selection: report.changed_bounds.unwrap(),
            branch: "b",
            before_images: "p",
            direction: "  ",
        }
        .prompt();
        assert!(prompt.contains("None given"));
    }
}
