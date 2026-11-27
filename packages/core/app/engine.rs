pub const PHOENIX: &str = "phoenix";
pub const KICK_LIMIT: u32 = 2;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct EngineMemory {
    pub forced_last_launch: bool,
    pub kicks: u32,
}

#[derive(Debug, PartialEq)]
pub struct EngineDecision {
    pub force_phoenix: bool,
    pub memory: EngineMemory,
    pub newly_refused: bool,
}

pub fn decide(previous_client: &str, memory: EngineMemory) -> EngineDecision {
    let kicked = memory.forced_last_launch && previous_client != PHOENIX;
    let kicks = match (previous_client == PHOENIX, kicked) {
        (true, _) => 0,
        (false, true) => memory.kicks.saturating_add(1),
        (false, false) => memory.kicks,
    };
    let refused = kicks >= KICK_LIMIT;
    EngineDecision {
        force_phoenix: !refused,
        memory: EngineMemory {
            forced_last_launch: !refused,
            kicks,
        },
        newly_refused: refused && memory.kicks < KICK_LIMIT,
    }
}

pub const REFUSED_TITLE: &str = "Discord turned off Phoenix for this account";
pub const REFUSED_MESSAGE: &str = "Discord keeps switching this account back to its classic app, which astral can't run in. astral has stopped switching it back so Discord stays usable. Phoenix access is rolled out by Discord; reinstall astral after a Discord update, or use an install that unlocks Phoenix, to try again.";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stops_forcing_after_discord_switches_back_twice() {
        let first = decide("phoenix", EngineMemory::default());
        assert!(first.force_phoenix && first.memory.kicks == 0);
        let kicked_once = decide("react-native", first.memory);
        assert!(kicked_once.force_phoenix && kicked_once.memory.kicks == 1 && !kicked_once.newly_refused);
        let kicked_twice = decide("react-native", kicked_once.memory);
        assert!(!kicked_twice.force_phoenix && kicked_twice.newly_refused);
        let later = decide("react-native", kicked_twice.memory);
        assert!(!later.force_phoenix && !later.newly_refused);
    }

    #[test]
    fn a_launch_that_stays_on_phoenix_clears_the_kicks() {
        let kicked = EngineMemory {
            forced_last_launch: true,
            kicks: 1,
        };
        let healthy = decide("phoenix", kicked);
        assert!(healthy.force_phoenix && healthy.memory.kicks == 0);
    }

    #[test]
    fn a_first_launch_on_the_classic_app_is_switched_without_counting() {
        let fresh = decide("", EngineMemory::default());
        assert!(fresh.force_phoenix && fresh.memory.kicks == 0);
    }
}
