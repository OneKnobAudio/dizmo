use super::*;
use crate::model::Move;
use crate::model::load::load;
use dizmo_kit::DizmoKit;

fn fixture_drumkit() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("dizmo_kit")
        .join("tests")
        .join("fixtures")
        .join("kit")
        .join("drumkit.xml")
}

fn write_wav(path: &Path) {
    write_wav_channels(path, 1);
}

fn write_wav_channels(path: &Path, channels: u16) {
    let spec = hound::WavSpec {
        channels,
        sample_rate: 48000,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut writer = hound::WavWriter::create(path, spec).unwrap();
    for _ in 0..100 {
        for _ in 0..channels {
            writer.write_sample(0i16).unwrap();
        }
    }
    writer.finalize().unwrap();
}

#[test]
fn save_normalizes_layout_copies_and_round_trips() {
    let root = std::env::temp_dir().join(format!("dizmo_editor_save_test_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let _ = std::fs::create_dir_all(&root);
    let external = root.parent().unwrap().join("outside.wav");
    write_wav(&external);

    let mut kit = EditorKit::new_kit("Save Test", 48000.0, &["Kick".into()]);
    kit.root_dir = Some(root.clone());

    let kick = kit.add_instrument("Kick Drum");
    kit.toggle_channel_assignment(kick, "Kick");
    kit.import_sample(kick, &external, false).unwrap();

    let snare = kit.add_instrument("Snare");
    kit.toggle_channel_assignment(snare, "Kick");
    let inside = root.join("inside.wav");
    write_wav(&inside);
    kit.import_sample(snare, &inside, false).unwrap();

    kit.add_note(35);
    let note_row = kit
        .midimap
        .entries
        .iter()
        .position(|e| e.note == 35)
        .unwrap();
    kit.midimap.entries[note_row].instrument = "Kick Drum".into();

    save(&mut kit).unwrap();
    assert!(!kit.dirty);
    assert_eq!(kit.default_midimap.as_deref(), Some("midimap.xml"));

    assert!(root.join("Kick Drum/Kick Drum.xml").exists());
    assert!(root.join("Kick Drum/samples/outside.wav").exists());
    assert!(root.join("Snare/Snare.xml").exists());
    assert!(!root.join("Snare/samples/inside.wav").exists());

    let loaded = DizmoKit::load(root.join("Save Test.xml")).unwrap();
    assert_eq!(loaded.drums.name, "Save Test");
    assert_eq!(loaded.drums.samplerate, 48000.0);
    assert_eq!(loaded.instruments.len(), 2);

    assert_eq!(loaded.instruments[0].name, "Kick Drum");
    assert_eq!(
        loaded.drums.instrument_refs[0].file,
        "Kick Drum/Kick Drum.xml"
    );
    assert_eq!(
        loaded.instruments[0].samples[0].audio_files[0].file,
        "samples/outside.wav"
    );

    assert_eq!(loaded.instruments[1].name, "Snare");
    assert_eq!(loaded.drums.instrument_refs[1].file, "Snare/Snare.xml");
    assert_eq!(
        loaded.instruments[1].samples[0].audio_files[0].file,
        "../inside.wav"
    );

    let midimap = loaded.load_midimap("midimap.xml").unwrap();
    assert_eq!(midimap.instrument_for_note(35), Some("Kick Drum"));

    let _ = std::fs::remove_dir_all(&root);
    let _ = std::fs::remove_file(&external);
}

#[test]
fn save_missing_source_does_not_abort() {
    let root =
        std::env::temp_dir().join(format!("dizmo_editor_save_missing_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);

    let mut kit = EditorKit::new_kit("Ghost Kit", 44100.0, &["A".into()]);
    kit.root_dir = Some(root.clone());
    let index = kit.add_instrument("Ghost");
    kit.toggle_channel_assignment(index, "A");
    // A sample whose source WAV is missing: save must keep the dangling
    // `samples/…` path instead of aborting.
    kit.instruments[index]
        .instrument
        .samples
        .push(dizmo_kit::Sample {
            name: "Ghost-1".into(),
            power: 0.5,
            normalized: true,
            resample: None,
            audio_files: vec![dizmo_kit::AudioFile {
                channel: "A".into(),
                file: "samples/ghost.wav".into(),
                file_channel: Some(0),
            }],
        });

    save(&mut kit).unwrap();

    let loaded = DizmoKit::load(root.join("Ghost Kit.xml")).unwrap();
    assert_eq!(loaded.instruments[0].name, "Ghost");
    assert_eq!(
        loaded.instruments[0].samples[0].audio_files[0].file,
        "samples/ghost.wav"
    );

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn save_rejects_duplicate_instrument_names() {
    let root = std::env::temp_dir().join(format!("dizmo_editor_save_dup_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);

    let mut kit = EditorKit::new_kit("Dup Kit", 44100.0, &["A".into()]);
    kit.root_dir = Some(root.clone());
    kit.add_instrument("Same");
    kit.add_instrument("Same");

    assert!(save(&mut kit).is_err());
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn save_rejects_duplicate_instrument_names_case_insensitively() {
    let root = std::env::temp_dir().join(format!("dizmo_editor_save_dupci_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);

    let mut kit = EditorKit::new_kit("Dup CI Kit", 44100.0, &["A".into()]);
    kit.root_dir = Some(root.clone());
    kit.add_instrument("Kick");
    kit.add_instrument(" kick ");

    assert!(save(&mut kit).is_err());
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn save_rejects_filesystem_unsafe_instrument_names() {
    let root = std::env::temp_dir().join(format!("dizmo_editor_deny_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);

    // Unsafe characters and surrounding whitespace are rejected, not sanitized.
    for bad in ["Kick:Snare!", "Kick/", "Lead?", "kick.", " kick", ""] {
        assert!(
            crate::model::types::validate_file_name(bad).is_err(),
            "'{bad}' should be rejected"
        );
    }
    for good in ["Kick", "Kick Snare", "Tom-1", "Ride_Cymbal"] {
        assert!(
            crate::model::types::validate_file_name(good).is_ok(),
            "'{good}' should be accepted"
        );
    }

    let mut kit = EditorKit::new_kit("Deny Test", 44100.0, &["A".into()]);
    kit.root_dir = Some(root.clone());
    kit.add_instrument("Kick:Snare!");
    assert!(save(&mut kit).is_err(), "unsafe name must abort the save");

    kit.rename_instrument(0, "Kick Snare");
    save(&mut kit).unwrap();
    assert!(root.join("Kick Snare").join("Kick Snare.xml").exists());
    let loaded = DizmoKit::load(root.join("Deny Test.xml")).unwrap();
    assert_eq!(loaded.instruments[0].name, "Kick Snare");
    assert_eq!(
        loaded.drums.instrument_refs[0].file,
        "Kick Snare/Kick Snare.xml"
    );

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn import_sample_feeds_one_file_to_all_channels() {
    let root = std::env::temp_dir().join(format!("dizmo_editor_import_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let _ = std::fs::create_dir_all(&root);

    let mut kit = EditorKit::new_kit(
        "Import Test",
        48000.0,
        &["Kick".into(), "AmbL".into(), "AmbR".into()],
    );
    let inst = kit.add_instrument("Kick");
    kit.toggle_channel_assignment(inst, "Kick");
    kit.toggle_channel_assignment(inst, "AmbL");
    kit.toggle_channel_assignment(inst, "AmbR");

    let wav = root
        .parent()
        .unwrap()
        .join(format!("hit_{}.wav", std::process::id()));
    write_wav_channels(&wav, 2);

    let index = kit.import_sample(inst, &wav, false).unwrap();
    assert!(kit.dirty);
    let sample = &kit.instruments[inst].instrument.samples[index];
    let expected_name = wav.file_stem().unwrap().to_string_lossy();
    assert_eq!(sample.name, expected_name);
    // One row per instrument channel, all pointing at the same WAV, with
    // filechannel cycling through the file's channels: stereo (2 ch)
    // alternates L, R, L across the three channels.
    assert_eq!(sample.audio_files.len(), 3);
    for (i, audio) in sample.audio_files.iter().enumerate() {
        assert_eq!(audio.channel, kit.drumkit.channels[i].name);
        assert_eq!(audio.file_channel, Some(i % 2));
        assert!(Path::new(&audio.file).is_absolute());
    }
    assert!(
        sample
            .audio_files
            .iter()
            .all(|a| a.file == sample.audio_files[0].file)
    );

    // Re-importing the same WAV deduplicates the sample name.
    let index2 = kit.import_sample(inst, &wav, false).unwrap();
    let deduped = format!("{expected_name} (2)");
    assert_eq!(
        kit.instruments[inst].instrument.samples[index2].name,
        deduped
    );

    // Powers are spread evenly across the samples in list order:
    // the i-th of N samples lands in the i-th cell of 0..1.
    let powers: Vec<f32> = kit.instruments[inst]
        .instrument
        .samples
        .iter()
        .map(|s| s.power)
        .collect();
    let expected: Vec<f32> = (0..powers.len())
        .map(|i| (i as f32 + 0.5) / powers.len() as f32)
        .collect();
    for (actual, expected) in powers.iter().zip(&expected) {
        assert!(
            (actual - expected).abs() < 1e-6,
            "power {actual} != expected {expected}"
        );
    }

    // An instrument with no channels cannot import.
    let empty = kit.add_instrument("Empty");
    assert!(kit.import_sample(empty, &wav, false).is_err());

    // Not a WAV is rejected.
    let not_wav = root.join("readme.txt");
    std::fs::write(&not_wav, "not audio").unwrap();
    assert!(kit.import_sample(inst, &not_wav, false).is_err());

    // Save copies the single source once; both samples reference it.
    let wav_name = wav.file_name().unwrap().to_string_lossy().into_owned();
    let copied = format!("samples/{wav_name}");
    kit.root_dir = Some(root.clone());
    save(&mut kit).unwrap();
    let loaded = DizmoKit::load(root.join("Import Test.xml")).unwrap();
    assert!(root.join("Kick").join(&copied).exists());
    for sample in &loaded.instruments[0].samples {
        assert_eq!(sample.audio_files.len(), 3);
        for audio in &sample.audio_files {
            assert_eq!(audio.file, copied);
        }
    }

    // A WAV already inside the kit root is referenced in place, never copied.
    let inside = root.join("inside.wav");
    write_wav_channels(&inside, 1);
    let index3 = kit.import_sample(inst, &inside, false).unwrap();
    save(&mut kit).unwrap();
    let loaded = DizmoKit::load(root.join("Import Test.xml")).unwrap();
    assert!(!root.join("Kick/samples/inside.wav").exists());
    assert_eq!(
        loaded.instruments[0].samples[index3].audio_files[0].file,
        "../inside.wav"
    );

    let _ = std::fs::remove_dir_all(&root);
    let _ = std::fs::remove_file(&wav);
}

#[test]
fn import_sample_mono_file_feeds_channel_zero_to_all_channels() {
    let root = std::env::temp_dir().join(format!("dizmo_editor_mono_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let _ = std::fs::create_dir_all(&root);

    let mut kit = EditorKit::new_kit(
        "Mono Import Test",
        48000.0,
        &["Kick".into(), "AmbL".into(), "AmbR".into()],
    );
    let inst = kit.add_instrument("Kick");
    kit.toggle_channel_assignment(inst, "Kick");
    kit.toggle_channel_assignment(inst, "AmbL");
    kit.toggle_channel_assignment(inst, "AmbR");

    let mono = root.join("mono.wav");
    write_wav_channels(&mono, 1);

    let index = kit.import_sample(inst, &mono, false).unwrap();
    let sample = &kit.instruments[inst].instrument.samples[index];
    assert_eq!(sample.audio_files.len(), 3);
    assert!(
        sample.audio_files.iter().all(|a| a.file_channel == Some(0)),
        "a mono sample must reference file channel 0 on every channel"
    );

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn import_warns_when_a_stereo_sample_has_fewer_than_two_main_channels() {
    let root = std::env::temp_dir().join(format!("dizmo_editor_warn_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let _ = std::fs::create_dir_all(&root);

    let mut kit = EditorKit::new_kit("Warn Test", 48000.0, &["Kick".into(), "Room".into()]);
    let inst = kit.add_instrument("Kick");
    kit.toggle_channel_assignment(inst, "Kick");
    kit.toggle_channel_assignment(inst, "Room");
    // Only "Kick" is main: a stereo sample can only play its left side.
    kit.set_channel_main(inst, "Kick", true);

    let stereo = root.join("stereo.wav");
    write_wav_channels(&stereo, 2);
    kit.import_sample(inst, &stereo, false).unwrap();

    let warnings = kit.take_warnings();
    assert_eq!(warnings.len(), 1, "{warnings:?}");
    assert!(
        warnings[0].contains("not enough channels"),
        "unexpected warning: {}",
        warnings[0]
    );
    assert!(
        warnings[0].contains("defaulting to the left channel"),
        "unexpected warning: {}",
        warnings[0]
    );

    // Mono files never warn, and neither do stereo files once the
    // instrument has two main channels.
    let mono = root.join("mono.wav");
    write_wav_channels(&mono, 1);
    kit.import_sample(inst, &mono, false).unwrap();
    assert!(kit.take_warnings().is_empty());

    kit.set_channel_main(inst, "Room", true);
    kit.import_sample(inst, &stereo, false).unwrap();
    assert!(kit.take_warnings().is_empty());

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn assigning_a_channel_propagates_rows_to_existing_samples() {
    let root = std::env::temp_dir().join(format!("dizmo_editor_prop_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let _ = std::fs::create_dir_all(&root);

    let mut kit = EditorKit::new_kit(
        "Propagate Test",
        48000.0,
        &["Kick".into(), "Snare".into(), "Room".into()],
    );
    let inst = kit.add_instrument("Kit");
    kit.toggle_channel_assignment(inst, "Kick");
    kit.toggle_channel_assignment(inst, "Snare");

    let stereo = root.join("stereo.wav");
    write_wav_channels(&stereo, 2);
    let stereo_sample = kit.import_sample(inst, &stereo, false).unwrap();
    let mono = root.join("mono.wav");
    write_wav_channels(&mono, 1);
    let mono_sample = kit.import_sample(inst, &mono, false).unwrap();

    // Assign a third channel: every existing sample gains a row whose
    // side follows the cycling rule over its primary file.
    kit.toggle_channel_assignment(inst, "Room");

    let stereo_rows = &kit.instruments[inst].instrument.samples[stereo_sample].audio_files;
    assert_eq!(stereo_rows.len(), 3);
    let room = stereo_rows.iter().find(|a| a.channel == "Room").unwrap();
    assert_eq!(room.file, stereo_rows[0].file);
    assert_eq!(
        room.file_channel,
        Some(2 % 2),
        "stereo file cycles back to L"
    );

    let mono_rows = &kit.instruments[inst].instrument.samples[mono_sample].audio_files;
    assert_eq!(mono_rows.len(), 3);
    let room = mono_rows.iter().find(|a| a.channel == "Room").unwrap();
    assert_eq!(
        room.file_channel,
        Some(0),
        "mono file feeds its only channel"
    );

    // Unassigning removes the propagated rows again.
    kit.toggle_channel_assignment(inst, "Room");
    for sample in &kit.instruments[inst].instrument.samples {
        assert_eq!(sample.audio_files.len(), 2);
        assert!(sample.audio_files.iter().all(|a| a.channel != "Room"));
    }

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn toggling_the_active_side_clears_the_assignment_but_keeps_the_file() {
    let root = std::env::temp_dir().join(format!("dizmo_editor_toggle_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let _ = std::fs::create_dir_all(&root);

    let mut kit = EditorKit::new_kit(
        "Toggle Test",
        48000.0,
        &["OverheadL".into(), "OverheadR".into()],
    );
    let inst = kit.add_instrument("Overhead");
    kit.toggle_channel_assignment(inst, "OverheadL");
    kit.toggle_channel_assignment(inst, "OverheadR");

    let stereo = root.join("stereo.wav");
    write_wav_channels(&stereo, 2);
    let sample = kit.import_sample(inst, &stereo, false).unwrap();

    let side = |kit: &EditorKit, channel: &str| {
        kit.instruments[inst].instrument.samples[sample]
            .audio_files
            .iter()
            .find(|a| a.channel == channel)
            .map(|a| a.file_channel)
    };

    // The import starts the rows at L (index 0) and R (index 1).
    assert_eq!(side(&kit, "OverheadL"), Some(Some(0)));
    assert_eq!(side(&kit, "OverheadR"), Some(Some(1)));

    // Clicking R on the row that already plays R removes the side
    // assignment but keeps the file on the row.
    kit.toggle_sample_channel_side(inst, sample, "OverheadR", 1);
    assert_eq!(side(&kit, "OverheadR"), Some(None));
    assert_eq!(
        kit.instruments[inst].instrument.samples[sample]
            .audio_files
            .len(),
        2,
        "the row (and its file) must survive the toggle"
    );

    // Clicking R again re-creates the assignment and its feedback.
    kit.toggle_sample_channel_side(inst, sample, "OverheadR", 1);
    assert_eq!(side(&kit, "OverheadR"), Some(Some(1)));

    // Clicking L on the unassigned row assigns it.
    kit.toggle_sample_channel_side(inst, sample, "OverheadR", 0);
    assert_eq!(side(&kit, "OverheadR"), Some(Some(0)));

    // Picking a file leaves the side unassigned until L/R is clicked.
    let second = root.join("second.wav");
    write_wav_channels(&second, 2);
    kit.set_sample_channel_file(inst, sample, "OverheadL", &second)
        .unwrap();
    assert_eq!(side(&kit, "OverheadL"), Some(None));
    kit.toggle_sample_channel_side(inst, sample, "OverheadL", 1);
    assert_eq!(side(&kit, "OverheadL"), Some(Some(1)));

    // A channel with no row is untouched.
    kit.remove_sample_channel(inst, sample, "OverheadR");
    kit.toggle_sample_channel_side(inst, sample, "OverheadR", 0);
    assert_eq!(side(&kit, "OverheadR"), None);

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn save_resamples_imported_samples_to_the_kit_rate() {
    let root = std::env::temp_dir().join(format!("dizmo_editor_resample_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let _ = std::fs::create_dir_all(&root);

    let mut kit = EditorKit::new_kit("Resample Kit", 48000.0, &["Kick".into()]);
    kit.root_dir = Some(root.clone());
    let inst = kit.add_instrument("Kick");
    kit.toggle_channel_assignment(inst, "Kick");

    // A 22050 Hz source, outside the kit root so it gets copied.
    let wav = root
        .parent()
        .unwrap()
        .join(format!("resample_src_{}.wav", std::process::id()));
    let mut writer = hound::WavWriter::create(
        &wav,
        hound::WavSpec {
            channels: 1,
            sample_rate: 22050,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        },
    )
    .unwrap();
    for i in 0..2205 {
        writer.write_sample((i % 1000) as i16).unwrap();
    }
    writer.finalize().unwrap();

    // Accept the resample: the copy must come out at the kit rate.
    let index = kit.import_sample(inst, &wav, true).unwrap();
    assert_eq!(
        kit.instruments[inst].instrument.samples[index].resample,
        Some(48000)
    );
    save(&mut kit).unwrap();
    assert_eq!(
        kit.instruments[inst].instrument.samples[index].resample,
        None
    );
    let wav_name = wav.file_name().unwrap().to_string_lossy().into_owned();
    let copied = root.join("Kick/samples").join(&wav_name);
    let reader = hound::WavReader::open(&copied).unwrap();
    assert_eq!(reader.spec().sample_rate, 48000);

    // No-resample imports (rate already matches the kit) keep the file at
    // its native rate: the copy is never touched.
    let index2 = kit.import_sample(inst, &wav, false).unwrap();
    assert_eq!(
        kit.instruments[inst].instrument.samples[index2].resample,
        None
    );
    save(&mut kit).unwrap();
    let reader = hound::WavReader::open(&copied).unwrap();
    assert_eq!(reader.spec().sample_rate, 22050);

    let _ = std::fs::remove_dir_all(&root);
    let _ = std::fs::remove_file(&wav);
}

#[test]
fn default_midi_notes_add_the_gm_drum_range_unassigned() {
    let mut kit = EditorKit::new_kit("GM Test", 44100.0, &["Kick".into()]);

    kit.add_default_midi_notes();

    // Notes 35..=81, the GM channel 10 percussion range: 47 notes,
    // sorted, all left unassigned for the user to map.
    assert_eq!(kit.midimap.entries.len(), 81 - 35 + 1);
    for (i, entry) in kit.midimap.entries.iter().enumerate() {
        assert_eq!(entry.note, 35 + i as u8);
        assert!(
            entry.instrument.is_empty(),
            "GM notes are added unassigned: {}",
            entry.instrument
        );
    }

    // Idempotent: a second click does not duplicate rows, and notes the
    // user added manually stay.
    kit.add_note(60);
    kit.add_default_midi_notes();
    assert_eq!(kit.midimap.entries.len(), 81 - 35 + 1);
}

#[test]
fn channel_assignment_and_main_flag_round_trip() {
    let root = std::env::temp_dir().join(format!("dizmo_editor_channels_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let _ = std::fs::create_dir_all(&root);

    let mut kit = EditorKit::new_kit("Channel Test", 44100.0, &["Kick".into(), "Snare".into()]);
    kit.root_dir = Some(root.clone());
    let inst = kit.add_instrument("Kick");

    // New instruments have no assigned channels.
    assert!(kit.instruments[inst].instrument.channels.is_empty());
    assert!(kit.instruments[inst].reference.channel_map.is_empty());

    // Marking an unassigned channel as main assigns it as well.
    kit.set_channel_main(inst, "Kick", true);
    assert_eq!(kit.instruments[inst].instrument.channels.len(), 1);
    assert_eq!(kit.instruments[inst].reference.channel_map.len(), 1);
    assert!(kit.instruments[inst].reference.channel_map[0].is_main);
    assert_eq!(
        kit.instruments[inst].instrument.channels[0].name, "Kick",
        "the instrument channel is created for the new assignment"
    );
    // Unmarking leaves the assignment in place.
    kit.set_channel_main(inst, "Kick", false);
    assert_eq!(kit.instruments[inst].reference.channel_map.len(), 1);
    assert!(!kit.instruments[inst].reference.channel_map[0].is_main);

    // Assign Kick and mark it main.
    kit.toggle_channel_assignment(inst, "Kick");
    kit.set_channel_main(inst, "Kick", true);
    assert_eq!(kit.instruments[inst].instrument.channels.len(), 1);
    assert_eq!(kit.instruments[inst].reference.channel_map.len(), 1);
    assert!(kit.instruments[inst].reference.channel_map[0].is_main);

    // Assigning the same channel again removes it.
    kit.toggle_channel_assignment(inst, "Kick");
    assert!(kit.instruments[inst].instrument.channels.is_empty());
    assert!(kit.instruments[inst].reference.channel_map.is_empty());

    // Assign two channels and save.
    kit.toggle_channel_assignment(inst, "Kick");
    kit.toggle_channel_assignment(inst, "Snare");
    kit.set_channel_main(inst, "Kick", true);

    save(&mut kit).unwrap();
    let loaded = DizmoKit::load(root.join("Channel Test.xml")).unwrap();
    let kick = &loaded.instruments[0];
    assert_eq!(kick.channels.len(), 2);
    assert_eq!(kick.channel_map.len(), 2);
    assert!(
        kick.channel_map
            .iter()
            .any(|m| m.in_name == "Kick" && m.is_main)
    );
    assert!(
        kick.channel_map
            .iter()
            .any(|m| m.in_name == "Snare" && !m.is_main)
    );
    // Main only appears on channelmap, not on instrument channels.
    let inst_xml = std::fs::read_to_string(root.join("Kick/Kick.xml")).unwrap();
    assert!(inst_xml.contains("<channel name=\"Kick\"/>"));
    assert!(!inst_xml.contains("<channel name=\"Kick\" main"));
    // The description attribute is always written (DrumGizmo requires it).
    assert!(inst_xml.contains("name=\"Kick\" description=\"\""));

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn converts_v1_velocity_groups_to_v2_power() {
    use dizmo_kit::{
        AudioFile, Instrument, InstrumentChannel, Sample, VelocityGroup, VelocitySampleRef,
    };

    let mut inst = Instrument {
        name: "HihatClosed".into(),
        description: String::new(),
        metadata: Default::default(),
        version: "1.0".into(),
        id: 0,
        base_dir: PathBuf::new(),
        group: None,
        channel_map: Vec::new(),
        chokes: Vec::new(),
        channels: vec![InstrumentChannel {
            name: "Hihat".into(),
        }],
        samples: vec![
            Sample {
                name: "HC-1".into(),
                power: 0.0,
                normalized: false,
                resample: None,
                audio_files: vec![AudioFile {
                    channel: "Hihat".into(),
                    file: "samples/hc1.wav".into(),
                    file_channel: Some(0),
                }],
            },
            Sample {
                name: "HC-2".into(),
                power: 0.0,
                normalized: false,
                resample: None,
                audio_files: vec![AudioFile {
                    channel: "Hihat".into(),
                    file: "samples/hc2.wav".into(),
                    file_channel: Some(0),
                }],
            },
        ],
        velocities: vec![
            VelocityGroup {
                lower: 0.0,
                upper: 0.5,
                sample_refs: vec![
                    VelocitySampleRef {
                        name: "HC-1".into(),
                        probability: 0.8,
                    },
                    VelocitySampleRef {
                        name: "HC-2".into(),
                        probability: 0.2,
                    },
                ],
            },
            VelocityGroup {
                lower: 0.5,
                upper: 1.0,
                sample_refs: vec![VelocitySampleRef {
                    name: "HC-2".into(),
                    probability: 1.0,
                }],
            },
        ],
    };

    convert_v1_instrument_to_v2(&mut inst);

    assert_eq!(inst.version, "2.0");
    assert!(inst.velocities.is_empty());
    assert_eq!(inst.samples.len(), 2);

    let hc1 = inst.samples.iter().find(|s| s.name == "HC-1").unwrap();
    assert_eq!(hc1.power, 0.25);
    assert!(!hc1.normalized);

    let hc2 = inst.samples.iter().find(|s| s.name == "HC-2").unwrap();
    assert!((hc2.power - 0.666_666_7).abs() < 1e-6);
    assert!(!hc2.normalized);
}

#[test]
fn per_channel_file_assignment_and_sides_round_trip() {
    let root =
        std::env::temp_dir().join(format!("dizmo_editor_channel_files_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let _ = std::fs::create_dir_all(&root);

    let mut kit = EditorKit::new_kit(
        "Stereo Test",
        44100.0,
        &["OverheadL".into(), "OverheadR".into()],
    );
    let inst = kit.add_instrument("Overhead");
    kit.toggle_channel_assignment(inst, "OverheadL");
    kit.toggle_channel_assignment(inst, "OverheadR");

    // Import one mono file: both channels get a row (fast path).
    let mono = root
        .parent()
        .unwrap()
        .join(format!("mono_{}.wav", std::process::id()));
    write_wav_channels(&mono, 1);
    let sample = kit.import_sample(inst, &mono, false).unwrap();
    assert_eq!(
        kit.instruments[inst].instrument.samples[sample]
            .audio_files
            .len(),
        2
    );

    // Task 1: point each channel at its own file (different files per side).
    let l = root
        .parent()
        .unwrap()
        .join(format!("l_{}.wav", std::process::id()));
    let r = root
        .parent()
        .unwrap()
        .join(format!("r_{}.wav", std::process::id()));
    write_wav_channels(&l, 1);
    write_wav_channels(&r, 1);
    assert_eq!(
        kit.set_sample_channel_file(inst, sample, "OverheadL", &l)
            .unwrap(),
        1,
        "returns the file's channel count"
    );
    assert_eq!(
        kit.set_sample_channel_file(inst, sample, "OverheadR", &r)
            .unwrap(),
        1
    );

    // Task 2: set the right side's channel (side index 1, XML filechannel 2).
    // The left row was never given a side: picking a file leaves the side
    // unassigned until the user clicks L or R.
    kit.set_sample_channel_side(inst, sample, "OverheadR", 1);
    let rows = &kit.instruments[inst].instrument.samples[sample].audio_files;
    assert_eq!(
        rows.iter()
            .find(|a| a.channel == "OverheadL")
            .unwrap()
            .file_channel,
        None
    );
    assert_eq!(
        rows.iter()
            .find(|a| a.channel == "OverheadR")
            .unwrap()
            .file_channel,
        Some(1)
    );

    // Removing a row and re-adding it leaves the side unassigned again.
    kit.remove_sample_channel(inst, sample, "OverheadL");
    assert!(
        !kit.instruments[inst].instrument.samples[sample]
            .audio_files
            .iter()
            .any(|a| a.channel == "OverheadL")
    );
    kit.set_sample_channel_file(inst, sample, "OverheadL", &l)
        .unwrap();
    assert_eq!(
        kit.instruments[inst].instrument.samples[sample]
            .audio_files
            .iter()
            .find(|a| a.channel == "OverheadL")
            .unwrap()
            .file_channel,
        None
    );

    // Save + reload: rows and sides survive (relative paths under
    // samples/). The unassigned left row saves as filechannel 1 and
    // reloads as the left channel (Some(0)), matching DrumGizmo's
    // default.
    kit.root_dir = Some(root.clone());
    save(&mut kit).unwrap();
    let loaded = DizmoKit::load(root.join("Stereo Test.xml")).unwrap();
    let rows = &loaded.instruments[0].samples[0].audio_files;
    let l_row = rows.iter().find(|a| a.channel == "OverheadL").unwrap();
    let r_row = rows.iter().find(|a| a.channel == "OverheadR").unwrap();
    assert!(
        l_row
            .file
            .ends_with(&format!("l_{}.wav", std::process::id()))
    );
    assert_eq!(l_row.file_channel, Some(0));
    assert_eq!(r_row.file_channel, Some(1));

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn save_uses_kit_name_as_filename_and_resolves_variation_midimap() {
    let root = std::env::temp_dir().join(format!("dizmo_editor_variation_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let _ = std::fs::create_dir_all(&root);

    let mut kit = EditorKit::new_kit("MyKit_full", 44100.0, &["Kick".into()]);
    kit.root_dir = Some(root.clone());
    kit.kit_file_name = Some("MyKit_full.xml".into());
    kit.add_instrument("Kick");
    kit.add_note(35);
    kit.midimap.entries[0].instrument = "Kick".into();

    save(&mut kit).unwrap();

    assert!(root.join("MyKit_full.xml").exists());
    assert!(root.join("Midimap_full.xml").exists());
    assert!(!root.join("drumkit.xml").exists());
    assert!(!root.join("midimap.xml").exists());

    let loaded = DizmoKit::load(root.join("MyKit_full.xml")).unwrap();
    assert_eq!(loaded.drums.name, "MyKit_full");
    assert_eq!(loaded.default_midimap.as_deref(), Some("Midimap_full.xml"));

    let midimap = loaded.load_midimap("Midimap_full.xml").unwrap();
    assert_eq!(midimap.instrument_for_note(35), Some("Kick"));

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn renaming_the_kit_updates_the_file_name_and_variation_midimap() {
    let root = std::env::temp_dir().join(format!(
        "dizmo_editor_rename_variation_{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    let _ = std::fs::create_dir_all(&root);

    let mut kit = EditorKit::new_kit("MyKit", 44100.0, &["Kick".into()]);
    kit.root_dir = Some(root.clone());
    kit.add_instrument("Kick");
    kit.add_note(35);
    kit.midimap.entries[0].instrument = "Kick".into();

    // Renaming the kit to a variation name must move both the entry point
    // and the midimap onto the variation convention.
    kit.rename_kit("MyKit_full");
    save(&mut kit).unwrap();

    assert!(root.join("MyKit_full.xml").exists());
    assert!(root.join("Midimap_full.xml").exists());
    assert!(!root.join("MyKit.xml").exists());
    assert!(!root.join("midimap.xml").exists());

    let loaded = DizmoKit::load(root.join("MyKit_full.xml")).unwrap();
    assert_eq!(loaded.drums.name, "MyKit_full");
    assert_eq!(loaded.default_midimap.as_deref(), Some("Midimap_full.xml"));

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn fixture_kit_round_trips() {
    let fixture = fixture_drumkit();
    let (mut kit, warning) = load(&fixture).unwrap();
    assert!(
        warning.as_deref().unwrap_or("").contains("HihatClosed"),
        "loading a kit with v1.0 instruments should warn about conversion: {warning:?}"
    );

    let root = std::env::temp_dir().join(format!("dizmo_editor_fixture_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let _ = std::fs::create_dir_all(&root);

    kit.root_dir = Some(root.clone());
    save(&mut kit).unwrap();

    let loaded = DizmoKit::load(root.join("drumkit.xml")).unwrap();
    // The editor rescales v2 powers to 0..1 in memory and maps them back
    // on save, so compare against the untouched fixture's raw powers with
    // a small tolerance for the f32 round trip.
    let original = DizmoKit::load(&fixture).unwrap();

    assert_eq!(loaded.drums.name, kit.drumkit.name);
    assert_eq!(loaded.drums.description, kit.drumkit.description);
    assert_eq!(loaded.drums.samplerate, kit.drumkit.samplerate);
    assert_eq!(loaded.drums.version, kit.drumkit.version);
    assert_eq!(loaded.drums.metadata, kit.drumkit.metadata);
    assert_eq!(loaded.default_midimap, kit.default_midimap);

    assert_eq!(loaded.drums.channels.len(), kit.drumkit.channels.len());
    for (expected, actual) in kit.drumkit.channels.iter().zip(&loaded.drums.channels) {
        assert_eq!(actual.name, expected.name);
        assert_eq!(actual.num, expected.num);
        assert_eq!(actual.title, expected.title);
    }

    assert_eq!(loaded.instruments.len(), kit.instruments.len());
    for (i, (expected, actual)) in kit.instruments.iter().zip(&loaded.instruments).enumerate() {
        assert_eq!(actual.name, expected.reference.name);
        assert_eq!(actual.group, expected.reference.group);
        assert_eq!(actual.channel_map, expected.reference.channel_map);
        assert_eq!(actual.chokes, expected.reference.chokes);

        assert_eq!(actual.version, expected.instrument.version);
        assert_eq!(actual.description, expected.instrument.description);
        assert_eq!(actual.metadata, expected.instrument.metadata);
        assert_eq!(actual.channels, expected.instrument.channels);

        assert_eq!(actual.samples.len(), expected.instrument.samples.len());
        let original_is_v2 = original.instruments[i].is_v2();
        for (j, (expected_sample, actual_sample)) in expected
            .instrument
            .samples
            .iter()
            .zip(&actual.samples)
            .enumerate()
        {
            assert_eq!(actual_sample.name, expected_sample.name);
            if original_is_v2 {
                // The editor model keeps normalized 0..1 powers for v2
                // instruments; the saved file restores the original scale.
                assert!(
                    (0.0..=1.0).contains(&expected_sample.power),
                    "editor power should be normalized: {}",
                    expected_sample.power
                );
                let original_power = original.instruments[i].samples[j].power;
                assert!(
                    (actual_sample.power - original_power).abs() < 1e-4,
                    "power round trip drifted: {} -> {}",
                    original_power,
                    actual_sample.power
                );
            } else {
                // v1 instruments are converted to v2 on save; their
                // powers are the editor's own, written as-is.
                assert_eq!(actual_sample.power, expected_sample.power);
            }
            assert_eq!(actual_sample.normalized, expected_sample.normalized);
            assert_eq!(
                actual_sample.audio_files.len(),
                expected_sample.audio_files.len()
            );
            for (expected_af, actual_af) in expected_sample
                .audio_files
                .iter()
                .zip(&actual_sample.audio_files)
            {
                assert_eq!(actual_af.channel, expected_af.channel);
                assert_eq!(actual_af.file_channel, expected_af.file_channel);
                assert!(
                    actual_af.file.starts_with("samples/"),
                    "audio file should be normalized to samples/: {}",
                    actual_af.file
                );
            }
        }

        assert_eq!(actual.velocities, expected.instrument.velocities);
    }

    let midimap = loaded.load_midimap("midimap.xml").unwrap();
    assert_eq!(midimap.entries, kit.midimap.entries);

    let _ = std::fs::remove_dir_all(&root);
}

/// Writes a minimal v2 kit with a single `Kick` instrument whose samples
/// carry the given powers (DRSKit-style unbounded values are fine). No
/// audio files are needed: the editor loader only parses XML.
fn write_power_kit(root: &Path, powers: &[f32]) {
    std::fs::write(
        root.join("drumkit.xml"),
        r#"<?xml version="1.0" encoding="UTF-8"?>
<drumkit version="2.0" name="Power Kit" description="power">
  <metadata><title>Power Kit</title></metadata>
  <channels>
<channel name="Kick"/>
  </channels>
  <instruments>
<instrument name="Kick" file="Kick/Kick.xml">
  <channelmap in="Kick" out="Kick" main="true"/>
</instrument>
  </instruments>
</drumkit>
"#,
    )
    .unwrap();
    std::fs::create_dir_all(root.join("Kick")).unwrap();
    let mut samples = String::new();
    for (i, power) in powers.iter().enumerate() {
        samples.push_str(&format!(
            "    <sample name=\"Kick-{}\" power=\"{}\">\n      <audiofile channel=\"Kick\" file=\"samples/kick.wav\" filechannel=\"1\"/>\n    </sample>\n",
            i + 1,
            power
        ));
    }
    std::fs::write(
        root.join("Kick/Kick.xml"),
        format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<instrument version=\"2.0\" name=\"Kick\" description=\"kick\">\n  <samples>\n{samples}  </samples>\n</instrument>\n"
        ),
    )
    .unwrap();
}

#[test]
fn rescaling_powers_round_trips_a_drs_kit_style_range() {
    let root = std::env::temp_dir().join(format!("dizmo_editor_rescale_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let _ = std::fs::create_dir_all(&root);

    let powers = [0.037478f32, 0.260193, 2.43585, 37.8792];
    write_power_kit(&root, &powers);

    let (mut kit, _) = load(&root.join("drumkit.xml")).unwrap();
    let inst = &kit.instruments[0];
    assert_eq!(
        inst.power_scale,
        Some(PowerScale {
            min: 0.037478,
            max: 37.8792
        })
    );

    // The editor model holds normalized 0..1 powers, order preserved.
    let span = f64::from(37.8792f32) - f64::from(0.037478f32);
    let normalized: Vec<f32> = inst.instrument.samples.iter().map(|s| s.power).collect();
    assert_eq!(normalized[0], 0.0);
    assert_eq!(normalized[3], 1.0);
    assert!(((normalized[1] as f64) - (0.260193f64 - 0.037478f64) / span).abs() < 1e-6);
    assert!(((normalized[2] as f64) - (2.43585f64 - 0.037478f64) / span).abs() < 1e-6);

    // Editing a power writes the value in the file's own scale: 0.5 on
    // the slider maps to the middle of the original range.
    kit.set_sample_power(0, 0, 0.5);
    kit.root_dir = Some(root.clone());
    save(&mut kit).unwrap();

    let loaded = DizmoKit::load(root.join("drumkit.xml")).unwrap();
    let samples = &loaded.instruments[0].samples;
    let expected_half = 0.037478f64 + 0.5 * span;
    assert!(
        ((samples[0].power as f64) - expected_half).abs() < 1e-3,
        "edited power should land mid-range: {}",
        samples[0].power
    );
    for (i, original) in powers.iter().enumerate().skip(1) {
        assert!(
            ((samples[i].power as f64) - f64::from(*original)).abs() < 1e-3,
            "untouched sample {i} drifted: {} -> {}",
            original,
            samples[i].power
        );
    }

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn equal_powers_are_left_untouched() {
    let root =
        std::env::temp_dir().join(format!("dizmo_editor_equal_power_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let _ = std::fs::create_dir_all(&root);

    write_power_kit(&root, &[0.5, 0.5]);

    let (mut kit, _) = load(&root.join("drumkit.xml")).unwrap();
    assert_eq!(kit.instruments[0].power_scale, None);
    assert!(
        kit.instruments[0]
            .instrument
            .samples
            .iter()
            .all(|s| s.power == 0.5)
    );

    kit.root_dir = Some(root.clone());
    save(&mut kit).unwrap();
    let loaded = DizmoKit::load(root.join("drumkit.xml")).unwrap();
    assert!(loaded.instruments[0].samples.iter().all(|s| s.power == 0.5));

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn move_operations_swap_and_keep_parallel_structures_in_sync() {
    let root = std::env::temp_dir().join(format!("dizmo_editor_moves_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let _ = std::fs::create_dir_all(&root);

    let mut kit = EditorKit::new_kit(
        "Moves",
        48000.0,
        &["Kick".into(), "Snare".into(), "Room".into()],
    );
    let a = kit.add_instrument("A");
    kit.add_instrument("B");
    kit.add_instrument("C");
    kit.toggle_channel_assignment(a, "Kick");
    kit.toggle_channel_assignment(a, "Snare");
    kit.toggle_channel_assignment(a, "Room");

    // Edge moves are no-ops.
    kit.dirty = false;
    assert!(!kit.move_kit_channel(0, Move::Up));
    assert!(!kit.move_kit_channel(2, Move::Down));
    assert!(!kit.move_instrument_channel(a, 0, Move::Up));
    assert!(!kit.move_instrument_channel(a, 2, Move::Down));
    assert!(!kit.move_sample(a, usize::MAX, Move::Up));
    assert!(!kit.dirty, "edge no-ops must not mark the kit dirty");

    // Kit channel move: order swaps, nums renumbered, instrument mappings
    // stay name-based (they follow the channel).
    assert!(kit.move_kit_channel(0, Move::Down));
    assert_eq!(kit.drumkit.channels[0].name, "Snare");
    assert_eq!(kit.drumkit.channels[0].num, 0);
    assert_eq!(kit.drumkit.channels[1].name, "Kick");
    assert_eq!(kit.drumkit.channels[1].num, 1);
    assert_eq!(kit.instruments[0].reference.channel_map[0].out_name, "Kick");
    assert_eq!(kit.instruments[0].instrument.channels[0].name, "Kick");

    // Instrument channel move: channels and channel_map stay parallel, the
    // drumkit reference is synced, and main flags travel.
    let a = 0;
    kit.set_channel_main(a, "Kick", true);
    assert!(kit.move_instrument_channel(a, 0, Move::Down));
    let inst = &kit.instruments[a];
    assert_eq!(inst.instrument.channels[0].name, "Snare");
    assert_eq!(inst.instrument.channels[1].name, "Kick");
    assert_eq!(inst.reference.channel_map[0].out_name, "Snare");
    assert!(!inst.reference.channel_map[0].is_main);
    assert_eq!(inst.reference.channel_map[1].out_name, "Kick");
    assert!(inst.reference.channel_map[1].is_main);
    assert_eq!(
        kit.drumkit.instrument_refs[a].channel_map,
        inst.reference.channel_map
    );

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn moving_a_kit_channel_keeps_instrument_mappings_and_round_trips() {
    let root = std::env::temp_dir().join(format!("dizmo_editor_kit_move_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let _ = std::fs::create_dir_all(&root);

    let mut kit = EditorKit::new_kit("Move Kit", 48000.0, &["Kick".into(), "Snare".into()]);
    kit.root_dir = Some(root.clone());
    let inst = kit.add_instrument("Snare Drum");
    kit.toggle_channel_assignment(inst, "Snare");
    kit.set_channel_main(inst, "Snare", true);

    assert!(kit.move_kit_channel(1, Move::Up));
    assert_eq!(kit.drumkit.channels[0].name, "Snare");
    assert_eq!(kit.drumkit.channels[1].name, "Kick");
    let map = &kit.instruments[inst].reference.channel_map;
    assert_eq!(map.len(), 1);
    assert_eq!(map[0].out_name, "Snare");
    assert!(map[0].is_main);

    save(&mut kit).unwrap();
    let loaded = DizmoKit::load(root.join("Move Kit.xml")).unwrap();
    assert_eq!(loaded.drums.channels[0].name, "Snare");
    assert_eq!(loaded.drums.channels[1].name, "Kick");
    assert_eq!(loaded.instruments[0].channel_map[0].out_name, "Snare");
    assert!(loaded.instruments[0].channel_map[0].is_main);

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn moving_an_instrument_channel_reorders_import_side_cycling() {
    let root = std::env::temp_dir().join(format!("dizmo_editor_ch_move_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let _ = std::fs::create_dir_all(&root);

    let mut kit = EditorKit::new_kit("Chan Move", 48000.0, &["A".into(), "B".into(), "C".into()]);
    let inst = kit.add_instrument("Inst");
    kit.toggle_channel_assignment(inst, "A");
    kit.toggle_channel_assignment(inst, "B");
    kit.toggle_channel_assignment(inst, "C");

    let stereo = root.join("stereo.wav");
    write_wav_channels(&stereo, 2);
    let sample = kit.import_sample(inst, &stereo, false).unwrap();
    let rows = &kit.instruments[inst].instrument.samples[sample].audio_files;
    assert_eq!(rows[0].channel, "A");
    assert_eq!(rows[0].file_channel, Some(0));
    assert_eq!(rows[1].channel, "B");
    assert_eq!(rows[1].file_channel, Some(1));
    assert_eq!(rows[2].channel, "C");
    assert_eq!(rows[2].file_channel, Some(0));

    // Move B to the front: the cycling follows the new order.
    assert!(kit.move_instrument_channel(inst, 1, Move::Up));
    kit.remove_sample(inst, sample);
    let sample = kit.import_sample(inst, &stereo, false).unwrap();
    let rows = &kit.instruments[inst].instrument.samples[sample].audio_files;
    assert_eq!(rows[0].channel, "B");
    assert_eq!(rows[0].file_channel, Some(0));
    assert_eq!(rows[1].channel, "A");
    assert_eq!(rows[1].file_channel, Some(1));
    assert_eq!(rows[2].channel, "C");
    assert_eq!(rows[2].file_channel, Some(0));

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn moving_a_sample_respreads_powers_and_round_trips() {
    let root = std::env::temp_dir().join(format!("dizmo_editor_s_move_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let _ = std::fs::create_dir_all(&root);

    let mut kit = EditorKit::new_kit("Sample Move", 48000.0, &["Kick".into()]);
    kit.root_dir = Some(root.clone());
    let inst = kit.add_instrument("Kick");
    kit.toggle_channel_assignment(inst, "Kick");

    for name in ["a1", "a2", "a3"] {
        let wav = root.join(format!("{name}.wav"));
        write_wav_channels(&wav, 1);
        kit.import_sample(inst, &wav, false).unwrap();
    }
    let powers = |kit: &EditorKit| -> Vec<f32> {
        kit.instruments[inst]
            .instrument
            .samples
            .iter()
            .map(|s| s.power)
            .collect()
    };
    // Import spreads 1/6, 3/6, 5/6 in list order.
    for (i, power) in powers(&kit).iter().enumerate() {
        assert!((power - (i as f32 + 0.5) / 3.0).abs() < 1e-6);
    }

    // Edge move is a no-op and leaves powers untouched.
    assert!(!kit.move_sample(inst, 0, Move::Up));

    // Moving the last sample up re-spreads by the new order: the sample
    // formerly at the loudest slot now sits in the middle.
    assert!(kit.move_sample(inst, 2, Move::Up));
    let samples = &kit.instruments[inst].instrument.samples;
    assert_eq!(samples[0].name, "a1");
    assert_eq!(samples[1].name, "a3");
    assert_eq!(samples[2].name, "a2");
    for (i, power) in powers(&kit).iter().enumerate() {
        assert!((power - (i as f32 + 0.5) / 3.0).abs() < 1e-6);
    }

    // The new order round-trips through save/load.
    save(&mut kit).unwrap();
    let loaded = DizmoKit::load(root.join("Sample Move.xml")).unwrap();
    let names: Vec<&str> = loaded.instruments[0]
        .samples
        .iter()
        .map(|s| s.name.as_str())
        .collect();
    assert_eq!(names, ["a1", "a3", "a2"]);

    let _ = std::fs::remove_dir_all(&root);
}
