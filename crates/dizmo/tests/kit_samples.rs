use dizmo::kit::{Kit, SampleError, load_samples};
use std::fs;
use std::path::{Path, PathBuf};

fn write_wav(path: &Path, channels: u16, rate: u32, samples: &[i16]) {
    let spec = hound::WavSpec {
        channels,
        sample_rate: rate,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut writer = hound::WavWriter::create(path, spec).unwrap();
    for &sample in samples {
        writer.write_sample(sample).unwrap();
    }
    writer.finalize().unwrap();
}

/// Creates a temp kit with one instrument and writes the given WAV files next
/// to it. Returns the kit directory.
fn write_kit(tag: &str, instrument_xml: &str, wavs: &[(&str, u16, &[i16])]) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("dizmo-samples-{tag}"));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();

    let drumkit = r#"<drumkit version="2.0">
  <instruments>
    <instrument name="Kick" file="inst_kick.xml"/>
  </instruments>
</drumkit>"#;
    fs::write(dir.join("drumkit.xml"), drumkit).unwrap();
    fs::write(dir.join("inst_kick.xml"), instrument_xml).unwrap();

    for (name, channels, samples) in wavs {
        write_wav(&dir.join(name), *channels, 44100, samples);
    }
    dir
}

fn approx(expected: f32, actual: f32) -> bool {
    (expected - actual).abs() < 1e-6
}

#[test]
fn decodes_samples_and_deduplicates() {
    let dir = write_kit(
        "dedupe",
        r#"<instrument version="2.0" name="Kick">
  <samples>
    <sample name="Kick-1" power="0.1">
      <audiofile channel="AmbL" file="kick.wav" filechannel="1"/>
      <audiofile channel="Kick" file="kick.wav" filechannel="2"/>
      <audiofile channel="AmbR" file="kick.wav" filechannel="3"/>
    </sample>
    <sample name="Kick-2" power="0.2">
      <audiofile channel="Kick" file="kick.wav" filechannel="2"/>
    </sample>
  </samples>
</instrument>"#,
        &[(
            "kick.wav",
            3,
            // 3 interleaved frames: [L, M, R] per frame.
            &[100, 1000, -100, 200, 2000, -200, 300, 3000, -300],
        )],
    );

    let kit = Kit::load(dir.join("drumkit.xml")).unwrap();
    let bank = load_samples(&kit, None).unwrap();

    // The two samples share one file, so it is decoded only once.
    assert_eq!(bank.len(), 1);

    let kick = &kit.instruments[0];
    let first = &kick.samples[0];
    assert_eq!(bank.file(&dir.join("kick.wav")).unwrap().frames(), 3);

    // filechannel 2 in the XML is the 1-based second channel of the WAV.
    let mid = bank
        .audio_file(&kick.base_dir, &first.audio_files[1])
        .unwrap();
    assert_eq!(mid.len(), 3);
    assert!(approx(1000.0 / 32768.0, mid[0]));
    assert!(approx(2000.0 / 32768.0, mid[1]));
    assert!(approx(3000.0 / 32768.0, mid[2]));

    // The same buffer is shared with the second sample's identical reference.
    let second = &kick.samples[1];
    assert!(
        bank.audio_file(&kick.base_dir, &second.audio_files[0])
            .is_some()
    );
}

#[test]
fn loads_multiple_files() {
    let dir = write_kit(
        "multi-file",
        r#"<instrument version="2.0" name="Kick">
  <samples>
    <sample name="Kick-1" power="0.1">
      <audiofile channel="Kick" file="kick.wav" filechannel="1"/>
    </sample>
    <sample name="Kick-2" power="0.2">
      <audiofile channel="Kick" file="snare.wav" filechannel="1"/>
    </sample>
  </samples>
</instrument>"#,
        &[("kick.wav", 1, &[1, 2, 3]), ("snare.wav", 1, &[4, 5, 6])],
    );

    let kit = Kit::load(dir.join("drumkit.xml")).unwrap();
    let bank = load_samples(&kit, None).unwrap();
    assert_eq!(bank.len(), 2);
    assert_eq!(bank.file(&dir.join("kick.wav")).unwrap().sample_rate, 44100);
}

#[test]
fn errors_on_missing_file() {
    let dir = write_kit(
        "missing",
        r#"<instrument version="2.0" name="Kick">
  <samples>
    <sample name="Kick-1" power="0.1">
      <audiofile channel="Kick" file="does-not-exist.wav" filechannel="1"/>
    </sample>
  </samples>
</instrument>"#,
        &[],
    );

    let kit = Kit::load(dir.join("drumkit.xml")).unwrap();
    let error = load_samples(&kit, None).unwrap_err();
    assert!(matches!(error, SampleError::Io { .. }), "got {error:?}");
}

#[test]
fn errors_on_invalid_wav() {
    let dir = write_kit(
        "invalid-wav",
        r#"<instrument version="2.0" name="Kick">
  <samples>
    <sample name="Kick-1" power="0.1">
      <audiofile channel="Kick" file="kick.wav" filechannel="1"/>
    </sample>
  </samples>
</instrument>"#,
        &[],
    );
    fs::write(dir.join("kick.wav"), b"this is not a wav file").unwrap();

    let kit = Kit::load(dir.join("drumkit.xml")).unwrap();
    let error = load_samples(&kit, None).unwrap_err();
    assert!(matches!(error, SampleError::Decode { .. }), "got {error:?}");
}

#[test]
fn errors_on_channel_out_of_range() {
    let dir = write_kit(
        "channel-range",
        r#"<instrument version="2.0" name="Kick">
  <samples>
    <sample name="Kick-1" power="0.1">
      <audiofile channel="Kick" file="kick.wav" filechannel="5"/>
    </sample>
  </samples>
</instrument>"#,
        &[("kick.wav", 1, &[1, 2, 3])],
    );

    let kit = Kit::load(dir.join("drumkit.xml")).unwrap();
    let error = load_samples(&kit, None).unwrap_err();
    match error {
        SampleError::ChannelOutOfRange {
            sample,
            channel,
            num_channels,
            ..
        } => {
            assert_eq!(sample, "Kick-1");
            assert_eq!(channel, 4);
            assert_eq!(num_channels, 1);
        }
        other => panic!("expected ChannelOutOfRange, got {other:?}"),
    }
}

#[test]
fn decodes_stereo_int_pcm_normalized() {
    let dir = write_kit(
        "stereo",
        r#"<instrument version="2.0" name="Kick">
  <samples>
    <sample name="Kick-1" power="0.1">
      <audiofile channel="L" file="stereo.wav" filechannel="1"/>
      <audiofile channel="R" file="stereo.wav" filechannel="2"/>
    </sample>
  </samples>
</instrument>"#,
        // [L0, R0, L1, R1]
        &[("stereo.wav", 2, &[32767, -32768, 16384, -16384])],
    );

    let kit = Kit::load(dir.join("drumkit.xml")).unwrap();
    let bank = load_samples(&kit, None).unwrap();
    let kick = &kit.instruments[0];
    let sample = &kick.samples[0];

    let left = bank
        .audio_file(&kick.base_dir, &sample.audio_files[0])
        .unwrap();
    let right = bank
        .audio_file(&kick.base_dir, &sample.audio_files[1])
        .unwrap();

    assert!(approx(32767.0 / 32768.0, left[0]));
    assert!(approx(-32768.0 / 32768.0, right[0]));
    assert!(approx(16384.0 / 32768.0, left[1]));
    assert!(approx(-16384.0 / 32768.0, right[1]));
}

const INST_XML: &str = r#"<instrument version="2.0" name="Kick">
  <samples>
    <sample name="Kick-1" power="0.1">
      <audiofile channel="Kick" file="kick.wav" filechannel="1"/>
    </sample>
  </samples>
</instrument>"#;

#[test]
fn resamples_down_to_target_rate() {
    let dir = write_kit("resample-down", INST_XML, &[]);
    write_wav(&dir.join("kick.wav"), 1, 44100, &[1000; 8]);

    let kit = Kit::load(dir.join("drumkit.xml")).unwrap();
    let bank = load_samples(&kit, Some(22050)).unwrap();
    let file = bank.file(&dir.join("kick.wav")).unwrap();

    assert_eq!(file.sample_rate, 22050);
    assert_eq!(file.frames(), 4);
    // A constant signal keeps its amplitude through the resampler.
    for &sample in file.channels[0].iter() {
        assert!(approx(1000.0 / 32768.0, sample));
    }
}

#[test]
fn resamples_up_to_target_rate() {
    let dir = write_kit("resample-up", INST_XML, &[]);
    write_wav(&dir.join("kick.wav"), 1, 22050, &[1, 2, 3]);

    let kit = Kit::load(dir.join("drumkit.xml")).unwrap();
    let bank = load_samples(&kit, Some(44100)).unwrap();
    let file = bank.file(&dir.join("kick.wav")).unwrap();
    let data = &file.channels[0];

    assert_eq!(file.sample_rate, 44100);
    assert_eq!(data.len(), 6);
    // Sample-grid positions are reproduced exactly; odd indices interpolate.
    assert!(approx(1.0 / 32768.0, data[0]));
    assert!(approx(2.0 / 32768.0, data[2]));
    assert!(approx(3.0 / 32768.0, data[4]));
}

#[test]
fn keeps_native_rate_when_target_matches() {
    let dir = write_kit("resample-same", INST_XML, &[]);
    write_wav(&dir.join("kick.wav"), 1, 44100, &[1000, 2000, 3000]);

    let kit = Kit::load(dir.join("drumkit.xml")).unwrap();
    let bank = load_samples(&kit, Some(44100)).unwrap();
    let file = bank.file(&dir.join("kick.wav")).unwrap();

    assert_eq!(file.sample_rate, 44100);
    assert_eq!(file.frames(), 3);
}

#[test]
fn resampling_all_channels_of_a_stereo_file() {
    let dir = write_kit("resample-stereo", INST_XML, &[]);
    write_wav(
        &dir.join("kick.wav"),
        2,
        44100,
        &[1000, -1000, 2000, -2000, 3000, -3000, 4000, -4000],
    );

    let kit = Kit::load(dir.join("drumkit.xml")).unwrap();
    let bank = load_samples(&kit, Some(22050)).unwrap();
    let file = bank.file(&dir.join("kick.wav")).unwrap();

    assert_eq!(file.channels.len(), 2);
    assert_eq!(file.frames(), 2);
    assert!(approx(1000.0 / 32768.0, file.channels[0][0]));
    assert!(approx(-1000.0 / 32768.0, file.channels[1][0]));
}
