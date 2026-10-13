//! Bounded SR/RR + SDES compound helpers for the isolated SRTCP experiment.
//! Does not implement an adaptive RTCP scheduler, RTT estimator or every RTCP type.

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub(super) enum ControlError {
    #[error("invalid isolated RTCP compound packet")]
    Malformed,
}

pub(super) struct Reception {
    pub(super) source: u32,
    pub(super) highest_sequence: u32,
    pub(super) last_sender_report: u32,
}
pub(super) struct Compound {
    pub(super) receptions: Vec<Reception>,
}

fn word(bytes: &[u8], offset: usize) -> u32 {
    // Every caller has checked the complete containing header/block.
    u32::from_be_bytes([
        bytes[offset],
        bytes[offset + 1],
        bytes[offset + 2],
        bytes[offset + 3],
    ])
}

pub(super) fn sender_report(
    source: u32,
    ntp: u64,
    timestamp: u32,
    packets: u32,
    octets: u32,
) -> Vec<u8> {
    let mut output = Vec::with_capacity(64);
    output.extend([0x80, 200, 0, 6]);
    output.extend(source.to_be_bytes());
    output.extend(ntp.to_be_bytes());
    output.extend(timestamp.to_be_bytes());
    output.extend(packets.to_be_bytes());
    output.extend(octets.to_be_bytes());
    let cname = format!("g0-{source:08x}@lab.invalid");
    let mut description = Vec::with_capacity(36);
    description.extend([0x81, 202, 0, 0]);
    description.extend(source.to_be_bytes());
    description.extend([1, cname.len() as u8]);
    description.extend(cname.as_bytes());
    description.push(0);
    while !description.len().is_multiple_of(4) {
        description.push(0);
    }
    let length = (description.len() / 4 - 1) as u16;
    description[2..4].copy_from_slice(&length.to_be_bytes());
    output.extend(description);
    output
}

fn description_has_cname(bytes: &[u8], chunks: usize, reporter: u32) -> Result<bool, ControlError> {
    let bad = ControlError::Malformed;
    let mut cursor = 4;
    let mut found = false;
    for _ in 0..chunks {
        if bytes.len() - cursor < 4 {
            return Err(bad);
        }
        let source = word(bytes, cursor);
        cursor += 4;
        loop {
            let kind = *bytes.get(cursor).ok_or(bad)?;
            cursor += 1;
            if kind == 0 {
                break;
            }
            let length = usize::from(*bytes.get(cursor).ok_or(bad)?);
            cursor += 1;
            if length > bytes.len() - cursor {
                return Err(bad);
            }
            if kind == 1 {
                if length == 0 {
                    return Err(bad);
                }
                if source == reporter {
                    if found {
                        return Err(bad);
                    }
                    found = true;
                }
            }
            cursor += length;
        }
        while !cursor.is_multiple_of(4) {
            if bytes.get(cursor) != Some(&0) {
                return Err(bad);
            }
            cursor += 1;
        }
    }
    if cursor != bytes.len() {
        return Err(bad);
    }
    Ok(found)
}

pub(super) fn parse(bytes: &[u8]) -> Result<Compound, ControlError> {
    let bad = ControlError::Malformed;
    if bytes.len() < 8 || bytes.len() > 1500 || !bytes.len().is_multiple_of(4) {
        return Err(bad);
    }
    let mut offset = 0;
    let mut reporter = None;
    let mut cname = false;
    let mut receptions = Vec::with_capacity(31);
    while offset < bytes.len() {
        if bytes.len() - offset < 4 {
            return Err(bad);
        }
        let first = bytes[offset];
        let count = usize::from(first & 31);
        let kind = bytes[offset + 1];
        let length =
            (usize::from(u16::from_be_bytes([bytes[offset + 2], bytes[offset + 3]])) + 1) * 4;
        if first >> 6 != 2 || length > bytes.len() - offset || !(192..=223).contains(&kind) {
            return Err(bad);
        }
        let mut end = offset + length;
        if first & 0x20 != 0 {
            let padding = usize::from(bytes[end - 1]);
            if end != bytes.len() || padding == 0 || padding > length - 4 {
                return Err(bad);
            }
            end -= padding;
        }
        let packet = &bytes[offset..end];
        if matches!(kind, 200 | 201) {
            let base = if kind == 200 { 28 } else { 8 };
            if packet.len() != base + 24 * count || receptions.len() + count > 31 {
                return Err(bad);
            }
            let source = word(packet, 4);
            if reporter.is_some_and(|known| known != source) {
                return Err(bad);
            }
            reporter = Some(source);
            for block in packet[base..].as_chunks::<24>().0 {
                receptions.push(Reception {
                    source: word(block, 0),
                    highest_sequence: word(block, 8),
                    last_sender_report: word(block, 16),
                });
            }
        } else {
            let reporter = reporter.ok_or(bad)?;
            if kind == 202 {
                let has = description_has_cname(packet, count, reporter)?;
                if has && cname {
                    return Err(bad);
                }
                cname |= has;
            }
            // Unknown subsequent RTCP types are bounded but otherwise ignored.
        }
        offset += length;
    }
    if reporter.is_none() || !cname {
        return Err(bad);
    }
    Ok(Compound { receptions })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn report() -> Vec<u8> {
        sender_report(7, 0x1234_5678_90ab_cdef, 1000, 20, 3200)
    }
    #[test]
    fn emitted_sender_report_has_cname_and_exact_fields() -> Result<(), ControlError> {
        let wire = report();
        assert_eq!(word(&wire, 4), 7);
        assert_eq!(word(&wire, 8), 0x1234_5678);
        assert_eq!(word(&wire, 12), 0x90ab_cdef);
        assert_eq!(word(&wire, 20), 20);
        assert_eq!(word(&wire, 24), 3200);
        assert!(parse(&wire)?.receptions.is_empty());
        Ok(())
    }
    #[test]
    fn receiver_report_decodes_source_highest_sequence_and_lsr() -> Result<(), ControlError> {
        let mut wire = vec![0x81, 201, 0, 7];
        wire.extend(7u32.to_be_bytes());
        let mut block = [0; 24];
        block[0..4].copy_from_slice(&8u32.to_be_bytes());
        block[8..12].copy_from_slice(&66000u32.to_be_bytes());
        block[16..20].copy_from_slice(&0x5678_90abu32.to_be_bytes());
        wire.extend(block);
        wire.extend(&report()[28..]);
        let parsed = parse(&wire)?;
        assert_eq!(parsed.receptions.len(), 1);
        assert_eq!(parsed.receptions[0].source, 8);
        assert_eq!(parsed.receptions[0].highest_sequence, 66000);
        assert_eq!(parsed.receptions[0].last_sender_report, 0x5678_90ab);
        Ok(())
    }
    #[test]
    fn rejects_truncation_and_oversized_compounds() {
        let wire = report();
        for len in 0..wire.len() {
            assert!(parse(&wire[..len]).is_err());
        }
        assert!(parse(&vec![0; 1504]).is_err());
    }
    #[test]
    fn requires_report_first_and_matching_cname() {
        let wire = report();
        assert!(parse(&wire[..28]).is_err());
        assert!(parse(&wire[28..]).is_err());
        let mut wrong = wire;
        wrong[32..36].copy_from_slice(&8u32.to_be_bytes());
        assert!(parse(&wrong).is_err());
    }
    #[test]
    fn rejects_incorrect_report_count_or_header_length() {
        let mut wire = report();
        wire[0] = 0x81;
        assert!(parse(&wire).is_err());
        wire[0] = 0x80;
        wire[3] = 0xff;
        assert!(parse(&wire).is_err());
    }
    #[test]
    fn rejects_nonfinal_or_excessive_padding() {
        let mut wire = report();
        wire[0] |= 0x20;
        assert!(parse(&wire).is_err());
        let mut wire = report();
        wire[28] |= 0x20;
        let last = wire.len() - 1;
        wire[last] = 255;
        assert!(parse(&wire).is_err());
    }
    #[test]
    fn rejects_sdes_truncation_empty_cname_and_alignment_bytes() {
        let mut wire = report();
        wire[37] = 255;
        assert!(parse(&wire).is_err());
        let mut wire = report();
        wire[37] = 0;
        assert!(parse(&wire).is_err());
        let mut wire = report();
        let last = wire.len() - 1;
        wire[last] = 1;
        assert!(parse(&wire).is_err());
    }
    #[test]
    fn rejects_duplicate_cname_for_reporter() {
        let mut wire = report();
        wire.extend(&report()[28..]);
        assert!(parse(&wire).is_err());
    }
    #[test]
    fn ignores_bounded_unknown_subsequent_packet() -> Result<(), ControlError> {
        let mut wire = report();
        wire.extend([0x80, 204, 0, 1, 0, 0, 0, 7]);
        assert!(parse(&wire)?.receptions.is_empty());
        Ok(())
    }
}
