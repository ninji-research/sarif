//! Record/Replay harness for deterministic test execution.
//!
//! This module provides infrastructure for recording and replaying nondeterministic
//! events (reads, writes, random numbers, time) to ensure reproducible test execution.

use std::fs::File;
use std::io::{Read, Write};
use std::path::Path;

/// Type of nondeterministic event
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum EventType {
    Read(String),
    Write(String),
    Random(u64),
    Now(u64),
}

/// A single nondeterministic event
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Event {
    pub event_type: EventType,
    pub timestamp_us: u64,
    pub sequence: u64,
}

/// The record/replay state
pub enum Mode {
    None,
    Recording(Vec<Event>),
    Replaying(Vec<Event>),
}

/// Record/Replay harness
pub struct ReplayHarness {
    mode: Mode,
    sequence: u64,
    replay_index: usize,
}

impl ReplayHarness {
    /// Create a new harness
    #[must_use]
    pub const fn new() -> Self {
        Self {
            mode: Mode::None,
            sequence: 0,
            replay_index: 0,
        }
    }

    /// Start recording to memory
    pub fn start_recording(&mut self) {
        self.mode = Mode::Recording(Vec::new());
        self.sequence = 0;
        self.replay_index = 0;
    }

    /// Start replaying from events
    pub fn start_replaying(&mut self, events: Vec<Event>) {
        self.mode = Mode::Replaying(events);
        self.sequence = 0;
        self.replay_index = 0;
    }

    /// Record a read event
    pub fn record_read(&mut self, input: &str) {
        if let Mode::Recording(events) = &mut self.mode {
            events.push(Event {
                event_type: EventType::Read(input.to_owned()),
                timestamp_us: self.sequence,
                sequence: self.sequence,
            });
            self.sequence += 1;
        }
    }

    /// Record a write event
    pub fn record_write(&mut self, output: &str) {
        if let Mode::Recording(events) = &mut self.mode {
            events.push(Event {
                event_type: EventType::Write(output.to_owned()),
                timestamp_us: self.sequence,
                sequence: self.sequence,
            });
            self.sequence += 1;
        }
    }

    /// Record a random number generation
    pub fn record_random(&mut self, value: u64) {
        if let Mode::Recording(events) = &mut self.mode {
            events.push(Event {
                event_type: EventType::Random(value),
                timestamp_us: self.sequence,
                sequence: self.sequence,
            });
            self.sequence += 1;
        }
    }

    /// Record current time
    pub fn record_now(&mut self, timestamp: u64) {
        if let Mode::Recording(events) = &mut self.mode {
            events.push(Event {
                event_type: EventType::Now(timestamp),
                timestamp_us: self.sequence,
                sequence: self.sequence,
            });
            self.sequence += 1;
        }
    }

    /// Replay a read event (returns recorded input)
    pub fn replay_read(&mut self) -> Option<String> {
        if let Mode::Replaying(events) = &mut self.mode
            && self.replay_index < events.len()
        {
            let event = &events[self.replay_index];
            self.replay_index += 1;
            if let EventType::Read(s) = &event.event_type {
                return Some(s.clone());
            }
        }
        None
    }

    /// Replay a write event (returns expected output)
    pub fn replay_write(&mut self) -> Option<String> {
        if let Mode::Replaying(events) = &mut self.mode
            && self.replay_index < events.len()
        {
            let event = &events[self.replay_index];
            self.replay_index += 1;
            if let EventType::Write(s) = &event.event_type {
                return Some(s.clone());
            }
        }
        None
    }

    /// Replay a random number
    pub fn replay_random(&mut self) -> Option<u64> {
        if let Mode::Replaying(events) = &mut self.mode
            && self.replay_index < events.len()
        {
            let event = &events[self.replay_index];
            self.replay_index += 1;
            if let EventType::Random(v) = &event.event_type {
                return Some(*v);
            }
        }
        None
    }

    /// Replay current time
    pub fn replay_now(&mut self) -> Option<u64> {
        if let Mode::Replaying(events) = &mut self.mode
            && self.replay_index < events.len()
        {
            let event = &events[self.replay_index];
            self.replay_index += 1;
            if let EventType::Now(t) = &event.event_type {
                return Some(*t);
            }
        }
        None
    }

    /// Get recorded events (for saving to file)
    #[must_use]
    pub fn events(&self) -> Vec<Event> {
        match &self.mode {
            Mode::Recording(events) => events.clone(),
            _ => Vec::new(),
        }
    }

    /// Save events to a file
    ///
    /// # Errors
    ///
    /// Returns an error if the file cannot be created or written.
    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        let mut file = File::create(path)?;
        for event in self.events() {
            let line = match &event.event_type {
                EventType::Read(s) => format!("READ|{s}\n"),
                EventType::Write(s) => format!("WRITE|{s}\n"),
                EventType::Random(v) => format!("RANDOM|{v}\n"),
                EventType::Now(t) => format!("NOW|{t}\n"),
            };
            file.write_all(line.as_bytes())?;
        }
        Ok(())
    }

    /// Load events from a file
    ///
    /// # Errors
    ///
    /// Returns an error if the file cannot be opened or read.
    pub fn load(&mut self, path: &Path) -> std::io::Result<()> {
        let mut file = File::open(path)?;
        let mut contents = String::new();
        file.read_to_string(&mut contents)?;

        let mut events = Vec::new();
        for line in contents.lines() {
            let parts: Vec<&str> = line.splitn(2, '|').collect();
            if parts.len() == 2 {
                let event = match parts[0] {
                    "READ" => Event {
                        event_type: EventType::Read(parts[1].to_owned()),
                        timestamp_us: 0,
                        sequence: 0,
                    },
                    "WRITE" => Event {
                        event_type: EventType::Write(parts[1].to_owned()),
                        timestamp_us: 0,
                        sequence: 0,
                    },
                    "RANDOM" => Event {
                        event_type: EventType::Random(parts[1].parse().unwrap_or(0)),
                        timestamp_us: 0,
                        sequence: 0,
                    },
                    "NOW" => Event {
                        event_type: EventType::Now(parts[1].parse().unwrap_or(0)),
                        timestamp_us: 0,
                        sequence: 0,
                    },
                    _ => continue,
                };
                events.push(event);
            }
        }

        self.start_replaying(events);
        Ok(())
    }
}

impl Default for ReplayHarness {
    fn default() -> Self {
        Self::new()
    }
}
