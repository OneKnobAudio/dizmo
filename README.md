# DIZMO

DIZMO is a VST3/CLAP audio plugin that lets you load [DrumGizmo](https://www.drumgizmo.org/) kits and play them using MIDI, plus a standalone editor application for creating and editing those kits.

It acts as a drum sampler: a DrumGizmo-style kit provides a set of drum instruments (kick, snare, toms, cymbals, ...) with velocity layers. DIZMO maps those instruments to MIDI notes, picks a layer per hit based on velocity (DrumGizmo's power-list algorithm, including its anti-repetition), and routes each drum to its own output so it can be mixed individually.

## Features

### Plugin (VST3/CLAP)

- Load DrumGizmo kit files (drumkit.xml, instrument XML v1/v2, midimap.xml). Kits load off the audio thread, so playback doesn't glitch.
- Play kits via MIDI using the kit's MIDI mapping. Sample-accurate note-on/off, all-notes-off, polyphonic aftertouch.
- Two variants sharing one engine:
  - **DIZMO** (stereo): everything is mixed and routed through the MAIN output.
  - **DIZMO Multi**: every channel outputs on its own (up to 16 outputs).
- Up to 16 channel strips, one per output, each with:
  - Editable channel name.
  - Volume fader (0 dB center, -18 dB .. +6 dB) with a per-channel signal LED.
  - Pan control for the stereo MAIN mix (stereo plugin only), constant-power pan law.
  - Solo and mute.
  - Choke handling defined by the kit XML (groups and directed chokes).
- Velocity-layer selection mirrors DrumGizmo's engine:
  - v2 kits: a Gaussian power draw around the hit velocity picks the closest-power sample; draws are redrawn up to 3 times to avoid repeating the previously chosen sample (DrumGizmo's anti-repetition).
  - v1 kits: probability-weighted random selection from the velocity group.
  - Samples tied at the same power are all reachable — one of the closest-power samples is picked at random, so equal-power (round-robin style) alternate sets actually alternate.
  - Normalized samples play at the hit's velocity; non-normalized samples play at full gain.
- Voice management: up to 64 simultaneous voices; the oldest is faded out (click-free) when exceeded. Attack, retrigger, choke-group (68 ms) and aftertouch (450 ms) fades match DrumGizmo's behavior.

### Editor (standalone app)

- Create kits from scratch with a guided workflow: kit channels → instruments → import samples → save as DrumGizmo-style XML (v2 with power lists).
- Import WAV files in bulk (multi-select) — the file feeds every channel of the instrument.
- Powers are distributed automatically across the instrument's samples in list order, so velocity selection tracks the sample ordering.
- The Normalized mark lives on the instrument: one checkbox, and it applies the value to all its samples (the samples themselves keep the per-sample `normalized` attribute in the file).
- Resample imported WAVs to the kit's sample rate on save.
- Edit kit metadata, instrument groups, chokes, channel assignments and the MIDI map; preview samples from the UI.
- Open existing DrumGizmo kits (including v1 velocity-group kits, converted to v2 on save).

## Outputs

| Plugin       | MAIN                 | Channels                          |
|--------------|----------------------|-----------------------------------|
| DIZMO        | Stereo mix of all    | Internal only (no plugin output)  |
| DIZMO Multi  | None                 | Each channel routed independently |

## DrumGizmo: similar and different

DIZMO reads DrumGizmo kits and mirrors its engine, but it is its own project rather than a port.

### What we do the same

- **Kit format**: drumkit.xml, instrument XML (v1 velocity groups and v2 power/samples), midimap.xml.
- **v2 sample selection (the PowerList)**: a Gaussian draw around the hit velocity picks the closest-power sample, with the same `MIN_SAMPLE_SET_SIZE` spread and retry-based anti-repetition (the chosen sample is never the previous one, up to 3 extra draws).
- **v1 sample selection**: probability-weighted random picks from the velocity group.
- **Normalized samples**: velocity scales the gain of normalized samples; non-normalized ones play at full gain.
- **Choke behavior**: 68 ms choke-group fade, directed chokes, 450 ms polyphonic aftertouch choke, instant retrigger self-fade.
- **Sample rate**: samples are resampled once, at load time, to the engine rate.
- **Note-off on drums**: ignored; samples ring out (with all-notes-off fading them click-free).

### What we do differently

| Area | DrumGizmo | DIZMO |
|------|-----------|-------|
| **Form factor** | Standalone application (JACK/ALSA audio). | VST3/CLAP plugin for use in a DAW. |
| **Host integration** | No host automation or plugin outputs. | Host automation of mixer parameters (fader, pan, mute, solo); up to 16 plugin outputs in DIZMO Multi. |
| **Kit authoring** | Kits are recorded and produced externally (typically by kit makers). | Ships a standalone editor that creates kits from scratch and imports WAVs, saving DrumGizmo-compatible XML. |
| **Power assignment** | Authored by the kit producer; DIZMO reads whatever is there. | The editor auto-distributes powers evenly across samples in list order on import, so any set of samples is immediately playable as velocity layers. |
| **Normalized mark** | A runtime engine option (`normalized_samples`) that applies to the whole loaded kit; not stored in kit files. | Stored in the kit XML as a per-sample attribute, edited at instrument level in the editor (one checkbox sets it for all samples). |
| **Round-robin** | Multi-sample layers are typically used as round-robin alternates. | No deterministic cycling. Samples tied at the same power (the round-robin case) are all reachable: the closest-power set is picked at random, and anti-repetition redraws avoid the same sample twice in a row. |
| **Sample rate editing** | Resampled on load only. | Same, plus the editor can resample imported files to the kit rate when saving. |
| **v1 kits** | Legacy velocity-group format supported. | Supported for playback and editable: saving converts them to v2 with powers derived from the group midpoints. |
| **Output routing** | Engine-level channel routing configured in the kit. | Kit channel routing is honoured; the plugin additionally exposes a stereo mix or per-channel outputs. |

## Building

```sh
cargo build --release            # plugin (VST3/CLAP)
cargo run --release -p dizmo_editor   # editor app
```

The plugin is built as VST3 and CLAP bundles from the `dizmo` crate (via `cargo xtask bundle`). The workspace is split into three crates: `dizmo` (plugin), `dizmo_kit` (kit parsing), and `dizmo_editor` (editor app).

## Status

Core features are implemented: DrumGizmo kit parsing (drumkit.xml, instrument XML v1/v2, midimap), sample loading with resampling, power-list velocity selection with anti-repetition, choke handling (groups and directed chokes), MIDI playback (note-on, note-off, all-notes-off, sample-accurate), per-channel parameters (fader gain, pan, mute, solo), and the editor (kit creation, bulk sample import with power distribution, instrument-level normalized, resampling on save, metadata/MIDI map/choke editing, sample preview). CI runs the test suite and publishes plugin and editor releases for macOS and Windows on version tags.

## License

DIZMO is free software licensed under the [GNU General Public License v2.0 (or, at your option, any later version)](LICENSE.md).

## AI Disclaimer

The author of this project used an LLM for code review and documentation.