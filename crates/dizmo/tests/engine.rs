use std::path::{Path, PathBuf};
use std::sync::Arc;

use dizmo::engine::{Engine, MAX_VOICES, load_engine};
use dizmo::kit::{Kit, MidiMap, SampleBank, load_samples};

const DRUMKIT: &str = r#"<drumkit version="2.0">
  <metadata>
    <title>Test Kit</title>
    <description>Engine test fixtures</description>
    <defaultmidimap src="midimap.xml"/>
  </metadata>
  <channels>
    <channel name="Kick"/>
  </channels>
  <instruments>
    <instrument name="Kick" file="inst_kick.xml">
      <channelmap in="Kick" out="Kick" main="true"/>
    </instrument>
  </instruments>
</drumkit>
"#;

const INST_KICK: &str = r#"<instrument version="2.0" name="Kick">
  <samples>
    <sample name="Kick-1" power="0.1">
      <audiofile channel="Kick" file="kick.wav" filechannel="1"/>
    </sample>
  </samples>
</instrument>
"#;

const MIDIMAP: &str = r#"<midimap>
  <map note="36" instr="Kick"/>
</midimap>
"#;

const OPEN_INST: &str = r#"<instrument version="2.0" name="Open">
  <samples>
    <sample name="Open-1" power="0.1">
      <audiofile channel="Open" file="open.wav" filechannel="1"/>
    </sample>
  </samples>
</instrument>
"#;

const CLOSED_INST: &str = r#"<instrument version="2.0" name="Closed">
  <samples>
    <sample name="Closed-1" power="0.1">
      <audiofile channel="Closed" file="closed.wav" filechannel="1"/>
    </sample>
  </samples>
</instrument>
"#;

fn approx(left: f32, right: f32) -> bool {
    (left - right).abs() < 1e-6
}

fn write_wav(path: &Path, channels: u16, samples: &[i16]) {
    let spec = hound::WavSpec {
        channels,
        sample_rate: 44100,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut writer = hound::WavWriter::create(path, spec).unwrap();
    for &sample in samples {
        writer.write_sample(sample).unwrap();
    }
    writer.finalize().unwrap();
}

fn write_file(dir: &Path, name: &str, content: &str) {
    std::fs::write(dir.join(name), content).unwrap();
}

fn write_wavs(dir: &Path, wavs: &[(&str, u16, &[i16])]) {
    for (name, channels, samples) in wavs {
        write_wav(&dir.join(name), *channels, samples);
    }
}

fn setup(tag: &str, drumkit: &str, xml: &[(&str, &str)], wavs: &[(&str, u16, &[i16])]) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("dizmo-engine-{tag}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    write_file(&dir, "drumkit.xml", drumkit);
    for (name, content) in xml {
        write_file(&dir, name, content);
    }
    write_wavs(&dir, wavs);
    dir
}

fn load(dir: &Path) -> (Arc<Kit>, Arc<SampleBank>, MidiMap) {
    let kit = Arc::new(Kit::load(dir.join("drumkit.xml")).unwrap());
    let bank = Arc::new(load_samples(&kit, None).unwrap());
    let midimap = kit.load_midimap("midimap.xml").unwrap();
    (kit, bank, midimap)
}

fn run(engine: &mut Engine, channels: usize, frames: usize) -> Vec<Vec<f32>> {
    let mut buffers: Vec<Vec<f32>> = vec![vec![0.0; frames]; channels];
    engine.process(0, frames, &mut buffers);
    buffers
}

#[test]
fn plays_sample_into_its_output_channel() {
    let dir = setup(
        "plays",
        DRUMKIT,
        &[("inst_kick.xml", INST_KICK), ("midimap.xml", MIDIMAP)],
        &[("kick.wav", 1, &[1000, 2000, 3000])],
    );
    let (kit, bank, midimap) = load(&dir);
    let mut engine = Engine::new(kit, bank, midimap);

    engine.note_on(36, 127);
    assert_eq!(engine.active_voices(), 1);

    let out = run(&mut engine, 1, 10);

    let kick = &out[0];
    assert!(approx(kick[0], 1000.0 / 32768.0));
    assert!(approx(kick[1], 2000.0 / 32768.0));
    assert!(approx(kick[2], 3000.0 / 32768.0));
    assert_eq!(kick[3], 0.0);
    assert_eq!(engine.active_voices(), 0);
}

const TWO_CHANNEL_DRUMKIT: &str = r#"<drumkit version="2.0">
  <metadata>
    <title>Two Channel Kit</title>
    <defaultmidimap src="midimap.xml"/>
  </metadata>
  <channels>
    <channel name="Kick"/>
    <channel name="Snare"/>
  </channels>
  <instruments>
    <instrument name="Kick" file="inst_kick.xml">
      <channelmap in="Kick" out="Kick" main="true"/>
    </instrument>
    <instrument name="Snare" file="inst_snare.xml">
      <channelmap in="Snare" out="Snare" main="true"/>
    </instrument>
  </instruments>
</drumkit>
"#;

const INST_SNARE: &str = r#"<instrument version="2.0" name="Snare">
  <samples>
    <sample name="Snare-1" power="0.1">
      <audiofile channel="Snare" file="snare.wav" filechannel="1"/>
    </sample>
  </samples>
</instrument>
"#;

const TWO_CHANNEL_MIDIMAP: &str = r#"<midimap>
  <map note="35" instr="Kick"/>
  <map note="36" instr="Kick"/>
  <map note="38" instr="Snare"/>
</midimap>
"#;

#[test]
fn maps_instruments_to_their_output_channels() {
    let dir = setup(
        "instruments-per-channel",
        TWO_CHANNEL_DRUMKIT,
        &[
            ("inst_kick.xml", INST_KICK),
            ("inst_snare.xml", INST_SNARE),
            ("midimap.xml", TWO_CHANNEL_MIDIMAP),
        ],
        &[
            ("kick.wav", 1, &[1000, 2000]),
            ("snare.wav", 1, &[4000, 5000]),
        ],
    );
    let (kit, bank, midimap) = load(&dir);
    let engine = Engine::new(kit, bank, midimap);

    assert_eq!(
        engine.instruments_per_channel(),
        vec![Some("Kick".to_string()), Some("Snare".to_string())]
    );

    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn channel_without_main_instrument_is_unmapped() {
    let drumkit = r#"<drumkit version="2.0">
  <metadata><title>T</title><description>d</description></metadata>
  <channels>
    <channel name="Kick"/>
    <channel name="Room"/>
  </channels>
  <instruments>
    <instrument name="Kick" file="inst_kick.xml">
      <channelmap in="Kick" out="Kick" main="true"/>
      <channelmap in="Kick" out="Room"/>
    </instrument>
  </instruments>
</drumkit>
"#;
    let dir = setup(
        "instruments-per-channel-main-only",
        drumkit,
        &[("inst_kick.xml", INST_KICK), ("midimap.xml", MIDIMAP)],
        &[("kick.wav", 1, &[1000, 2000])],
    );
    let (kit, bank, midimap) = load(&dir);
    let engine = Engine::new(kit, bank, midimap);

    // "Room" only has a non-main bleed channelmap entry, so it gets no
    // instrument; only the main "Kick" output is mapped.
    assert_eq!(
        engine.instruments_per_channel(),
        vec![Some("Kick".to_string()), None]
    );

    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn selects_velocity_layer_by_power() {
    let dir = setup(
        "velocity",
        DRUMKIT,
        &[("inst_kick.xml", INST_KICK), ("midimap.xml", MIDIMAP)],
        &[("kick1.wav", 1, &[10, 20]), ("kick2.wav", 1, &[100, 200])],
    );
    let inst = r#"<instrument version="2.0" name="Kick">
  <samples>
    <sample name="Kick-1" power="0.05">
      <audiofile channel="Kick" file="kick1.wav" filechannel="1"/>
    </sample>
    <sample name="Kick-2" power="0.9">
      <audiofile channel="Kick" file="kick2.wav" filechannel="1"/>
    </sample>
  </samples>
</instrument>
"#;
    std::fs::write(dir.join("inst_kick.xml"), inst).unwrap();

    let (kit, bank, midimap) = load(&dir);

    let mut soft = Engine::new(kit.clone(), bank.clone(), midimap.clone());
    soft.note_on(36, 32);
    let out = run(&mut soft, 1, 4);
    assert!(approx(out[0][0], 10.0 / 32768.0));

    let mut loud = Engine::new(kit, bank, midimap);
    loud.note_on(36, 127);
    let out = run(&mut loud, 1, 4);
    assert!(approx(out[0][0], 100.0 / 32768.0));
}

#[test]
fn choke_cuts_target_instantly_when_choketime_is_zero() {
    let drumkit = r#"<drumkit version="2.0">
  <metadata><title>T</title><description>d</description><defaultmidimap src="midimap.xml"/></metadata>
  <channels>
    <channel name="Open"/>
    <channel name="Closed"/>
  </channels>
  <instruments>
    <instrument name="Open" file="inst_open.xml">
      <channelmap in="Open" out="Open" main="true"/>
    </instrument>
    <instrument name="Closed" file="inst_closed.xml">
      <chokes><choke instrument="Open" choketime="0"/></chokes>
      <channelmap in="Closed" out="Closed" main="true"/>
    </instrument>
  </instruments>
</drumkit>
"#;
    let midimap = r#"<midimap>
  <map note="42" instr="Closed"/>
  <map note="46" instr="Open"/>
</midimap>
"#;
    let dir = setup(
        "choke-instant",
        drumkit,
        &[
            ("inst_open.xml", OPEN_INST),
            ("inst_closed.xml", CLOSED_INST),
            ("midimap.xml", midimap),
        ],
        &[
            ("open.wav", 1, &[111, 222, 333]),
            ("closed.wav", 1, &[777, 888, 999]),
        ],
    );
    let (kit, bank, midimap) = load(&dir);
    let mut engine = Engine::new(kit, bank, midimap);

    engine.note_on(46, 127);
    let out = run(&mut engine, 2, 2);
    assert!(approx(out[0][0], 111.0 / 32768.0));
    assert!(approx(out[0][1], 222.0 / 32768.0));

    engine.note_on(42, 127);
    let out = run(&mut engine, 2, 2);
    assert_eq!(out[0][0], 0.0);
    assert_eq!(out[0][1], 0.0);
    assert!(approx(out[1][0], 777.0 / 32768.0));
    assert_eq!(engine.active_voices(), 1);
}

#[test]
fn choke_fades_target_over_choketime() {
    let drumkit = r#"<drumkit version="2.0">
  <metadata><title>T</title><description>d</description><defaultmidimap src="midimap.xml"/></metadata>
  <channels>
    <channel name="Open"/>
    <channel name="Closed"/>
  </channels>
  <instruments>
    <instrument name="Open" file="inst_open.xml">
      <channelmap in="Open" out="Open" main="true"/>
    </instrument>
    <instrument name="Closed" file="inst_closed.xml">
      <chokes><choke instrument="Open" choketime="2"/></chokes>
      <channelmap in="Closed" out="Closed" main="true"/>
    </instrument>
  </instruments>
</drumkit>
"#;
    let midimap = r#"<midimap>
  <map note="42" instr="Closed"/>
  <map note="46" instr="Open"/>
</midimap>
"#;
    let dir = setup(
        "choke-fade",
        drumkit,
        &[
            ("inst_open.xml", OPEN_INST),
            ("inst_closed.xml", CLOSED_INST),
            ("midimap.xml", midimap),
        ],
        &[
            ("open.wav", 1, &[100, 200, 300, 400, 500]),
            ("closed.wav", 1, &[777, 888, 999]),
        ],
    );
    let (kit, bank, midimap) = load(&dir);
    let mut engine = Engine::new(kit, bank, midimap);
    // 1000 Hz sample rate => a 2 ms choke is exactly 2 frames long.
    engine.set_sample_rate(1000.0);

    engine.note_on(46, 127);
    let out = run(&mut engine, 2, 2);
    assert!(approx(out[0][0], 100.0 / 32768.0));
    assert!(approx(out[0][1], 200.0 / 32768.0));

    engine.note_on(42, 127);
    let out = run(&mut engine, 2, 2);
    assert!(approx(out[0][0], 300.0 / 32768.0));
    assert!(approx(out[0][1], 200.0 / 32768.0)); // 400 * 0.5
    assert_eq!(engine.active_voices(), 1);

    let out = run(&mut engine, 2, 2);
    assert_eq!(out[0][0], 0.0);
    assert!(approx(out[1][0], 999.0 / 32768.0));
    assert_eq!(engine.active_voices(), 0);
}

#[test]
fn retrigger_fades_previous_voice_instead_of_cutting() {
    let dir = setup(
        "retrigger",
        DRUMKIT,
        &[("inst_kick.xml", INST_KICK), ("midimap.xml", MIDIMAP)],
        &[("kick.wav", 1, &[1000, 2000, 3000, 4000, 4000, 4000])],
    );
    let (kit, bank, midimap) = load(&dir);
    let mut engine = Engine::new(kit, bank, midimap);
    // 2 kHz sample rate => the 1 ms retrigger fade is exactly 2 frames.
    engine.set_sample_rate(2000.0);

    engine.note_on(36, 127);
    let out = run(&mut engine, 1, 1);
    assert!(approx(out[0][0], 1000.0 / 32768.0));
    assert_eq!(engine.active_voices(), 1);

    engine.note_on(36, 127);
    let out = run(&mut engine, 1, 1);
    // The fresh voice restarts from position 0; the previous voice is not cut
    // but rings on its first fade frame (still full gain).
    assert!(approx(out[0][0], (2000.0 + 1000.0) / 32768.0));
    assert_eq!(engine.active_voices(), 2);

    let out = run(&mut engine, 1, 4);
    // The previous voice fades out over its 2 fade frames (gains 0.5, 0.0)
    // while the fresh voice plays s[1..5] in full.
    assert!(approx(out[0][0], (3000.0 * 0.5 + 2000.0) / 32768.0));
    assert!(approx(out[0][1], 3000.0 / 32768.0));
    assert!(approx(out[0][2], 4000.0 / 32768.0));
    assert!(approx(out[0][3], 4000.0 / 32768.0));
    assert_eq!(engine.active_voices(), 1);
}

#[test]
fn mid_block_retrigger_restarts_sample_from_beginning() {
    let dir = setup(
        "mid-block-retrigger",
        DRUMKIT,
        &[("inst_kick.xml", INST_KICK), ("midimap.xml", MIDIMAP)],
        &[("kick.wav", 1, &[1000, 2000, 3000, 4000])],
    );
    let (kit, bank, midimap) = load(&dir);
    let mut engine = Engine::new(kit, bank, midimap);
    let mut buffers = vec![vec![0.0; 8]; 1];

    // First hit at frame 1: sample renders from s[0].
    engine.process(0, 1, &mut buffers);
    engine.note_on(36, 127);
    engine.process(1, 2, &mut buffers);
    assert!(approx(buffers[0][1], 1000.0 / 32768.0));
    assert!(approx(buffers[0][2], 2000.0 / 32768.0));

    // Retrigger at frame 4: the fresh voice restarts from s[0], while the
    // previous voice plays out its one remaining frame (s[3]) at full gain on
    // the first fade frame instead of being cut.
    engine.process(3, 1, &mut buffers);
    engine.note_on(36, 127);
    engine.process(4, 2, &mut buffers);
    assert!(approx(buffers[0][4], 5000.0 / 32768.0));
    assert!(approx(buffers[0][5], 2000.0 / 32768.0));
    assert_eq!(engine.active_voices(), 1);
}

#[test]
fn ignores_unmapped_notes() {
    let dir = setup(
        "unmapped",
        DRUMKIT,
        &[("inst_kick.xml", INST_KICK), ("midimap.xml", MIDIMAP)],
        &[("kick.wav", 1, &[1000])],
    );
    let (kit, bank, midimap) = load(&dir);
    let mut engine = Engine::new(kit, bank, midimap);

    engine.note_on(60, 127);
    assert_eq!(engine.active_voices(), 0);

    let out = run(&mut engine, 1, 4);
    assert!(out[0].iter().all(|&sample| sample == 0.0));
}

#[test]
fn all_notes_off_fades_voices_out_instead_of_cutting() {
    let dir = setup(
        "all-off",
        DRUMKIT,
        &[("inst_kick.xml", INST_KICK), ("midimap.xml", MIDIMAP)],
        &[("kick.wav", 1, &[1000; 8])],
    );
    let (kit, bank, midimap) = load(&dir);
    let mut engine = Engine::new(kit, bank, midimap);
    // 1 kHz sample rate => the 5 ms fade is exactly 5 frames.
    engine.set_sample_rate(1000.0);

    engine.note_on(36, 127);
    assert_eq!(engine.active_voices(), 1);

    engine.all_notes_off();
    // Voices are faded out, not cut, so they linger briefly.
    assert_eq!(engine.active_voices(), 1);

    let out = run(&mut engine, 1, 4);
    assert!(approx(out[0][0], 1000.0 / 32768.0));
    assert!(approx(out[0][1], 800.0 / 32768.0));
    assert!(approx(out[0][2], 600.0 / 32768.0));
    assert!(approx(out[0][3], 400.0 / 32768.0));
    assert_eq!(engine.active_voices(), 1);

    let out = run(&mut engine, 1, 2);
    assert!(approx(out[0][0], 200.0 / 32768.0));
    assert_eq!(out[0][1], 0.0);
    assert_eq!(engine.active_voices(), 0);
}

#[test]
fn voice_stealing_fades_the_oldest_instead_of_cutting() {
    let dir = setup(
        "steal",
        DRUMKIT,
        &[("inst_kick.xml", INST_KICK), ("midimap.xml", MIDIMAP)],
        &[("kick.wav", 1, &[1000; 256])],
    );
    let (kit, bank, midimap) = load(&dir);
    let mut engine = Engine::new(kit, bank, midimap);

    for _ in 0..=MAX_VOICES {
        engine.note_on(36, 127);
    }
    // The MAX_VOICES+1-th note fades the oldest voice instead of dropping it,
    // so all MAX_VOICES+1 voices are still present right after the hit.
    assert_eq!(engine.active_voices(), MAX_VOICES + 1);

    // Frame 0: every voice, including the oldest, rings at full gain. An
    // instant cut would have silenced the stolen voice here.
    let out = run(&mut engine, 1, 1);
    assert!(approx(
        out[0][0],
        (MAX_VOICES + 1) as f32 * 1000.0 / 32768.0
    ));
}

#[test]
fn resampled_kit_plays_at_target_rate() {
    let dir = setup(
        "resampled",
        DRUMKIT,
        &[("inst_kick.xml", INST_KICK), ("midimap.xml", MIDIMAP)],
        &[("kick.wav", 1, &[1000; 4])],
    );
    let mut engine = load_engine(dir.join("drumkit.xml"), Some(22050)).unwrap();

    engine.note_on(36, 127);
    let out = run(&mut engine, 1, 4);
    // 4 frames at 44.1 kHz were resampled down to 2 frames at 22.05 kHz.
    assert!(approx(out[0][0], 1000.0 / 32768.0));
    assert!(approx(out[0][1], 1000.0 / 32768.0));
    assert_eq!(out[0][2], 0.0);
    assert_eq!(engine.active_voices(), 0);
}

#[test]
fn note_triggered_mid_block_renders_at_its_offset() {
    let dir = setup(
        "mid-block",
        DRUMKIT,
        &[("inst_kick.xml", INST_KICK), ("midimap.xml", MIDIMAP)],
        &[("kick.wav", 1, &[1000, 2000, 3000, 4000])],
    );
    let (kit, bank, midimap) = load(&dir);
    let mut engine = Engine::new(kit, bank, midimap);

    let mut buffers = vec![vec![0.0; 8]; 1];

    // Pre-roll: nothing is ringing, the first 3 frames stay silent.
    engine.process(0, 3, &mut buffers);
    // The note arrives at frame 3 of this block.
    engine.note_on(36, 127);
    engine.process(3, 5, &mut buffers);

    assert_eq!(buffers[0][0], 0.0);
    assert_eq!(buffers[0][1], 0.0);
    assert_eq!(buffers[0][2], 0.0);
    // The sample starts exactly at the note's offset, not at the block start.
    assert!(approx(buffers[0][3], 1000.0 / 32768.0));
    assert!(approx(buffers[0][4], 2000.0 / 32768.0));
    assert!(approx(buffers[0][5], 3000.0 / 32768.0));
}

#[test]
fn split_block_rendering_matches_contiguous_rendering() {
    let dir = setup(
        "split",
        DRUMKIT,
        &[("inst_kick.xml", INST_KICK), ("midimap.xml", MIDIMAP)],
        &[("kick.wav", 1, &[1000, 2000, 3000, 4000, 5000, 6000])],
    );
    let (kit, bank, midimap) = load(&dir);
    let mut contiguous = Engine::new(Arc::clone(&kit), Arc::clone(&bank), midimap.clone());
    let mut split = Engine::new(kit, bank, midimap);

    // Contiguous: the note plays from frame 0 of a single block.
    contiguous.note_on(36, 127);
    let mut out_a = vec![vec![0.0; 6]; 1];
    contiguous.process(0, 6, &mut out_a);

    // Split: the note arrives at frame 2 and is rendered as sub-blocks.
    let mut out_b = vec![vec![0.0; 6]; 1];
    split.process(0, 2, &mut out_b);
    split.note_on(36, 127);
    split.process(2, 2, &mut out_b);
    split.process(4, 2, &mut out_b);

    assert_eq!(out_b[0][0], 0.0);
    assert_eq!(out_b[0][1], 0.0);
    for (index, (expected, actual)) in out_a[0].iter().zip(&out_b[0][2..]).enumerate() {
        assert!(
            approx(*expected, *actual),
            "frame {index}: {expected} != {actual}"
        );
    }
}

/// A drumkit without a declared `<defaultmidimap>`.
const CONVENTION_DRUMKIT: &str = r#"<drumkit version="2.0">
  <metadata><title>Convention Kit</title></metadata>
  <channels><channel name="Kick"/></channels>
  <instruments>
    <instrument name="Kick" file="inst_kick.xml">
      <channelmap in="Kick" out="Kick" main="true"/>
    </instrument>
  </instruments>
</drumkit>
"#;

#[test]
fn load_engine_picks_up_convention_midimap() {
    // `TestKit_2.xml` + `Midimap_2.xml`: the midimap is detected from the kit
    // filename variation and applied, so note 36 plays the Kick.
    let dir = std::env::temp_dir().join("dizmo-engine-convention-midimap");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    write_file(&dir, "TestKit_2.xml", CONVENTION_DRUMKIT);
    write_file(&dir, "inst_kick.xml", INST_KICK);
    write_file(&dir, "Midimap_2.xml", MIDIMAP);
    write_wavs(&dir, &[("kick.wav", 1, &[1000, 2000, 3000])]);

    let mut engine = load_engine(dir.join("TestKit_2.xml"), None).unwrap();
    engine.note_on(36, 127);
    assert_eq!(engine.active_voices(), 1);

    let out = run(&mut engine, 1, 3);
    assert!(approx(out[0][0], 1000.0 / 32768.0));

    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn load_engine_picks_up_lowercase_convention_midimap() {
    // `TestKit_4.xml` pairs with `Midimap_4.xml` by convention, but the kit
    // ships the midimap lowercase (`midimap_4.xml`); the case fallback must
    // still map note 36.
    let dir = std::env::temp_dir().join("dizmo-engine-lowercase-midimap");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    write_file(&dir, "TestKit_4.xml", CONVENTION_DRUMKIT);
    write_file(&dir, "inst_kick.xml", INST_KICK);
    write_file(&dir, "midimap_4.xml", MIDIMAP);
    write_wavs(&dir, &[("kick.wav", 1, &[1000, 2000, 3000])]);

    let mut engine = load_engine(dir.join("TestKit_4.xml"), None).unwrap();
    engine.note_on(36, 127);
    assert_eq!(engine.active_voices(), 1);

    let out = run(&mut engine, 1, 3);
    assert!(approx(out[0][0], 1000.0 / 32768.0));

    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn load_engine_tolerates_missing_convention_midimap() {
    // `TestKit_3.xml` with no `Midimap_3.xml`: the kit still loads, just
    // unmapped, instead of failing the whole load.
    let dir = std::env::temp_dir().join("dizmo-engine-missing-midimap");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    write_file(&dir, "TestKit_3.xml", CONVENTION_DRUMKIT);
    write_file(&dir, "inst_kick.xml", INST_KICK);
    write_wavs(&dir, &[("kick.wav", 1, &[1000, 2000, 3000])]);

    let mut engine = load_engine(dir.join("TestKit_3.xml"), None).unwrap();
    engine.note_on(36, 127);
    assert_eq!(engine.active_voices(), 0);

    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn load_engine_picks_up_plain_midimap_without_variation() {
    // A kit without a variation (`drumkit.xml`) pairs with the plain
    // `midimap.xml`, so note 36 plays the Kick.
    let dir = std::env::temp_dir().join("dizmo-engine-plain-midimap");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    write_file(&dir, "drumkit.xml", CONVENTION_DRUMKIT);
    write_file(&dir, "inst_kick.xml", INST_KICK);
    write_file(&dir, "midimap.xml", MIDIMAP);
    write_wavs(&dir, &[("kick.wav", 1, &[1000, 2000, 3000])]);

    let mut engine = load_engine(dir.join("drumkit.xml"), None).unwrap();
    engine.note_on(36, 127);
    assert_eq!(engine.active_voices(), 1);

    let out = run(&mut engine, 1, 3);
    assert!(approx(out[0][0], 1000.0 / 32768.0));

    std::fs::remove_dir_all(&dir).ok();
}
