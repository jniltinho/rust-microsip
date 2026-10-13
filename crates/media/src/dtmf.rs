//! Allocation-free RFC 4733 DTMF payloads and finite transmission planning.
//!
//! The session must negotiate telephone-event/8000, supply one constant RTP
//! timestamp per event, advance sequence numbers for repeats, pace packets and
//! leave an inter-digit gap. This module performs no I/O, timing or SDP parsing.
use crate::MediaError;

/// One of the sixteen standard DTMF events, never an arbitrary event code.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Digit(u8);
impl Digit {
    /// Return the standardized event number in 0..=15.
    pub const fn event_number(self) -> u8 {
        self.0
    }
    /// Validate a standard DTMF event number.
    ///
    /// # Errors
    /// Returns [`MediaError::InvalidEvent`] for non-DTMF event codes.
    pub fn from_event(event: u8) -> Result<Self, MediaError> {
        if event > 15 {
            return Err(MediaError::InvalidEvent);
        }
        Ok(Self(event))
    }
}
impl TryFrom<char> for Digit {
    type Error = MediaError;
    fn try_from(symbol: char) -> Result<Self, Self::Error> {
        match symbol {
            '0'..='9' => Ok(Self(symbol as u8 - b'0')),
            '*' => Ok(Self(10)),
            '#' => Ok(Self(11)),
            'A'..='D' => Ok(Self(symbol as u8 - b'A' + 12)),
            _ => Err(MediaError::InvalidEvent),
        }
    }
}

/// Validated four-byte single-event DTMF payload (not an RTP packet).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TelephoneEvent {
    digit: Digit,
    end: bool,
    volume: u8,
    duration: u16,
}
impl TelephoneEvent {
    /// Create a DTMF payload; volume is unsigned attenuation in dBm0.
    ///
    /// # Errors
    /// Returns [`MediaError::InvalidEvent`] for volume >63 or zero duration.
    /// Zero-duration state events and multi-event/long-segment encoding are
    /// deliberately outside this DTMF-only primitive.
    pub fn new(digit: Digit, end: bool, volume: u8, duration: u16) -> Result<Self, MediaError> {
        if volume > 63 || duration == 0 {
            return Err(MediaError::InvalidEvent);
        }
        Ok(Self {
            digit,
            end,
            volume,
            duration,
        })
    }
    /// Parse exactly one four-byte DTMF event; ignore the received reserved bit.
    ///
    /// # Errors
    /// Returns [`MediaError::InvalidEvent`] for malformed/non-DTMF payloads.
    pub fn parse(bytes: &[u8]) -> Result<Self, MediaError> {
        if bytes.len() != 4 {
            return Err(MediaError::InvalidEvent);
        }
        Self::new(
            Digit::from_event(bytes[0])?,
            bytes[1] & 128 != 0,
            bytes[1] & 63,
            u16::from_be_bytes([bytes[2], bytes[3]]),
        )
    }
    /// Serialize without allocation, always clearing the reserved bit.
    pub const fn to_bytes(self) -> [u8; 4] {
        let duration = self.duration.to_be_bytes();
        [
            self.digit.0,
            self.volume | if self.end { 128 } else { 0 },
            duration[0],
            duration[1],
        ]
    }
    /// Return event duration in the negotiated 8 kHz clock's ticks.
    pub const fn duration(self) -> u16 {
        self.duration
    }
    /// Whether this packet reports the end of this event.
    pub const fn is_end(self) -> bool {
        self.end
    }
}

/// One scheduled payload and the RTP marker flag to use for it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EventPacket {
    /// True only on the first packet of the event.
    pub marker: bool,
    /// Validated single-event payload.
    pub event: TelephoneEvent,
}

/// Finite plan for one short DTMF event, ending with three end reports.
///
/// Callers own pacing/SSRC/sequence/timestamp and cancellation. This never
/// represents an unlimited key hold: total duration is bounded to u16 ticks;
/// longer segmented events require a separate future implementation.
pub struct Transmission {
    digit: Digit,
    volume: u8,
    total: u16,
    step: u16,
    duration: u16,
    end_reports: u8,
    first: bool,
}
impl Transmission {
    /// Create a plan with nonzero total/step ticks and step <= total.
    ///
    /// # Errors
    /// Returns [`MediaError::InvalidEvent`] for invalid durations or volume.
    pub fn new(digit: Digit, volume: u8, total: u16, step: u16) -> Result<Self, MediaError> {
        if volume > 63 || total == 0 || step == 0 || step > total {
            return Err(MediaError::InvalidEvent);
        }
        Ok(Self {
            digit,
            volume,
            total,
            step,
            duration: 0,
            end_reports: 0,
            first: true,
        })
    }
}
impl Iterator for Transmission {
    type Item = EventPacket;
    fn next(&mut self) -> Option<Self::Item> {
        if self.end_reports == 3 {
            return None;
        }
        self.duration = self.duration.saturating_add(self.step).min(self.total);
        let end = self.duration == self.total;
        if end {
            self.end_reports += 1;
        }
        let marker = std::mem::replace(&mut self.first, false);
        Some(EventPacket {
            marker,
            event: TelephoneEvent {
                digit: self.digit,
                volume: self.volume,
                duration: self.duration,
                end,
            },
        })
    }
}
impl std::iter::FusedIterator for Transmission {}
