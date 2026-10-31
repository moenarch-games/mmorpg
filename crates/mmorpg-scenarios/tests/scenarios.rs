//! Checked-in control-plane scenarios and their expected outputs, and the bot runner's own
//! behaviour. The checked-in bot scenarios run one trial each in `bot_scenarios.rs`.
//!
//! Every `scenarios/control-plane/<name>.toml` must pass and reproduce `<name>.expected` byte
//! for byte. After an intentional behavior change, regenerate with:
//!
//! ```sh
//! MMORPG_SCENARIOS_UPDATE=1 cargo test -p mmorpg-scenarios --locked
//! ```

mod support;

use std::path::Path;
use std::process::Command;

use mmorpg_scenarios::{Report, bots, control_plane};

fn run_bots(text: &str) -> Result<Report, String> {
    bots::run(&bots::load(text)?)
}

fn run_control_plane(text: &str) -> Result<Report, String> {
    control_plane::run(&control_plane::load(text)?)
}

#[test]
fn control_plane_scenarios_match_expected_output() {
    let mut mismatches = Vec::new();
    for path in support::scenario_files("control-plane") {
        let text = std::fs::read_to_string(&path).unwrap();
        let report =
            run_control_plane(&text).unwrap_or_else(|error| panic!("{}: {error}", path.display()));
        let actual = report.to_text();
        let expected_path = path.with_extension("expected");
        if support::updating() {
            std::fs::write(&expected_path, &actual).unwrap();
        } else {
            let expected = std::fs::read_to_string(&expected_path).unwrap_or_default();
            if expected != actual {
                mismatches.push(format!(
                    "{}\n--- expected\n{expected}--- actual\n{actual}",
                    path.display()
                ));
            }
        }
        assert!(
            report.passed(),
            "checked-in scenario {} must pass:\n{actual}",
            path.display()
        );
        support::check_json_matches_text(&report);
    }
    assert!(
        mismatches.is_empty(),
        "scenario output changed; rerun with MMORPG_SCENARIOS_UPDATE=1 if intentional:\n{}",
        mismatches.join("\n")
    );
}

#[test]
fn unmet_bot_expectations_fail_the_scenario() {
    let report = run_bots(
        r#"
name = "wrong"
zone = 1
ticks = 2
[[bots]]
name = "alice"
[[steps]]
tick = 0
bot = "alice"
action = "join"
[[steps]]
tick = 1
bot = "alice"
action = "move"
forward = 1
strafe = 0
facing = 16384
seq = 0
[[expect]]
kind = "position"
bot = "alice"
tick = 2
position = [999, 90, 0]
"#,
    )
    .unwrap();
    let text = report.to_text();
    assert!(!report.passed());
    assert_eq!(report.failures(), 2, "{text}");
    assert!(text.contains("FAIL expected applied"), "{text}");
    assert!(text.contains("got [-1550, 90, 1250]"), "{text}");
    assert!(text.ends_with("failures=2\n"), "{text}");
}

#[test]
fn area_expectations_compare_with_the_core_area_table() {
    let report = run_bots(
        r#"
name = "wrong-area"
zone = 1
ticks = 1
[[bots]]
name = "alice"
[[steps]]
tick = 0
bot = "alice"
action = "join"
[[steps]]
tick = 0
bot = "alice"
action = "jump"
seq = 3
[[expect]]
kind = "area"
bot = "alice"
tick = 1
area = "Nowhere"
"#,
    )
    .unwrap();
    let text = report.to_text();
    assert!(!report.passed());
    assert!(text.contains("jump -> applied seq=3"), "{text}");
    assert!(text.contains("FAIL alice in Greyhaven Outpost"), "{text}");
}

#[test]
fn combat_expectations_read_the_decoded_projection() {
    // Guard NPC 8 stands at the west gate, inside the hub bots' interest radius.
    let report = run_bots(
        r#"
name = "wrong-combat"
zone = 1
ticks = 2
[[bots]]
name = "alice"
[[steps]]
tick = 0
bot = "alice"
action = "join"
[[steps]]
tick = 0
bot = "alice"
action = "select_target"
entity = "npc:8"
[[steps]]
tick = 0
bot = "alice"
action = "swing"
facing = 0
[[expect]]
kind = "event"
bot = "alice"
event = "attack_performed"
tick = 1
[[expect]]
kind = "target"
bot = "alice"
entity = "npc:8"
tick = 1
[[expect]]
kind = "health"
bot = "alice"
health = 49
tick = 1
[[expect]]
kind = "unit"
bot = "alice"
entity = "npc:8"
state = "dead"
tick = 1
[[expect]]
kind = "event"
bot = "alice"
event = "died"
tick = 2
"#,
    )
    .unwrap();
    let text = report.to_text();
    assert_eq!(report.failures(), 3, "{text}");
    assert!(
        text.contains("select_target -> applied seq=1 epoch=1 entity=npc:8"),
        "{text}"
    );
    // A swing never hits the guard (#517): it whiffs.
    assert!(text.contains("swing -> applied seq=2"), "{text}");
    assert!(
        text.contains("expect alice event attack_performed ok"),
        "{text}"
    );
    assert!(text.contains("expect alice target npc:8 ok"), "{text}");
    assert!(text.contains("FAIL got health=50/50"), "{text}");
    assert!(
        text.contains("FAIL got L10 hp100% (-2900,90,1500) []"),
        "{text}"
    );
    assert!(text.contains("FAIL got events[]"), "{text}");
}

#[test]
fn unexpected_control_plane_outcomes_fail_the_scenario() {
    let report = run_control_plane(
        r#"
name = "wrong"
lease_ttl_ticks = 10
heartbeat_ttl_ticks = 10
[[steps]]
at = 0
op = "assign"
zone = 1
host = "host-a"
"#,
    )
    .unwrap();
    assert!(!report.passed());
    assert!(report.to_text().contains("-> rejected:host_not_registered"));
}

#[test]
fn invalid_scenarios_are_rejected_at_load() {
    for (text, message) in [
        (
            "name = \"x\"\nzone = 1\nticks = 0\n[[bots]]\nname = \"a\"",
            "ticks",
        ),
        ("name = \"x\"\nzone = 1\nticks = 2\nbots = []", "bots"),
        (
            "name = \"x\"\nzone = 1\nticks = 2\n[[bots]]\nname = \"a\"\n[[steps]]\ntick = 0\nbot = \"b\"\naction = \"join\"",
            "unknown bot",
        ),
        (
            "name = \"x\"\nzone = 1\nticks = 2\n[[bots]]\nname = \"a\"\n[[steps]]\ntick = 0\nbot = \"a\"\naction = \"move\"",
            "forward and strafe are required for move",
        ),
        (
            "name = \"x\"\nzone = 1\nticks = 2\n[[bots]]\nname = \"a\"\n[[steps]]\ntick = 0\nbot = \"a\"\naction = \"move\"\nforward = 1\nstrafe = 0",
            "exactly one of facing and entity",
        ),
        (
            "name = \"x\"\nzone = 1\nticks = 2\n[[bots]]\nname = \"a\"\n[[steps]]\ntick = 0\nbot = \"a\"\naction = \"join\"\nfacing = 0",
            "facing only for move and swing",
        ),
        (
            "name = \"x\"\nzone = 1\nticks = 2\n[[bots]]\nname = \"a\"\n[[steps]]\ntick = 0\nbot = \"a\"\naction = \"move\"\nx = 1\nz = 0",
            "unknown field",
        ),
        (
            "name = \"x\"\nzone = 1\nticks = 2\ntypo = 1\n[[bots]]\nname = \"a\"",
            "unknown field",
        ),
        (
            "name = \"x\"\nzone = 1\nticks = 2\n[[bots]]\nname = \"a\"\n[[steps]]\ntick = 0\nbot = \"a\"\naction = \"jump\"\nfacing = 0",
            "facing only for move and swing",
        ),
        (
            "name = \"x\"\nzone = 1\nticks = 2\n[[bots]]\nname = \"a\"\n[[steps]]\ntick = 0\nbot = \"a\"\naction = \"disconnect\"\nseq = 2",
            "apply only to commands",
        ),
        (
            "name = \"x\"\nzone = 1\nticks = 2\n[[bots]]\nname = \"a\"\n[[expect]]\nkind = \"area\"\nbot = \"a\"\ntick = 1",
            "missing its required field",
        ),
        (
            "name = \"x\"\nzone = 1\nticks = 2\n[[bots]]\nname = \"a\"\n[[steps]]\ntick = 0\nbot = \"a\"\naction = \"select_target\"",
            "entity is required for select_target",
        ),
        (
            "name = \"x\"\nzone = 1\nticks = 2\n[[bots]]\nname = \"a\"\n[[steps]]\ntick = 0\nbot = \"a\"\naction = \"jump\"\nentity = \"creature:1\"",
            "allowed only for it, move, swing and use_ability",
        ),
        (
            "name = \"x\"\nzone = 1\nticks = 2\n[[bots]]\nname = \"a\"\n[[steps]]\ntick = 0\nbot = \"a\"\naction = \"jump\"\nuntil = 2\nevery = 1",
            "until and every repeat only a swing",
        ),
        (
            "name = \"x\"\nzone = 1\nticks = 2\n[[bots]]\nname = \"a\"\n[[steps]]\ntick = 0\nbot = \"a\"\naction = \"select_target\"\nentity = \"wolf:1\"",
            "must be none, bot:<name>, creature:<id> or npc:<id>",
        ),
        (
            "name = \"x\"\nzone = 1\nticks = 2\n[[bots]]\nname = \"a\"\n[[steps]]\ntick = 0\nbot = \"a\"\naction = \"select_target\"\nentity = \"bot:b\"",
            "unknown bot b",
        ),
        (
            "name = \"x\"\nzone = 1\nticks = 2\n[[bots]]\nname = \"a\"\n[[expect]]\nkind = \"unit\"\nbot = \"a\"\nentity = \"none\"\nstate = \"alive\"\ntick = 1",
            "missing its required field",
        ),
        (
            "name = \"x\"\nzone = 1\nticks = 2\n[[bots]]\nname = \"a\"\n[[expect]]\nkind = \"event\"\nbot = \"a\"\nevent = \"error:typo\"\ntick = 1",
            "must be damage_dealt",
        ),
        (
            "name = \"x\"\nzone = 1\nticks = 2\n[[bots]]\nname = \"a\"\n[[expect]]\nkind = \"health\"\nbot = \"a\"\nhealth = 50\nby_tick = 1",
            "by_tick only for sees, event, unit and resource",
        ),
        (
            "name = \"x\"\nzone = 1\nticks = 2\n[[bots]]\nname = \"a\"\n[[steps]]\ntick = 0\nbot = \"a\"\naction = \"choose_class\"\nclass = \"warden\"",
            "class and sex are required only for choose_class",
        ),
        (
            "name = \"x\"\nzone = 1\nticks = 2\n[[bots]]\nname = \"a\"\n[[steps]]\ntick = 0\nbot = \"a\"\naction = \"use_ability\"",
            "ability is required only for use_ability",
        ),
        (
            "name = \"x\"\nzone = 1\nticks = 2\n[[bots]]\nname = \"a\"\n[[steps]]\ntick = 0\nbot = \"a\"\naction = \"cancel_cast\"\nability = 1",
            "ability is required only for use_ability",
        ),
        (
            "name = \"x\"\nzone = 1\nticks = 2\n[[bots]]\nname = \"a\"\n[[expect]]\nkind = \"resource\"\nbot = \"a\"\ntick = 1",
            "missing its required field",
        ),
        (
            "name = \"x\"\nzone = 1\nticks = 2\n[[bots]]\nname = \"a\"\n[[expect]]\nkind = \"chat\"\nbot = \"a\"\ntarget = \"a\"\ntext = \"hi\"\nemote = \"wavve\"\ntick = 1",
            "missing its required field",
        ),
        (
            "name = \"x\"\nzone = 1\nticks = 2\n[[bots]]\nname = \"a\"\n[[expect]]\nkind = \"chat\"\nbot = \"a\"\ntarget = \"a\"\ntext = \"hi\"\nemote = \"wave\"\ntick = 1",
            "missing its required field",
        ),
        (
            "name = \"x\"\nzone = 1\nticks = 2\n[[bots]]\nname = \"a\"\n[[steps]]\ntick = 0\nbot = \"a\"\naction = \"accept_quest\"\nquest = 1",
            "npc is required only for",
        ),
        (
            "name = \"x\"\nzone = 1\nticks = 2\n[[bots]]\nname = \"a\"\n[[steps]]\ntick = 0\nbot = \"a\"\naction = \"abandon_quest\"\nquest = 1\nchoice = 0",
            "choice is required only for complete_quest",
        ),
        (
            "name = \"x\"\nzone = 1\nticks = 2\n[[bots]]\nname = \"a\"\n[[steps]]\ntick = 0\nbot = \"a\"\naction = \"complete_quest\"\nnpc = 1\nchoice = 0",
            "quest is required only for",
        ),
        (
            "name = \"x\"\nzone = 1\nticks = 2\n[[bots]]\nname = \"a\"\n[[expect]]\nkind = \"quest\"\nbot = \"a\"\nquest = 1\nquest_state = \"absent\"\nprogress = [0, 0, 0]\ntick = 1",
            "missing its required field",
        ),
        (
            "name = \"x\"\nzone = 1\nticks = 2\n[[bots]]\nname = \"a\"\n[[expect]]\nkind = \"marker\"\nbot = \"a\"\nnpc = 1\nmarker = \"!\"\ntick = 1",
            "missing its required field",
        ),
    ] {
        let error = bots::load(text).unwrap_err();
        assert!(error.contains(message), "{error}");
    }
    let error = control_plane::load(
        "name = \"x\"\nlease_ttl_ticks = 1\nheartbeat_ttl_ticks = 1\n[[steps]]\nat = 5\nop = \"expire_hosts\"\n[[steps]]\nat = 4\nop = \"expire_hosts\"",
    )
    .unwrap_err();
    assert!(error.contains("backwards"), "{error}");
}

fn cli(arguments: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_mmorpg-scenario"))
        .args(arguments)
        .output()
        .unwrap()
}

#[test]
fn cli_reports_pass_fail_and_usage_through_exit_codes() {
    let passing = support::scenario_files("control-plane")[0].clone();
    let passing = passing.to_str().unwrap();
    let output = cli(&["control-plane", "--json", passing]);
    assert_eq!(output.status.code(), Some(0));
    let expected = run_control_plane(&std::fs::read_to_string(passing).unwrap())
        .unwrap()
        .to_json_lines();
    assert_eq!(String::from_utf8(output.stdout).unwrap(), expected);

    let failing = Path::new(env!("CARGO_TARGET_TMPDIR")).join("failing-scenario.toml");
    std::fs::write(
        &failing,
        "name = \"f\"\nlease_ttl_ticks = 1\nheartbeat_ttl_ticks = 1\n[[steps]]\nat = 0\nop = \"heartbeat\"\nhost = \"h\"\n",
    )
    .unwrap();
    let output = cli(&["control-plane", failing.to_str().unwrap()]);
    assert_eq!(output.status.code(), Some(1));

    assert_eq!(cli(&["bots", "missing.toml"]).status.code(), Some(2));
    assert_eq!(cli(&["nonsense", passing]).status.code(), Some(2));
    assert_eq!(cli(&[]).status.code(), Some(2));
}
