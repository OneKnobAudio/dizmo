use dizmo::kit::{Choke, DizmoKit, KitError, MidiMap};
use std::path::Path;

fn fixture_dir() -> &'static Path {
    Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/kit"))
}

#[test]
fn loads_a_complete_kit() {
    let kit = DizmoKit::load(fixture_dir().join("drumkit.xml")).unwrap();

    assert_eq!(kit.version, "2.1.0");
    assert_eq!(kit.name, "Test Kit");
    assert_eq!(kit.description, "Fixture kit for tests");
    assert_eq!(kit.samplerate, 48000.0);
    assert_eq!(kit.default_midimap.as_deref(), Some("midimap.xml"));
    assert_eq!(kit.root_dir, fixture_dir());

    let names: Vec<&str> = kit
        .channels
        .iter()
        .map(|channel| channel.name.as_str())
        .collect();
    assert_eq!(names, ["AmbL", "AmbR", "Kick", "Snare", "Hihat"]);
    assert_eq!(kit.channels[2].num, 2);

    assert_eq!(kit.instruments.len(), 4);
    let names: Vec<&str> = kit
        .instruments
        .iter()
        .map(|instrument| instrument.name.as_str())
        .collect();
    assert_eq!(names, ["Kick", "Snare", "HihatClosed", "HihatOpen"]);
    for (id, instrument) in kit.instruments.iter().enumerate() {
        assert_eq!(instrument.id, id);
    }

    // base_dir points at the instrument files' directory.
    assert_eq!(kit.instruments[0].base_dir, fixture_dir());
}

#[test]
fn resolves_channel_maps() {
    let kit = DizmoKit::load(fixture_dir().join("drumkit.xml")).unwrap();

    let kick = &kit.instruments[0];
    assert_eq!(kick.channel_map.len(), 3);
    assert_eq!(kick.channel_map[0].in_name, "Kick");
    assert_eq!(kick.channel_map[0].out_name, "Kick");
    assert!(kick.channel_map[0].is_main);

    let hihat = &kit.instruments[2];
    assert_eq!(hihat.channel_map[0].out_name, "Hihat");
    assert!(hihat.channel_map[0].is_main);
}

#[test]
fn resolves_groups_and_chokes() {
    let kit = DizmoKit::load(fixture_dir().join("drumkit.xml")).unwrap();

    let closed = &kit.instruments[2];
    let open = &kit.instruments[3];

    assert_eq!(closed.group.as_deref(), Some("hihat"));
    assert_eq!(open.group.as_deref(), Some("hihat"));

    assert_eq!(
        closed.chokes,
        vec![Choke {
            instrument: "HihatOpen".to_string(),
            choketime_ms: 100
        }]
    );
    assert!(open.chokes.is_empty());
}

#[test]
fn parses_v2_samples_with_power() {
    let kit = DizmoKit::load(fixture_dir().join("drumkit.xml")).unwrap();
    let kick = &kit.instruments[0];

    assert!(kick.is_v2());
    assert_eq!(kick.samples.len(), 3);

    let softest = &kick.samples[0];
    assert_eq!(softest.name, "Kick-1");
    assert_eq!(softest.power, 0.00833794);
    assert!(!softest.normalized);
    assert_eq!(softest.audio_files.len(), 3);

    // filechannel is 1-based in XML, stored 0-based.
    assert_eq!(softest.audio_files[0].channel, "AmbL");
    assert_eq!(softest.audio_files[0].file, "samples/kick.wav");
    assert_eq!(softest.audio_files[0].file_channel, 0);
    assert_eq!(softest.audio_files[2].file_channel, 2);

    // Velocity layers: samples sorted by ascending power.
    let powers: Vec<f32> = kick
        .samples_by_power()
        .iter()
        .map(|sample| sample.power)
        .collect();
    assert_eq!(powers, [0.00833794, 0.05, 0.09]);
}

#[test]
fn parses_normalized_flag() {
    let kit = DizmoKit::load(fixture_dir().join("drumkit.xml")).unwrap();
    let snare = &kit.instruments[1];

    assert!(snare.samples[0].normalized);
    assert!(!snare.samples[1].normalized);
    assert_eq!(snare.samples[0].audio_files[0].file_channel, 0);
}

#[test]
fn parses_v1_velocity_groups() {
    let kit = DizmoKit::load(fixture_dir().join("drumkit.xml")).unwrap();
    let hihat = &kit.instruments[2];

    assert!(!hihat.is_v2());
    assert_eq!(hihat.samples.len(), 2);
    assert_eq!(hihat.samples[0].power, 0.0);

    assert_eq!(hihat.velocities.len(), 2);
    assert_eq!(hihat.velocities[0].lower, 0.0);
    assert_eq!(hihat.velocities[0].upper, 0.5);
    assert_eq!(hihat.velocities[0].sample_refs.len(), 2);
    assert_eq!(hihat.velocities[0].sample_refs[0].name, "HC-1");
    assert_eq!(hihat.velocities[0].sample_refs[0].probability, 0.8);
    assert_eq!(hihat.velocities[1].sample_refs.len(), 1);
}

#[test]
fn loads_midimap() {
    let kit = DizmoKit::load(fixture_dir().join("drumkit.xml")).unwrap();
    let midimap: MidiMap = kit.load_midimap("midimap.xml").unwrap();

    assert_eq!(midimap.entries.len(), 4);
    assert_eq!(midimap.instrument_for_note(35), Some("Kick"));
    assert_eq!(midimap.instrument_for_note(46), Some("HihatOpen"));
    assert_eq!(midimap.instrument_for_note(60), None);
}

#[test]
fn errors_on_missing_file() {
    let error = DizmoKit::load(fixture_dir().join("does-not-exist.xml")).unwrap_err();
    assert!(matches!(error, KitError::Io { .. }), "got {error:?}");
}

#[test]
fn errors_on_malformed_drumkit_xml() {
    let kit_dir = std::env::temp_dir().join("dizmo-malformed-drumkit");
    std::fs::create_dir_all(&kit_dir).unwrap();

    let drumkit = kit_dir.join("drumkit.xml");
    std::fs::write(&drumkit, "<drumkit><instruments>").unwrap();

    let error = DizmoKit::load(&drumkit).unwrap_err();
    assert!(matches!(error, KitError::Parse { .. }), "got {error:?}");

    std::fs::remove_dir_all(&kit_dir).ok();
}

#[test]
fn errors_on_malformed_instrument_xml() {
    let kit_dir = std::env::temp_dir().join("dizmo-malformed-instrument");
    std::fs::create_dir_all(&kit_dir).unwrap();

    let drumkit = kit_dir.join("drumkit.xml");
    let instrument = kit_dir.join("inst_broken.xml");
    std::fs::write(&drumkit, r#"<drumkit><instruments><instrument name="Broken" file="inst_broken.xml"/></instruments></drumkit>"#).unwrap();
    std::fs::write(&instrument, "<instrument><name>").unwrap();

    let error = DizmoKit::load(&drumkit).unwrap_err();
    assert!(matches!(error, KitError::Parse { .. }), "got {error:?}");

    std::fs::remove_dir_all(&kit_dir).ok();
}

#[test]
fn errors_on_missing_required_attribute() {
    let kit_dir = std::env::temp_dir().join("dizmo-missing-attr");
    std::fs::create_dir_all(&kit_dir).unwrap();

    let drumkit = kit_dir.join("drumkit.xml");
    let instrument = kit_dir.join("inst_bad.xml");
    std::fs::write(&drumkit, r#"<drumkit><instruments><instrument name="Bad" file="inst_bad.xml"/></instruments></drumkit>"#).unwrap();
    std::fs::write(&instrument, r#"<instrument version="2.0"><samples><sample name="S" power="0.1"/></samples></instrument>"#).unwrap();

    let error = DizmoKit::load(&drumkit).unwrap_err();
    assert!(matches!(error, KitError::Missing { .. }), "got {error:?}");

    std::fs::remove_dir_all(&kit_dir).ok();
}

#[test]
fn looks_up_instruments_by_name() {
    let kit = DizmoKit::load(fixture_dir().join("drumkit.xml")).unwrap();
    assert_eq!(kit.instrument("Snare").unwrap().id, 1);
    assert!(kit.instrument("Cowbell").is_none());
}

#[test]
fn rejects_out_of_range_midi_notes() {
    let kit = DizmoKit::load(fixture_dir().join("drumkit.xml")).unwrap();
    let kit_dir = std::env::temp_dir().join("dizmo-bad-note");
    std::fs::create_dir_all(&kit_dir).unwrap();
    let path = kit_dir.join("midimap.xml");
    std::fs::write(
        &path,
        r#"<midimap><map note="200" instr="Kick"/></midimap>"#,
    )
    .unwrap();
    let error = kit.load_midimap(&path).unwrap_err();
    assert!(matches!(error, KitError::Invalid { .. }), "got {error:?}");
    std::fs::remove_dir_all(&kit_dir).ok();
}

#[test]
fn supports_old_drumkit_attributes() {
    let kit_dir = std::env::temp_dir().join("dizmo-old-format");
    std::fs::create_dir_all(&kit_dir).unwrap();

    let drumkit = kit_dir.join("drumkit.xml");
    let instrument = kit_dir.join("inst_snare.xml");
    std::fs::write(
        &drumkit,
        r#"<drumkit name="Old Kit" description="Legacy">
  <channels><channel name="Snare"/></channels>
  <instruments>
    <instrument name="Snare" file="inst_snare.xml">
      <channelmap in="Snare" out="Snare"/>
    </instrument>
  </instruments>
</drumkit>"#,
    )
    .unwrap();
    std::fs::write(
        &instrument,
        r#"<instrument version="2.0" name="Snare">
  <samples><sample name="S-1" power="0.1">
    <audiofile channel="Snare" file="s.wav" filechannel="1"/>
  </sample></samples>
</instrument>"#,
    )
    .unwrap();

    let kit = DizmoKit::load(&drumkit).unwrap();
    assert_eq!(kit.name, "Old Kit");
    assert_eq!(kit.description, "Legacy");
    assert_eq!(kit.samplerate, 44100.0);

    std::fs::remove_dir_all(&kit_dir).ok();
}

/// A minimal instrument file, enough for `Kit::load` to succeed.
const MINIMAL_INST: &str = r#"<instrument version="2.0" name="Kick">
  <samples><sample name="K-1" power="0.1">
    <audiofile channel="Kick" file="kick.wav" filechannel="1"/>
  </sample></samples>
</instrument>"#;

/// Writes a minimal kit under `name` (no declared `defaultmidimap`) and loads it.
fn load_minimal_kit(tag: &str, name: &str) -> DizmoKit {
    let dir = std::env::temp_dir().join(format!("dizmo-midimap-{tag}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join(name),
        r#"<drumkit><instruments><instrument name="Kick" file="inst_kick.xml"/></instruments></drumkit>"#,
    )
    .unwrap();
    std::fs::write(dir.join("inst_kick.xml"), MINIMAL_INST).unwrap();
    let kit = DizmoKit::load(dir.join(name)).unwrap();
    std::fs::remove_dir_all(&dir).ok();
    kit
}

#[test]
fn detects_midimap_from_kit_filename_variation() {
    let kit = load_minimal_kit("convention", "CrocellKit_full.xml");
    assert_eq!(kit.default_midimap.as_deref(), Some("Midimap_full.xml"));
}

#[test]
fn midimap_candidates_try_both_letter_cases() {
    let kit = load_minimal_kit("candidates", "CrocellKit_full.xml");
    assert_eq!(
        kit.default_midimap_candidates(),
        vec![
            "Midimap_full.xml".to_string(),
            "midimap_full.xml".to_string()
        ]
    );

    let kit = load_minimal_kit("candidates-plain", "drumkit.xml");
    assert_eq!(
        kit.default_midimap_candidates(),
        vec!["midimap.xml".to_string(), "Midimap.xml".to_string()]
    );
}

#[test]
fn detects_midimap_from_numeric_variation() {
    let kit = load_minimal_kit("numeric", "Muldjord_2.xml");
    assert_eq!(kit.default_midimap.as_deref(), Some("Midimap_2.xml"));
}

#[test]
fn detects_plain_midimap_without_variation() {
    let kit = load_minimal_kit("plain", "drumkit.xml");
    assert_eq!(kit.default_midimap.as_deref(), Some("midimap.xml"));
}

#[test]
fn does_not_detect_midimap_for_degenerate_underscore() {
    let kit = load_minimal_kit("trailing", "Kit_.xml");
    assert_eq!(kit.default_midimap, None);
}

#[test]
fn explicit_defaultmidimap_wins_over_convention() {
    let dir = std::env::temp_dir().join("dizmo-midimap-explicit");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("SomeKit_7.xml"),
        r#"<drumkit version="2.0">
  <metadata><defaultmidimap src="custom.xml"/></metadata>
  <instruments><instrument name="Kick" file="inst_kick.xml"/></instruments>
</drumkit>"#,
    )
    .unwrap();
    std::fs::write(dir.join("inst_kick.xml"), MINIMAL_INST).unwrap();

    let kit = DizmoKit::load(dir.join("SomeKit_7.xml")).unwrap();
    assert_eq!(kit.default_midimap.as_deref(), Some("custom.xml"));

    std::fs::remove_dir_all(&dir).ok();
}
