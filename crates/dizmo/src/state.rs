use nice_plug::prelude::*;

/// How the channels are routed to the host outputs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Enum, Default)]
pub enum OutputMode {
    /// All channels are panned and mixed down to the stereo MAIN bus.
    #[id = "stereo"]
    #[default]
    Stereo,

    /// The MAIN bus is disabled and each channel outputs through its own bus.
    #[id = "multi"]
    Multi,
}
