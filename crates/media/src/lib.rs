//! Allocation-free G.711 and bounded RTP packet primitives for G0.
//!
//! No device I/O, network, codec dependency or cryptography is installed here.
//! This does not yet provide SRTP, RTCP, resampling or production playout/jitter.
use std::{error::Error, fmt};

/// G.711 mono 8 kHz codec with the static RTP/AVP payload mapping.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Codec {
    /// A-law, static payload type 8.
    Pcma,
    /// Mu-law, static payload type 0.
    Pcmu,
}
impl Codec {
    /// Return the static RTP payload number from RFC 3551.
    pub const fn payload_type(self) -> u8 {
        match self {
            Self::Pcma => 8,
            Self::Pcmu => 0,
        }
    }
    /// Return a fixed non-sensitive SDP encoding label.
    pub const fn name(self) -> &'static str {
        match self {
            Self::Pcma => "PCMA",
            Self::Pcmu => "PCMU",
        }
    }
    /// Quantize signed 16-bit PCM into one G.711 octet without allocation.
    ///
    /// Negative quantization uses an arithmetic shift before magnitude/sign
    /// encoding. This matches the independently generated 13/14-bit reference.
    pub fn encode_sample(self, sample: i16) -> u8 {
        let sample = i32::from(sample);
        match self {
            Self::Pcma => {
                let quantized = sample >> 3;
                let magnitude = if quantized < 0 {
                    -quantized - 1
                } else {
                    quantized
                } as u32;
                let segment = if magnitude < 32 {
                    0
                } else {
                    magnitude.ilog2() - 4
                };
                let mantissa = if segment < 2 {
                    magnitude >> 1
                } else {
                    magnitude >> segment
                } & 15;
                let sign_mask = if sample < 0 { 0x55 } else { 0xd5 };
                ((segment << 4) | mantissa) as u8 ^ sign_mask
            }
            Self::Pcmu => {
                let quantized = sample >> 2;
                let magnitude = quantized.unsigned_abs().min(8158) + 33;
                let segment = if magnitude < 64 {
                    0
                } else {
                    magnitude.ilog2() - 5
                };
                let mantissa = (magnitude >> (segment + 1)) & 15;
                let sign_mask = if sample < 0 { 0x7f } else { 0xff };
                ((segment << 4) | mantissa) as u8 ^ sign_mask
            }
        }
    }
    /// Decode every possible G.711 octet to signed 16-bit PCM.
    pub fn decode_sample(self, octet: u8) -> i16 {
        match self {
            Self::Pcma => {
                let bits = octet ^ 0x55;
                let segment = (bits >> 4) & 7;
                let mantissa = i32::from(bits & 15) << 4;
                let value = match segment {
                    0 => mantissa + 8,
                    1 => mantissa + 264,
                    _ => (mantissa + 264) << (segment - 1),
                };
                if bits & 128 != 0 {
                    value as i16
                } else {
                    -value as i16
                }
            }
            Self::Pcmu => {
                let bits = !octet;
                let value = ((i32::from(bits & 15) << 3) + 132) << ((bits >> 4) & 7);
                if bits & 128 != 0 {
                    (132 - value) as i16
                } else {
                    (value - 132) as i16
                }
            }
        }
    }
}

/// Sanitized packet validation failure; never contains audio or input bytes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MediaError {
    /// Truncated, oversized, malformed or unsupported RTP header/length.
    InvalidPacket,
    /// Caller-provided output buffer is too small.
    BufferTooSmall,
}
impl fmt::Display for MediaError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::InvalidPacket => "invalid bounded RTP packet",
            Self::BufferTooSmall => "RTP output buffer too small",
        })
    }
}
impl Error for MediaError {}

/// RTP v2 fixed header. The caller owns randomized session values and rollover.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RtpHeader {
    /// First packet of a talkspurt when the profile requires it.
    pub marker: bool,
    /// Seven-bit negotiated payload type.
    pub payload_type: u8,
    /// Sequence number, incremented modulo 65536 per packet.
    pub sequence: u16,
    /// Sampling-clock timestamp, incremented modulo 2^32.
    pub timestamp: u32,
    /// Synchronization source identifier chosen by the caller.
    pub ssrc: u32,
}
impl RtpHeader {
    /// Serialize a fixed header and payload into caller-owned storage.
    ///
    /// # Errors
    /// Returns [`MediaError`] for insufficient output, more than 1500 total
    /// octets, or an invalid seven-bit payload type. Never partially writes on error.
    pub fn write(self, payload: &[u8], output: &mut [u8]) -> Result<usize, MediaError> {
        let length = payload
            .len()
            .checked_add(12)
            .ok_or(MediaError::InvalidPacket)?;
        if self.payload_type > 127 || length > 1500 {
            return Err(MediaError::InvalidPacket);
        }
        if output.len() < length {
            return Err(MediaError::BufferTooSmall);
        }
        output[0] = 0x80;
        output[1] = self.payload_type | if self.marker { 0x80 } else { 0 };
        output[2..4].copy_from_slice(&self.sequence.to_be_bytes());
        output[4..8].copy_from_slice(&self.timestamp.to_be_bytes());
        output[8..12].copy_from_slice(&self.ssrc.to_be_bytes());
        output[12..length].copy_from_slice(payload);
        Ok(length)
    }
}

/// Borrowed, validated RTP packet; parsing never allocates or retains audio.
/// Deliberately lacks Debug to prevent accidental payload logging.
pub struct RtpPacket<'a> {
    /// Parsed fixed header.
    pub header: RtpHeader,
    /// Audio/event bytes, excluding CSRC, extension and padding.
    pub payload: &'a [u8],
}
impl<'a> RtpPacket<'a> {
    /// Parse RTP v2 with a 1500-octet bound, CSRCs, extension and padding.
    ///
    /// # Errors
    /// Returns [`MediaError::InvalidPacket`] for malformed lengths or version.
    /// Payload types, SSRC/source address, sequence and SDP policy are checked
    /// by the receiving session, not inferred from arbitrary incoming bytes.
    pub fn parse(bytes: &'a [u8]) -> Result<Self, MediaError> {
        let invalid = MediaError::InvalidPacket;
        if bytes.len() < 12 || bytes.len() > 1500 || bytes[0] >> 6 != 2 {
            return Err(invalid);
        }
        let mut offset = 12 + usize::from(bytes[0] & 15) * 4;
        if bytes.len() < offset {
            return Err(invalid);
        }
        if bytes[0] & 16 != 0 {
            if bytes.len() < offset + 4 {
                return Err(invalid);
            }
            let words = u16::from_be_bytes([bytes[offset + 2], bytes[offset + 3]]);
            offset += 4 + usize::from(words) * 4;
            if bytes.len() < offset {
                return Err(invalid);
            }
        }
        let mut end = bytes.len();
        if bytes[0] & 32 != 0 {
            let padding = usize::from(bytes[end - 1]);
            if padding == 0 || padding > end - offset {
                return Err(invalid);
            }
            end -= padding;
        }
        Ok(Self {
            header: RtpHeader {
                marker: bytes[1] & 128 != 0,
                payload_type: bytes[1] & 127,
                sequence: u16::from_be_bytes([bytes[2], bytes[3]]),
                timestamp: u32::from_be_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]),
                ssrc: u32::from_be_bytes([bytes[8], bytes[9], bytes[10], bytes[11]]),
            },
            payload: &bytes[offset..end],
        })
    }
}
