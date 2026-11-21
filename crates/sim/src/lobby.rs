use crate::bots::Skill;
use serde::{Deserialize, Serialize};

pub const MAX_BOTS: u8 = 8;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Member {
    pub num: u32,
    pub name: String,
}

/// The state of one lobby. Every machine holds the same state because
/// every change travels the bus in one order.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Lobby {
    pub members: Vec<Member>,
    pub bots: u8,
    pub skill: Skill,
    pub running: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Command {
    Join { num: u32, name: String },
    Leave { num: u32 },
    Rename { num: u32, name: String },
    SetBots(u8),
    SetSkill(Skill),
    Start,
    End,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Rejected {
    NotHost,
    Running,
    NotRunning,
    TooManyBots,
}

impl Default for Lobby {
    fn default() -> Self {
        Self {
            members: Vec::new(),
            bots: 4,
            skill: Skill::Pilot,
            running: false,
        }
    }
}

impl Lobby {
    /// The host is the member with the lowest number. The bus gives the
    /// same order to everyone, so everyone agrees on the host.
    pub fn host(&self) -> Option<u32> {
        self.members.iter().map(|m| m.num).min()
    }

    pub fn apply(&mut self, from: u32, cmd: Command) -> Result<bool, Rejected> {
        match cmd {
            Command::Join { num, name } => {
                if self.members.iter().any(|m| m.num == num) {
                    if let Some(m) = self.members.iter_mut().find(|m| m.num == num) {
                        m.name = name;
                    }
                } else {
                    self.members.push(Member { num, name });
                    self.members.sort_by_key(|m| m.num);
                }
                Ok(true)
            }
            Command::Leave { num } => {
                self.members.retain(|m| m.num != num);
                Ok(true)
            }
            Command::Rename { num, name } => {
                if let Some(m) = self.members.iter_mut().find(|m| m.num == num) {
                    m.name = name;
                }
                Ok(true)
            }
            Command::SetBots(bots) => {
                self.require_host(from)?;
                if self.running {
                    return Err(Rejected::Running);
                }
                if bots > MAX_BOTS {
                    return Err(Rejected::TooManyBots);
                }
                self.bots = bots;
                Ok(true)
            }
            Command::SetSkill(skill) => {
                self.require_host(from)?;
                if self.running {
                    return Err(Rejected::Running);
                }
                self.skill = skill;
                Ok(true)
            }
            Command::Start => {
                self.require_host(from)?;
                if self.running {
                    return Err(Rejected::Running);
                }
                self.running = true;
                Ok(true)
            }
            Command::End => {
                self.require_host(from)?;
                if !self.running {
                    return Err(Rejected::NotRunning);
                }
                self.running = false;
                Ok(true)
            }
        }
    }

    fn require_host(&self, from: u32) -> Result<(), Rejected> {
        if self.host() == Some(from) {
            Ok(())
        } else {
            Err(Rejected::NotHost)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lobby() -> Lobby {
        let mut l = Lobby::default();
        l.apply(3, Command::Join { num: 3, name: "host".into() }).unwrap();
        l.apply(7, Command::Join { num: 7, name: "guest".into() }).unwrap();
        l
    }

    #[test]
    fn the_lowest_number_hosts() {
        let l = lobby();
        assert_eq!(l.host(), Some(3));
    }

    #[test]
    fn only_the_host_changes_the_bots() {
        let mut l = lobby();
        assert_eq!(l.apply(7, Command::SetBots(2)), Err(Rejected::NotHost));
        assert_eq!(l.apply(3, Command::SetBots(2)), Ok(true));
        assert_eq!(l.bots, 2);
    }

    #[test]
    fn the_host_passes_on_when_it_leaves() {
        let mut l = lobby();
        l.apply(3, Command::Leave { num: 3 }).unwrap();
        assert_eq!(l.host(), Some(7));
        assert_eq!(l.apply(7, Command::Start), Ok(true));
    }

    #[test]
    fn a_late_join_enters_the_running_match() {
        let mut l = lobby();
        l.apply(3, Command::Start).unwrap();
        assert_eq!(l.apply(9, Command::Join { num: 9, name: "late".into() }), Ok(true));
        assert!(l.running);
        assert_eq!(l.members.len(), 3);
    }

    #[test]
    fn a_match_cannot_start_twice() {
        let mut l = lobby();
        l.apply(3, Command::Start).unwrap();
        assert_eq!(l.apply(3, Command::Start), Err(Rejected::Running));
        assert_eq!(l.apply(3, Command::SetBots(1)), Err(Rejected::Running));
        assert_eq!(l.apply(3, Command::End), Ok(true));
        assert_eq!(l.apply(3, Command::End), Err(Rejected::NotRunning));
    }

    #[test]
    fn the_bot_count_is_capped() {
        let mut l = lobby();
        assert_eq!(l.apply(3, Command::SetBots(MAX_BOTS + 1)), Err(Rejected::TooManyBots));
        assert_eq!(l.apply(3, Command::SetBots(MAX_BOTS)), Ok(true));
    }

    #[test]
    fn members_are_sorted_by_number() {
        let mut l = Lobby::default();
        l.apply(9, Command::Join { num: 9, name: "b".into() }).unwrap();
        l.apply(2, Command::Join { num: 2, name: "a".into() }).unwrap();
        assert_eq!(l.host(), Some(2));
    }
}
