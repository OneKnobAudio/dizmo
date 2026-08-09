# DIZMO

DIZMO is a VST3/CLAP audio plugin that lets you load [DrumGizmo](https://www.drumgizmo.org/) kits and play them using MIDI.

It acts as a drum sampler: a DrumGizmo kit provides a set of drum instruments (kick, snare, toms, cymbals, ...) with velocity layers and round-robin samples. DIZMO maps those instruments to MIDI notes and routes them to up to 16 outputs so each drum can be mixed individually.

## Features

- Load DrumGizmo kit files (XML kit definition, sample sets, velocity layers, MIDI mapping) via the editor's LOAD KIT button; kits load off the audio thread, so playback doesn't glitch.
- Play kits via MIDI using the kit's MIDI mapping.
- Up to 16 channel strips, one per output, each with:
  - Editable channel name.
  - Volume fader: 0 dB center, -18 dB .. +6 dB range, with a per-channel signal LED.
  - Pan control for the stereo MAIN mix (stereo plugin only).
  - Fixed channel number indicator.
  - Solo and mute.
  - MIDI choke (defined by the DrumGizmo kit XML).
- Two plugin variants sharing the same channel strips:
  - **DIZMO** (stereo): everything is mixed and routed through the MAIN output.
  - **DIZMO Multi**: every channel outputs on its own (up to 16 outputs).
- Scrollable main view (vertical and horizontal).

## Outputs

| Plugin       | MAIN                 | Channels                          |
|--------------|----------------------|-----------------------------------|
| DIZMO        | Stereo mix of all    | Internal only (no plugin output)  |
| DIZMO Multi  | None                 | Each channel routed independently |

## Status

The core features are implemented: DrumGizmo kit parsing (drumkit.xml, instrument.xml, midimap.xml), sample loading with resampling, velocity layer selection, choke handling, MIDI playback (note-on, note-off, all-notes-off, sample-accurate), per-channel parameters (fader gain, pan with 3dB law, mute, solo), kit loading from the editor UI, and the mixer UI (channel strips, choke assign).

## Building

```sh
cargo build --release
```

The plugin is built as a VST3 and CLAP from the `dizmo` crate.
