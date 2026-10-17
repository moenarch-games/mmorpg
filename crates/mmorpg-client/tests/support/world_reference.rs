use serde_json::{Value, json};
use std::{
    io::Write,
    path::PathBuf,
    process::{Command, Stdio},
    time::Instant,
};

fn requests() -> Vec<Value> {
    let mut cases = Vec::new();
    for (name, eye) in [
        ("outpost", [0.0, 15.0, 35.0]),
        ("woods", [-95.0, 12.0, -20.0]),
        ("farm", [90.0, 18.0, 20.0]),
        ("lake", [0.0, 15.0, 125.0]),
        ("hollow", [0.0, 15.0, -150.0]),
        ("far", [600.0, 40.0, 600.0]),
    ] {
        cases.push(json!({"name":format!("region:{name}"),"kind":"world","seconds":0,"animate":true,"eye":eye}));
    }
    for seconds in [0, 300, 600, 900, 1200] {
        cases.push(json!({"name":format!("day:{seconds}"),"kind":"world","seconds":seconds,"animate":true,"eye":[0,15,35]}));
    }
    for seconds in [0, 1, 600] {
        cases.push(json!({"name":format!("frozen:{seconds}"),"kind":"world","seconds":seconds,"animate":false,"eye":[0,15,35]}));
    }
    let player = entity("player", 1, 1, false);
    for (class, class_id) in ["warden", "ranger", "arcanist"].into_iter().enumerate() {
        for (sex_code, sex) in ["female", "male"].into_iter().enumerate() {
            for headwear in [
                None,
                Some("ironcrest-helm"),
                Some("ranger-cap"),
                Some("wayfarer-hood"),
            ] {
                let mut own = player.clone();
                own["appearance"] = json!(1 + class * 2 + sex_code);
                cases.push(json!({"name":format!("look:{class_id}:{sex}:{headwear:?}"),"kind":"units","classId":class_id,"sex":sex,
                    "headwear":headwear,"entities":[own],"animate":false}));
            }
        }
    }
    let catalog: Value = serde_json::from_str(mmorpg_wasm::catalog::hosted_catalog_json()).unwrap();
    let content = mmorpg_core::greyhaven_vale::content();
    for template in catalog["creatureTemplates"].as_array().unwrap() {
        let template_id = template["id"].as_u64().unwrap();
        let spawn = content
            .creature_spawns()
            .iter()
            .find(|spawn| u64::from(spawn.template.get()) == template_id)
            .unwrap();
        for state in ["alive", "tapped", "selected", "dead"] {
            let dead = state == "dead";
            let mut creature = entity("creature", u64::from(spawn.id.get()), template_id, dead);
            creature["position"][1] = template["halfExtents"][1].clone();
            creature["flags"]["hostile"] = json!(template["behaviour"] == "aggressive");
            creature["flags"]["attackable"] = json!(!dead);
            creature["flags"]["tappedByOther"] = json!(state == "tapped");
            let target = if state == "selected" {
                json!({"kind":"creature","id":spawn.id.get()})
            } else {
                Value::Null
            };
            cases.push(json!({"name":format!("creature:{template_id}:{state}"),"kind":"units","classId":"warden","sex":"female",
                "headwear":null,"entities":[player.clone(),creature],"target":target,"animate":false}));
        }
    }
    for npc in catalog["npcs"].as_array().unwrap() {
        let id = npc["id"].as_u64().unwrap();
        let mut record = entity("npc", id, id, false);
        record["level"] = npc["level"].clone();
        cases.push(json!({"name":format!("npc:{id}"),"kind":"units","classId":"warden","sex":"female","headwear":null,
            "entities":[player.clone(),record],"animate":false}));
    }
    for appearance in 0..7 {
        cases.push(json!({"name":format!("remote:{appearance}"),"kind":"units","classId":"warden","sex":"female","headwear":null,
            "entities":[player.clone(),entity("player",2,appearance,false)],"animate":false}));
    }
    for (name, velocity, y, facing) in [
        ("walk", [0, 0, 20], 90, 0),
        ("jump", [0, 8, 10], 140, 0),
        ("turn", [20, 0, 0], 90, 16384),
    ] {
        for class_id in ["warden", "ranger", "arcanist"] {
            let mut own = viewer(class_id);
            own["velocity"] = json!(velocity);
            own["position"][1] = json!(y);
            own["facing"] = json!(facing);
            for animate in [true, false] {
                cases.push(json!({"name":format!("motion:{name}:{class_id}:{animate}"),"kind":"units","classId":class_id,"sex":"female","headwear":null,
                    "entities":[own.clone()],"animate":animate,"sampleSeconds":0.1,"delta":0.1}));
            }
        }
    }
    let target = json!({"kind":"creature","id":100});
    let mut wolf = entity("creature", 100, 1, false);
    wolf["position"] = json!([300, 45, 0]);
    for ability in 1..=12 {
        let class_id = if ability <= 4 {
            "warden"
        } else if ability <= 8 {
            "ranger"
        } else {
            "arcanist"
        };
        for animate in [true, false] {
            cases.push(json!({"name":format!("ability:{ability}:{animate}"),"kind":"units","classId":class_id,"sex":"female","headwear":null,
                "entities":[viewer(class_id),wolf.clone()],"animate":animate,"target":target.clone(),"sampleSeconds":0.15,"delta":0.15,
                "events":[{"kind":"ability-used","source":{"kind":"player","id":1},"target":target.clone(),"ability":ability}]}));
        }
    }
    for (name, ability, channel) in [
        ("cast", 9, false),
        ("channel", 12, true),
        ("interrupted", 9, false),
        ("release", 9, false),
    ] {
        let cast = json!({"ability":ability,"elapsed":30,"total":60,"channel":channel});
        let mut case = json!({"name":format!("action:{name}"),"kind":"units","classId":"arcanist","sex":"female","headwear":null,
            "entities":[viewer("arcanist"),wolf.clone()],"animate":true,"target":target.clone(),"sampleSeconds":0.15,"delta":0.15,"cast":cast});
        if name == "interrupted" || name == "release" {
            case["previousCast"] = case["cast"].clone();
            case["cast"] = Value::Null;
            case["events"] = if name == "release" {
                json!([{"kind":"ability-used","source":{"kind":"player","id":1},"target":target.clone(),"ability":9}])
            } else {
                json!([{"kind":"interrupted","source":{"kind":"creature","id":100},"target":{"kind":"player","id":1},"ability":9}])
            };
        }
        cases.push(case);
    }
    cases.push(json!({"name":"barrier-aura","kind":"units","classId":"arcanist","sex":"female","headwear":null,
        "entities":[viewer("arcanist")],"animate":true,"auras":[{"ability":11,"kind":"absorb","remaining":60,"amount":20}],"sampleSeconds":0.15,"delta":0.15}));
    cases.push(json!({"name":"expired-ability","kind":"units","classId":"arcanist","sex":"female","headwear":null,
        "entities":[viewer("arcanist"),wolf.clone()],"animate":true,"target":target.clone(),"sampleSeconds":2.0,"delta":0.25,
        "events":[{"kind":"ability-used","source":{"kind":"player","id":1},"target":target.clone(),"ability":9}]}));
    cases.push(json!({"name":"paused-projected-cast","kind":"units","classId":"arcanist","sex":"female","headwear":null,
        "entities":[viewer("arcanist"),wolf.clone()],"animate":true,"target":target.clone(),"sampleSeconds":30.0,"delta":0.25,
        "cast":{"ability":9,"elapsed":30,"total":60,"channel":false}}));
    cases
}

fn viewer(class: &str) -> Value {
    entity(
        "player",
        1,
        match class {
            "warden" => 1,
            "ranger" => 3,
            "arcanist" => 5,
            _ => panic!("bad fixture class"),
        },
        false,
    )
}

fn entity(kind: &str, id: u64, appearance: u64, dead: bool) -> Value {
    json!({"kind":kind,"entityId":id,"appearance":appearance,"position":[0,90,0],"velocity":[0,0,0],"facing":0,"level":1,"healthPercent":if dead{0}else{100},
        "flags":{"dead":dead,"inCombat":false,"hostile":false,"attackable":false,"tappedByOther":false,"evading":false,"targetsViewer":false,"lootable":false}})
}

fn export(cases: &[Value]) -> Value {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    // Exactly the shared WASM reliefAt backing function used by the real browser.
    let mut relief_samples = std::collections::BTreeMap::new();
    for case in cases {
        if let Some(entities) = case["entities"].as_array() {
            for entity in entities {
                let x: i32 = entity["position"][0].as_i64().unwrap().try_into().unwrap();
                let z: i32 = entity["position"][2].as_i64().unwrap().try_into().unwrap();
                relief_samples.insert(format!("{x},{z}"), mmorpg_wasm::scenery::relief_at(x, z));
            }
        }
    }
    let input = json!({"scenery":serde_json::from_str::<Value>(mmorpg_wasm::scenery::hosted_scenery_json()).unwrap(),
        "catalog":serde_json::from_str::<Value>(mmorpg_wasm::catalog::hosted_catalog_json()).unwrap(),"reliefSamples":relief_samples,"requests":cases});
    let started = Instant::now();
    let mut child = Command::new("bun")
        .arg("tests/support/native-world-reference.ts")
        .current_dir(root.join("web"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(input.to_string().as_bytes())
        .unwrap();
    let result = child.wait_with_output().unwrap();
    assert!(
        result.status.success(),
        "Pages reference exporter failed: {}",
        result.status
    );
    eprintln!(
        "Pages reference: {} bytes for {} cases in {:?}",
        result.stdout.len(),
        cases.len(),
        started.elapsed()
    );
    serde_json::from_slice(&result.stdout).unwrap()
}

pub fn shared() -> &'static (Vec<Value>, Value) {
    static REFERENCE: std::sync::OnceLock<(Vec<Value>, Value)> = std::sync::OnceLock::new();
    REFERENCE.get_or_init(|| {
        let cases = requests();
        let output = export(&cases);
        (cases, output)
    })
}
