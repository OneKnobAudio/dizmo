use std::path::{Path, PathBuf};
use std::sync::Arc;

use dizmo::engine::{Engine, MAX_VOICES, load_engine, load_engine_with_progress};
use dizmo::kit::{DizmoKit, MidiMap};
use dizmo::samples::{SampleBank, load_samples_with_progress};

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

/// Expected gain of a fresh voice's 1 ms fade-in at `frame` (0-based), given
/// the engine sample rate. The ramp starts at `1/attack_frames` on the first
/// frame and reaches full gain after one attack.
fn attack_gain(frame: usize, sample_rate: f32) -> f32 {
    let attack_frames = 1.0 / 1000.0 * sample_rate;
    ((frame as f32 + 1.0) / attack_frames).clamp(0.0, 1.0)
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

fn load(dir: &Path) -> (Arc<DizmoKit>, Arc<SampleBank>, MidiMap) {
    let kit = Arc::new(DizmoKit::load(dir.join("drumkit.xml")).unwrap());
    let bank = Arc::new(load_samples_with_progress(&kit, None, &mut |_, _| {}).unwrap());
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
    // The new voice fades in over 1 ms (44.1 frames at 44.1 kHz), so the
    // first frames ramp up instead of stepping straight to full gain.
    assert!(approx(kick[0], 1000.0 / 32768.0 * attack_gain(0, 44100.0)));
    assert!(approx(kick[1], 2000.0 / 32768.0 * attack_gain(1, 44100.0)));
    assert!(approx(kick[2], 3000.0 / 32768.0 * attack_gain(2, 44100.0)));
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
fn mappings_include_only_the_primary_channel() {
    let drumkit = r#"<drumkit version="2.0">
  <metadata><title>T</title><description>d</description><defaultmidimap src="midimap.xml"/></metadata>
  <channels>
    <channel name="Kick"/>
    <channel name="AmbL"/>
    <channel name="AmbR"/>
    <channel name="Room"/>
  </channels>
  <instruments>
    <instrument name="Kick" file="inst_kick.xml">
      <channelmap in="Kick" out="Kick" main="true"/>
      <channelmap in="AmbL" out="AmbL" main="true"/>
      <channelmap in="AmbR" out="AmbR" main="true"/>
      <channelmap in="Room" out="Room"/>
    </instrument>
  </instruments>
</drumkit>
"#;
    let dir = setup(
        "mappings-primary",
        drumkit,
        &[("inst_kick.xml", INST_KICK), ("midimap.xml", MIDIMAP)],
        &[("kick.wav", 1, &[1000, 2000])],
    );
    let (kit, bank, midimap) = load(&dir);
    let engine = Engine::new(kit, bank, midimap);

    let mappings = engine.mappings();
    assert_eq!(mappings.len(), 1);
    // Only the primary mapping is reported: the main entry whose input
    // channel matches the instrument name, not the ambient mains or bleed.
    assert_eq!(mappings[0].channel_map.len(), 1);
    assert_eq!(mappings[0].channel_map[0].in_name, "Kick");
    assert_eq!(mappings[0].channel_map[0].out_name, "Kick");
    assert!(mappings[0].channel_map[0].is_main);

    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn mappings_fall_back_when_no_main_is_declared() {
    let drumkit = r#"<drumkit version="2.0">
  <metadata><title>T</title><description>d</description><defaultmidimap src="midimap.xml"/></metadata>
  <channels>
    <channel name="Kick"/>
    <channel name="Room"/>
  </channels>
  <instruments>
    <instrument name="Kick" file="inst_kick.xml">
      <channelmap in="Kick" out="Kick"/>
      <channelmap in="Kick" out="Room"/>
    </instrument>
  </instruments>
</drumkit>
"#;
    let dir = setup(
        "mappings-fallback",
        drumkit,
        &[("inst_kick.xml", INST_KICK), ("midimap.xml", MIDIMAP)],
        &[("kick.wav", 1, &[1000, 2000])],
    );
    let (kit, bank, midimap) = load(&dir);
    let engine = Engine::new(kit, bank, midimap);

    let mappings = engine.mappings();
    // Without a `main` channelmap entry, the instrument exposes no channel
    // mapping: only its MIDI notes are reported.
    assert_eq!(mappings[0].channel_map.len(), 0);

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
    assert!(approx(out[0][0], 10.0 / 32768.0 * attack_gain(0, 44100.0)));

    let mut loud = Engine::new(kit, bank, midimap);
    loud.note_on(36, 127);
    let out = run(&mut loud, 1, 4);
    assert!(approx(out[0][0], 100.0 / 32768.0 * attack_gain(0, 44100.0)));
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
    assert!(approx(out[0][0], 111.0 / 32768.0 * attack_gain(0, 44100.0)));
    assert!(approx(out[0][1], 222.0 / 32768.0 * attack_gain(1, 44100.0)));

    engine.note_on(42, 127);
    let out = run(&mut engine, 2, 2);
    assert_eq!(out[0][0], 0.0);
    assert_eq!(out[0][1], 0.0);
    assert!(approx(out[1][0], 777.0 / 32768.0 * attack_gain(0, 44100.0)));
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
    // 2 kHz sample rate => the 1 ms attack and the 1 ms retrigger fade are
    // both exactly 2 frames (gains 0.5 then 1.0; 1.0 then 0.5).
    engine.set_sample_rate(2000.0);

    engine.note_on(36, 127);
    let out = run(&mut engine, 1, 1);
    // The fresh voice's first frame is halfway through its fade-in.
    assert!(approx(out[0][0], 500.0 / 32768.0));
    assert_eq!(engine.active_voices(), 1);

    engine.note_on(36, 127);
    let out = run(&mut engine, 1, 1);
    // The previous voice is at full gain on its first fade frame (s[1]);
    // the fresh voice restarts from position 0, half-way through its fade-in.
    assert!(approx(out[0][0], (2000.0 + 500.0) / 32768.0));
    assert_eq!(engine.active_voices(), 2);

    let out = run(&mut engine, 1, 4);
    // The previous voice fades out over its 2 fade frames (gains 1.0, 0.0)
    // while the fresh voice reaches full gain from frame 1 on.
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
    // 2 kHz sample rate => the 1 ms attack and 1 ms retrigger fade are both
    // exactly 2 frames (gains 0.5 then 1.0; 1.0 then 0.5).
    engine.set_sample_rate(2000.0);
    let mut buffers = vec![vec![0.0; 8]; 1];

    // First hit at frame 1: sample renders from s[0], ramping over its 2-frame
    // attack.
    engine.process(0, 1, &mut buffers);
    engine.note_on(36, 127);
    engine.process(1, 2, &mut buffers);
    assert!(approx(buffers[0][1], 500.0 / 32768.0));
    assert!(approx(buffers[0][2], 2000.0 / 32768.0));

    // Retrigger at frame 4: the fresh voice restarts from s[0] (still in its
    // fade-in), while the previous voice plays out its one remaining frame
    // (s[3]) at full gain on the first fade frame instead of being cut.
    engine.process(3, 1, &mut buffers);
    engine.note_on(36, 127);
    engine.process(4, 2, &mut buffers);
    assert!(approx(buffers[0][4], 4500.0 / 32768.0));
    assert!(approx(buffers[0][5], 2000.0 / 32768.0));
    assert_eq!(engine.active_voices(), 1);
}

#[test]
fn new_voices_fade_in_to_avoid_retrigger_click() {
    // A sample that starts at full amplitude: an instant gain step at note-on
    // would pop. The 1 ms attack ramps it in instead.
    let dir = setup(
        "attack",
        DRUMKIT,
        &[("inst_kick.xml", INST_KICK), ("midimap.xml", MIDIMAP)],
        &[("kick.wav", 1, &[10000; 100])],
    );
    let (kit, bank, midimap) = load(&dir);
    let mut engine = Engine::new(kit, bank, midimap);
    // 2 kHz sample rate => the 1 ms attack is exactly 2 frames (gains 0.5, 1.0).
    engine.set_sample_rate(2000.0);

    engine.note_on(36, 127);
    let out = run(&mut engine, 1, 4);
    assert!(approx(out[0][0], 5000.0 / 32768.0));
    assert!(approx(out[0][1], 10000.0 / 32768.0));
    assert!(approx(out[0][2], 10000.0 / 32768.0));
    assert!(approx(out[0][3], 10000.0 / 32768.0));

    // A rapid retrigger starts a second voice from the same spot while the
    // previous one fades out; both ramp in/out instead of stepping.
    engine.note_on(36, 127);
    let out = run(&mut engine, 1, 2);
    assert!(approx(out[0][0], (10000.0 + 5000.0) / 32768.0));
    assert!(approx(out[0][1], (5000.0 + 10000.0) / 32768.0));
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

    // Frame 0: every voice, including the oldest, rings at the first frame of
    // its attack ramp. An instant cut would have silenced the stolen voice here.
    let out = run(&mut engine, 1, 1);
    assert!(approx(
        out[0][0],
        (MAX_VOICES + 1) as f32 * 1000.0 / 32768.0 * attack_gain(0, 44100.0)
    ));
}

#[test]
fn resampled_kit_plays_at_target_rate() {
    let dir = setup(
        "resampled",
        DRUMKIT,
        &[("inst_kick.xml", INST_KICK), ("midimap.xml", MIDIMAP)],
        &[("kick.wav", 1, &[1000; 256])],
    );
    let mut engine = load_engine(dir.join("drumkit.xml"), Some(22050)).unwrap();
    // The engine plays the resampled data 1:1, so its rate is the target rate.
    engine.set_sample_rate(22050.0);

    engine.note_on(36, 127);
    let out = run(&mut engine, 1, 256);
    // 256 frames at 44.1 kHz were resampled down to 128 frames at 22.05 kHz.
    // The first ~22 frames are the 1 ms attack ramp; the steady-state region
    // is the constant sample. The FFT low-pass resampler is not sample-exact:
    // allow 1% edge ripple around the constant and require exact silence after
    // the sample ends.
    let expected = 1000.0 / 32768.0;
    for &sample in &out[0][22..128] {
        assert!(
            (sample - expected).abs() < expected * 0.01,
            "resampled frame {sample} deviates too far from {expected}"
        );
    }
    assert!(
        out[0][127] != 0.0,
        "the resampled sample should be 128 frames long"
    );
    assert!(out[0][128..].iter().all(|&sample| sample == 0.0));
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
    // The sample starts exactly at the note's offset, ramping in over 1 ms.
    assert!(approx(
        buffers[0][3],
        1000.0 / 32768.0 * attack_gain(0, 44100.0)
    ));
    assert!(approx(
        buffers[0][4],
        2000.0 / 32768.0 * attack_gain(1, 44100.0)
    ));
    assert!(approx(
        buffers[0][5],
        3000.0 / 32768.0 * attack_gain(2, 44100.0)
    ));
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
    assert!(approx(
        out[0][0],
        1000.0 / 32768.0 * attack_gain(0, 44100.0)
    ));

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
    assert!(approx(
        out[0][0],
        1000.0 / 32768.0 * attack_gain(0, 44100.0)
    ));

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
    assert!(approx(
        out[0][0],
        1000.0 / 32768.0 * attack_gain(0, 44100.0)
    ));

    std::fs::remove_dir_all(&dir).ok();
}

const EDGE_INST: &str = r#"<instrument version="2.0" name="Edge">
  <samples>
    <sample name="Edge-1" power="0.1">
      <audiofile channel="Edge" file="edge.wav" filechannel="1"/>
    </sample>
  </samples>
</instrument>
"#;

/// Builds a kit with `channel_count` channels and returns its directory.
fn setup_multi_channel(tag: &str, channel_count: usize) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("dizmo-engine-{tag}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();

    let channels: String = (1..=channel_count)
        .map(|i| format!(r#"<channel name="Ch{i}"/>"#))
        .collect();
    let drumkit = format!(
        r#"<drumkit version="2.0">
  <metadata><defaultmidimap src="midimap.xml"/></metadata>
  <channels>{channels}</channels>
  <instruments>
    <instrument name="Kick" file="inst_kick.xml">
      <channelmap in="Kick" out="Ch1" main="true"/>
    </instrument>
    <instrument name="Edge" file="inst_edge.xml">
      <channelmap in="Edge" out="Ch{channel_count}" main="true"/>
    </instrument>
  </instruments>
</drumkit>"#
    );
    write_file(&dir, "drumkit.xml", &drumkit);
    write_file(&dir, "inst_kick.xml", INST_KICK);
    write_file(&dir, "inst_edge.xml", EDGE_INST);
    write_file(
        &dir,
        "midimap.xml",
        r#"<midimap>
  <map note="36" instr="Kick"/>
  <map note="37" instr="Edge"/>
</midimap>"#,
    );
    write_wavs(
        &dir,
        &[
            ("kick.wav", 1, &[1000, 2000, 3000]),
            ("edge.wav", 1, &[500, 600, 700]),
        ],
    );
    dir
}

#[test]
fn kit_with_more_than_16_channels_warns_and_clamps() {
    let dir = setup_multi_channel("17ch-warn", 17);

    let (engine, warnings) =
        load_engine_with_progress(dir.join("drumkit.xml"), None, &mut |_, _| {}).unwrap();
    assert_eq!(engine.kit_channels(), 16);
    assert_eq!(warnings.len(), 1);
    assert!(
        warnings[0].contains("17"),
        "warning should mention the declared channel count: {}",
        warnings[0]
    );
    assert!(
        warnings[0].contains("16"),
        "warning should mention the supported channel count: {}",
        warnings[0]
    );

    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn kit_channels_beyond_16_are_ignored_without_crashing() {
    let dir = setup_multi_channel("17ch-run", 17);
    let (mut engine, _warnings) =
        load_engine_with_progress(dir.join("drumkit.xml"), None, &mut |_, _| {}).unwrap();

    // Kick routes to Ch1 (output 0); Edge routes to Ch17 (output 16), which
    // has no plugin output buffer and must be dropped instead of panicking.
    engine.note_on(36, 127);
    engine.note_on(37, 127);
    assert_eq!(engine.active_voices(), 2);

    let out = run(&mut engine, 16, 3);
    // The Kick reaches output 0...
    assert!(approx(
        out[0][0],
        1000.0 / 32768.0 * attack_gain(0, 44100.0)
    ));
    // ...and no other output receives the out-of-range Edge stream.
    for buffer in &out[1..] {
        assert!(buffer.iter().all(|&sample| sample == 0.0));
    }

    std::fs::remove_dir_all(&dir).ok();
}

const POWER_DRUMKIT: &str = r#"<drumkit version="2.0">
  <metadata><title>Power Kit</title><defaultmidimap src="midimap.xml"/></metadata>
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

const POWER_INST: &str = r#"<instrument version="2.0" name="Kick">
  <samples>
    <sample name="Kick-1" power="0.1">
      <audiofile channel="Kick" file="kick1.wav" filechannel="1"/>
    </sample>
    <sample name="Kick-2" power="0.5">
      <audiofile channel="Kick" file="kick2.wav" filechannel="1"/>
    </sample>
    <sample name="Kick-3" power="0.9">
      <audiofile channel="Kick" file="kick3.wav" filechannel="1"/>
    </sample>
  </samples>
</instrument>
"#;

const POWER_MIDIMAP: &str = r#"<midimap>
  <map note="36" instr="Kick"/>
</midimap>
"#;

/// Triggers `hits` note-ons at `velocity` and counts which of the three power
/// layers played each time. Each sample's audio is a constant value, so once a
/// hit has rung past the 1 ms attack (44 frames) its output sample equals the
/// layer's constant, letting us read the choice back from the buffer. The
/// self-choke fades the previous voice within 100 frames, so a 100-frame block
/// per hit leaves only the fresh voice audible.
fn count_power_choices(engine: &mut Engine, velocity: u8, hits: usize) -> [usize; 3] {
    let mut counts = [0usize; 3];
    for _ in 0..hits {
        engine.note_on(36, velocity);
        let out = run(engine, 1, 100);
        let value = (out[0][99] * 32768.0).round() as i32;
        let layer = match value {
            1000 => 0,
            2000 => 1,
            3000 => 2,
            other => panic!("unexpected sample value {other}"),
        };
        counts[layer] += 1;
    }
    counts
}

#[test]
fn powerlist_spreads_adjacent_layers_near_the_velocity_boundary() {
    let dir = setup(
        "powerlist-boundary",
        POWER_DRUMKIT,
        &[("inst_kick.xml", POWER_INST), ("midimap.xml", POWER_MIDIMAP)],
        &[
            ("kick1.wav", 1, &[1000; 200]),
            ("kick2.wav", 1, &[2000; 200]),
            ("kick3.wav", 1, &[3000; 200]),
        ],
    );
    let (kit, bank, midimap) = load(&dir);
    let mut engine = Engine::new(kit, bank, midimap);

    // Powers 0.1/0.5/0.9 with the 26-sample spread put the velocity-97 target
    // right on the 0.5/0.9 midpoint, so both layers should be picked often.
    let counts = count_power_choices(&mut engine, 97, 300);
    assert!(
        counts[1] >= 60 && counts[2] >= 60,
        "expected both upper layers near the boundary, got {counts:?}"
    );
    // The softest layer is ~13 stddevs from the target: it must never play.
    assert_eq!(counts[0], 0, "softest layer played unexpectedly: {counts:?}");

    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn powerlist_pins_the_extremes_at_min_and_max_velocity() {
    let dir = setup(
        "powerlist-extremes",
        POWER_DRUMKIT,
        &[("inst_kick.xml", POWER_INST), ("midimap.xml", POWER_MIDIMAP)],
        &[
            ("kick1.wav", 1, &[1000; 200]),
            ("kick2.wav", 1, &[2000; 200]),
            ("kick3.wav", 1, &[3000; 200]),
        ],
    );
    let (kit, bank, midimap) = load(&dir);
    let mut engine = Engine::new(kit, bank, midimap);

    // Velocity 1 targets the bottom of the range: the softest layer always wins.
    let soft = count_power_choices(&mut engine, 1, 100);
    assert_eq!(soft[0], 100, "softest layer not dominant: {soft:?}");

    // Velocity 127 targets the top: the loudest layer always wins.
    let loud = count_power_choices(&mut engine, 127, 100);
    assert_eq!(loud[2], 100, "loudest layer not dominant: {loud:?}");

    std::fs::remove_dir_all(&dir).ok();
}
