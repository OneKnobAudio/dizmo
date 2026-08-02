# DIZMO

DIZMO is a VST3/CLAP audio plugin that lets you load [DrumGizmo](https://www.drumgizmo.org/) kits and play them using MIDI.

It acts as a drum sampler: a DrumGizmo kit provides a set of drum instruments (kick, snare, toms, cymbals, ...) with velocity layers and round-robin samples. DIZMO maps those instruments to MIDI notes and routes them to up to 16 outputs so each drum can be mixed individually.

## Features

- Load DrumGizmo kit files (XML kit definition, sample sets, velocity layers, hitfiles, MIDI mapping).
- Play kits via MIDI with per-instrument note assignment.
- Up to 16 channel strips, one per output, each with:
  - Editable channel name.
  - Volume fader: 0 dB center, +/- 12 dB range.
  - Pan control for the stereo MAIN mix.
  - Fixed channel number indicator.
  - Assignable MIDI note indicator.
  - Solo and mute.
  - MIDI choke (a channel can be choked by one or more other channels).
- Main strip for the stereo mix of all channels.
- Stereo or Multi output modes:
  - **STEREO**: everything is mixed and routed through the MAIN output.
  - **MULTI**: MAIN is disabled and every channel outputs on its own (up to 16 outputs).
- Scrollable main view (vertical and horizontal).

## Outputs

| Mode    | MAIN                 | Channels                          |
|---------|----------------------|-----------------------------------|
| STEREO  | Stereo mix of all    | Internal only (no plugin output)  |
| MULTI   | Disabled             | Each channel routed independently |

## Status

Work in progress. See [TASKS.md](TASKS.md) for the current task list, [DESIGN.md](DESIGN.md) for the UI design, and [ARCHITECTURE.md](ARCHITECTURE.md) for the architecture.

## Building

```sh
cargo build --release
```

The plugin is built as a VST3 and CLAP (plus standalone) from the `dizmo` crate.
