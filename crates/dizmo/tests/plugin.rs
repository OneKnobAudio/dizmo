use std::path::{Path, PathBuf};

use dizmo::DizmoMultiPlugin;
use dizmo::DizmoPlugin;
use dizmo::mixdown_to_stereo;

const DRUMKIT: &str = r#"<drumkit version="2.0">
  <metadata>
    <title>Test Kit</title>
    <description>Plugin test fixtures</description>
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

const INST_KICK: &str = r#"<instrument version="2.0" name="Kick">
  <samples>
    <sample name="Kick-1" power="0.1">
      <audiofile channel="Kick" file="kick.wav" filechannel="1"/>
    </sample>
  </samples>
</instrument>
"#;

const INST_SNARE: &str = r#"<instrument version="2.0" name="Snare">
  <samples>
    <sample name="Snare-1" power="0.1">
      <audiofile channel="Snare" file="snare.wav" filechannel="1"/>
    </sample>
  </samples>
</instrument>
"#;

const MIDIMAP: &str = r#"<midimap>
  <map note="36" instr="Kick"/>
  <map note="38" instr="Snare"/>
</midimap>
"#;

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

fn setup(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("dizmo-plugin-{tag}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    write_file(&dir, "drumkit.xml", DRUMKIT);
    write_file(&dir, "inst_kick.xml", INST_KICK);
    write_file(&dir, "inst_snare.xml", INST_SNARE);
    write_file(&dir, "midimap.xml", MIDIMAP);
    write_wav(&dir.join("kick.wav"), 1, &[1000, 2000, 3000]);
    write_wav(&dir.join("snare.wav"), 1, &[4000, 5000, 6000]);
    dir
}

#[test]
fn stereo_plugin_loads_a_kit() {
    let dir = setup("stereo");
    let mut plugin = DizmoPlugin::default();
    assert!(plugin.load_kit(dir.join("drumkit.xml")).is_ok());
}

#[test]
fn multi_plugin_loads_a_kit() {
    let dir = setup("multi");
    let mut plugin = DizmoMultiPlugin::default();
    assert!(plugin.load_kit(dir.join("drumkit.xml")).is_ok());
}

#[test]
fn loading_a_broken_kit_errors() {
    let dir = setup("broken");
    write_file(&dir, "drumkit.xml", "not xml at all");
    let mut plugin = DizmoPlugin::default();
    assert!(plugin.load_kit(dir.join("drumkit.xml")).is_err());
}

#[test]
fn loading_twice_replaces_the_engine() {
    let dir = setup("twice");
    let mut plugin = DizmoMultiPlugin::default();
    assert!(plugin.load_kit(dir.join("drumkit.xml")).is_ok());
    assert!(plugin.load_kit(dir.join("drumkit.xml")).is_ok());
}

#[test]
fn mixdown_sums_kit_channels_into_both_stereo_sides() {
    use dizmo::params::DizmoParams;

    let scratch = vec![
        vec![1.0, 2.0, 3.0],
        vec![10.0, 20.0, 30.0],
        vec![100.0, 200.0, 300.0],
    ];
    let mut left = vec![0.0; 3];
    let mut right = vec![0.0; 3];
    let params = DizmoParams::default();

    mixdown_to_stereo(&scratch, 2, 3, &mut left, &mut right, &params.channels);

    // With default params (0 dB gain, center pan), both channels should sum equally
    // but scaled by the 0.70710678 constant-power center pan law.
    let sqrt2_2 = std::f32::consts::FRAC_1_SQRT_2;
    assert!((left[0] - 11.0 * sqrt2_2).abs() < 1e-5);
    assert!((left[1] - 22.0 * sqrt2_2).abs() < 1e-5);
    assert!((left[2] - 33.0 * sqrt2_2).abs() < 1e-5);
    assert_eq!(right, left);
}

#[test]
fn mixdown_ramps_the_fader_smoother_on_automation() {
    use dizmo::params::DizmoParams;

    let scratch = vec![vec![1.0, 1.0, 1.0]];
    let mut left = vec![0.0; 3];
    let mut right = vec![0.0; 3];
    let params = DizmoParams::default();

    // Model the host automating the fader from 0 dB (1.0) down to 0.5. The
    // wrapper resets the smoother to the current value on activate, so do the
    // same here before starting the ramp.
    let fader = &params.channels[0].fader;
    fader.smoothed.reset(1.0);
    fader.smoothed.set_target(48000.0, 0.5);

    mixdown_to_stereo(&scratch, 1, 3, &mut left, &mut right, &params.channels);

    // Three samples into a 2400-step ramp: still near unity, strictly
    // decreasing, and nowhere near the 0.5 target yet.
    assert!(left[0] > left[1], "gains must ramp: {:?}", left);
    assert!(left[1] > left[2], "gains must ramp: {:?}", left);
    assert!(left[0] < 1.0, "first step must move off unity: {:?}", left);
    assert!(left[2] > 0.5, "ramp must not reach target yet: {:?}", left);
    assert_eq!(right, left);
}

#[test]
fn mixdown_applies_constant_power_pan_law() {
    use dizmo::params::DizmoParams;

    let scratch = vec![vec![1.0]];
    let mut left = vec![0.0; 1];
    let mut right = vec![0.0; 1];
    let params = DizmoParams::default();

    // Center pan (0.0): should apply -3dB (0.707) to both sides
    params.channels[0].pan.smoothed.reset(0.0);
    mixdown_to_stereo(&scratch, 1, 1, &mut left, &mut right, &params.channels);
    assert!((left[0] - std::f32::consts::FRAC_1_SQRT_2).abs() < 1e-6);
    assert!((right[0] - std::f32::consts::FRAC_1_SQRT_2).abs() < 1e-6);
}

#[test]
fn mixdown_pan_uses_percentage_range() {
    use dizmo::params::DizmoParams;

    let scratch = vec![vec![1.0]];
    let params = DizmoParams::default();

    // Full left (-100): all signal to the left channel.
    let mut left = vec![0.0];
    let mut right = vec![0.0];
    params.channels[0].pan.smoothed.reset(-100.0);
    params.channels[0].pan.smoothed.set_target(48000.0, -100.0);
    mixdown_to_stereo(&scratch, 1, 1, &mut left, &mut right, &params.channels);
    assert!((left[0] - 1.0).abs() < 1e-6);
    assert!(right[0].abs() < 1e-6);

    // Full right (+100): all signal to the right channel.
    params.channels[0].pan.smoothed.reset(100.0);
    params.channels[0].pan.smoothed.set_target(48000.0, 100.0);
    let mut left = vec![0.0];
    let mut right = vec![0.0];
    mixdown_to_stereo(&scratch, 1, 1, &mut left, &mut right, &params.channels);
    assert!(left[0].abs() < 1e-6);
    assert!((right[0] - 1.0).abs() < 1e-6);
}
