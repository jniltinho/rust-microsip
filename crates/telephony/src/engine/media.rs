//! One call's RTP/SRTP media task: 20 ms capture/encode/send, authenticated
//! receive into a bounded jitter buffer, playout, RFC 4733 DTMF, hold and mute.

use rust_microsip_core::{DtmfKey, FRAME_SAMPLES, MediaIo};
use rust_microsip_media::{
    Codec, RtpHeader, RtpPacket,
    dtmf::{Digit, Transmission},
    srtp_adapter::{MasterKey, ReceiverBank, SecureError, SecureSender},
};
use std::{
    collections::BTreeMap,
    net::SocketAddr,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::{net::UdpSocket, sync::mpsc, time::MissedTickBehavior};

/// Shared device bridge; locked only by the single active media task.
pub(super) type SharedMedia = Arc<Mutex<Box<dyn MediaIo>>>;

/// Frames buffered before playout starts, and the hard bound.
const PREFILL: usize = 3;
const MAX_BUFFERED: usize = 25;

/// Bounded reordering playout buffer keyed by extended sequence number.
#[derive(Default)]
pub(super) struct JitterBuffer {
    frames: BTreeMap<u64, [i16; FRAME_SAMPLES]>,
    next: Option<u64>,
    playing: bool,
    highest: Option<u64>,
    source: Option<u32>,
    late_streak: u32,
    /// Frames discarded as late, duplicate or overflow.
    pub dropped: u64,
    /// Ticks without a frame while playing.
    pub underruns: u64,
}
impl JitterBuffer {
    fn extend(&self, sequence: u16) -> u64 {
        let Some(highest) = self.highest else {
            return u64::from(sequence) + (1 << 16);
        };
        let base = highest & !0xffff;
        let candidates = [base.wrapping_sub(1 << 16), base, base + (1 << 16)];
        candidates
            .into_iter()
            .map(|b| b | u64::from(sequence))
            .min_by_key(|c| c.abs_diff(highest))
            .unwrap_or(highest)
    }
    /// Insert one decoded frame; late, duplicate and overflow frames are dropped.
    /// A new source (PBX bridge change, transfer) or a long run of "late"
    /// frames restarts the buffer instead of discarding the new stream.
    pub(super) fn push(&mut self, source: u32, sequence: u16, frame: [i16; FRAME_SAMPLES]) {
        if self.source.is_some_and(|s| s != source) || self.late_streak >= 25 {
            let (dropped, underruns) = (self.dropped, self.underruns);
            *self = Self {
                dropped,
                underruns,
                ..Self::default()
            };
        }
        self.source = Some(source);
        let index = self.extend(sequence);
        if self.next.is_some_and(|next| index < next) || self.frames.contains_key(&index) {
            self.dropped += 1;
            self.late_streak += 1;
            return;
        }
        self.late_streak = 0;
        self.highest = Some(self.highest.map_or(index, |h| h.max(index)));
        self.frames.insert(index, frame);
        while self.frames.len() > MAX_BUFFERED {
            self.frames.pop_first();
            self.dropped += 1;
            self.next = self.frames.keys().next().copied();
        }
    }
    /// The next frame to play, or `None` (silence) while prefilling/underrunning.
    pub(super) fn pop(&mut self) -> Option<[i16; FRAME_SAMPLES]> {
        if !self.playing {
            if self.frames.len() < PREFILL {
                return None;
            }
            self.playing = true;
            self.next = self.frames.keys().next().copied();
        }
        let next = self.next?;
        self.next = Some(next + 1);
        match self.frames.remove(&next) {
            Some(frame) => Some(frame),
            None => {
                self.underruns += 1;
                if self.frames.is_empty() {
                    // Rebuffer after a gap instead of drifting ahead of the stream.
                    self.playing = false;
                }
                None
            }
        }
    }
}

/// Runtime control of a running media task.
pub(super) enum MediaControl {
    Mute(bool),
    /// Send direction / receive direction after a hold change.
    Flow {
        send: bool,
        receive: bool,
    },
    Dtmf(DtmfKey),
    /// New remote address and, for SRTP, a new remote key epoch.
    Remote(SocketAddr, Option<MasterKey>),
}

pub(super) struct MediaParams {
    pub socket: Arc<UdpSocket>,
    pub remote: SocketAddr,
    pub codec: Codec,
    pub event_pt: Option<u8>,
    pub local_key: Option<MasterKey>,
    pub remote_key: Option<MasterKey>,
    pub send: bool,
    pub receive: bool,
}

/// Counters reported when the task ends.
#[derive(Debug, Default)]
pub(super) struct MediaStats {
    pub sent: u64,
    pub received: u64,
    pub rejected: u64,
    /// Why the task stopped on its own; `None` when the call ended it.
    pub failure: Option<MediaFailure>,
}

/// A media fault that must end the call instead of leaving it silent.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum MediaFailure {
    /// No authenticated RTP while receiving for [`INACTIVITY`].
    Inactivity,
    /// The SRTP context could not be created or closed itself.
    Crypto,
}

/// RTP silence tolerated while receive is active. Peers that suppress
/// silence for longer would trip it.
// ponytail: fixed 30 s; use RTCP liveness or a setting if VAD peers need it.
pub(super) const INACTIVITY: Duration = Duration::from_secs(30);

fn ssrc() -> u32 {
    let mut bytes = [0; 4];
    // A failed RNG still yields a valid (fixed) SSRC; SRTP keys never come from here.
    ring::rand::SecureRandom::fill(&ring::rand::SystemRandom::new(), &mut bytes).ok();
    u32::from_be_bytes(bytes) | 1
}

struct Sender {
    socket: Arc<UdpSocket>,
    remote: SocketAddr,
    srtp: Option<SecureSender>,
    ssrc: u32,
    sequence: u16,
    timestamp: u32,
    marker: bool,
}
impl Sender {
    async fn send(&mut self, pt: u8, marker: bool, timestamp: u32, payload: &[u8]) -> bool {
        let mut clear = [0u8; 12 + FRAME_SAMPLES];
        let Ok(length) = (RtpHeader {
            marker,
            payload_type: pt,
            sequence: self.sequence,
            timestamp,
            ssrc: self.ssrc,
        })
        .write(payload, &mut clear) else {
            return false;
        };
        self.sequence = self.sequence.wrapping_add(1);
        let wire = match &mut self.srtp {
            Some(srtp) => match srtp.protect_rtp(&clear[..length]) {
                Ok(wire) => wire,
                Err(_) => return false,
            },
            None => clear[..length].to_vec(),
        };
        self.socket.send_to(&wire, self.remote).await.is_ok()
    }
}

/// Open the devices for a call's media (blocking work off the workers).
/// Returns `false` when no device could be opened, and the guard that
/// releases them. The guard is made on the blocking thread right after
/// `start`, so a task aborted while waiting here still releases the devices:
/// tokio drops the output of a blocking task whose handle is gone.
pub(super) async fn open_devices(media: &SharedMedia) -> (bool, Option<DeviceGuard>) {
    let media = Arc::clone(media);
    tokio::task::spawn_blocking(move || {
        // Made before `start` so a panic inside it still releases the devices.
        let guard = DeviceGuard(Arc::clone(&media));
        let opened = lock_media(&media).start();
        (opened, Some(guard))
    })
    .await
    .unwrap_or((false, None))
}

fn lock_media(media: &SharedMedia) -> std::sync::MutexGuard<'_, Box<dyn MediaIo>> {
    media
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Releases the devices when a call's media task ends — also when the task
/// is aborted at the end of a call, which skips any code after an `.await`.
pub(super) struct DeviceGuard(SharedMedia);
impl Drop for DeviceGuard {
    fn drop(&mut self) {
        let media = Arc::clone(&self.0);
        let stop = move || lock_media(&media).stop();
        // Closing streams joins device threads: keep it off the async workers.
        // Order against the next call's `start` does not matter: `MediaIo`
        // implementations count users and close only when the last one stops.
        // ponytail: during runtime shutdown the blocking task may not run; the
        // process is exiting then and the OS closes the devices.
        match tokio::runtime::Handle::try_current() {
            Ok(handle) => drop(handle.spawn_blocking(stop)),
            Err(_) => stop(),
        }
    }
}

/// Run until `control` closes. The device bridge is used only on the tick.
pub(super) async fn run(
    params: MediaParams,
    media: SharedMedia,
    mut control: mpsc::Receiver<MediaControl>,
) -> MediaStats {
    let codec = params.codec;
    let ssrc = ssrc();
    let srtp = match params.local_key {
        Some(key) => SecureSender::new(ssrc, key).ok(),
        None => None,
    };
    let secure = params.remote_key.is_some();
    let mut bank = params
        .remote_key
        .map(|key| ReceiverBank::new(key, std::time::Instant::now()));
    let mut sender = Sender {
        socket: params.socket.clone(),
        remote: params.remote,
        srtp,
        ssrc,
        sequence: (ssrc >> 16) as u16 & 0x7fff,
        timestamp: ssrc.rotate_left(7),
        marker: true,
    };
    if secure && sender.srtp.is_none() {
        // Fail closed: never fall back to plaintext on a secure call.
        return MediaStats {
            failure: Some(MediaFailure::Crypto),
            ..MediaStats::default()
        };
    }
    let (mut send, mut receive, mut muted) = (params.send, params.receive, false);
    let mut stats = MediaStats::default();
    let mut jitter = JitterBuffer::default();
    let mut dtmf: Option<(Transmission, u32)> = None;
    let mut tick = tokio::time::interval(Duration::from_millis(20));
    tick.set_missed_tick_behavior(MissedTickBehavior::Delay);
    let mut buffer = [0u8; 1600];
    let mut heard = tokio::time::Instant::now();
    let mut frame = [0i16; FRAME_SAMPLES];
    let mut encoded = [0u8; FRAME_SAMPLES];
    loop {
        tokio::select! {
            command = control.recv() => match command {
                None => break,
                Some(MediaControl::Mute(on)) => muted = on,
                Some(MediaControl::Flow { send: s, receive: r }) => {
                    heard = tokio::time::Instant::now();
                    send = s;
                    receive = r;
                    jitter = JitterBuffer::default();
                    sender.marker = true;
                }
                Some(MediaControl::Dtmf(key)) => {
                    if let (Some(_), true, Ok(digit)) = (params.event_pt, send, Digit::from_event(key.event()))
                        && let Ok(plan) = Transmission::new(digit, 10, 960, 160)
                    {
                        dtmf = Some((plan, sender.timestamp));
                    }
                }
                Some(MediaControl::Remote(address, key)) => {
                    heard = tokio::time::Instant::now();
                    sender.remote = address;
                    if let Some(key) = key {
                        bank = Some(ReceiverBank::new(key, std::time::Instant::now()));
                    }
                    jitter = JitterBuffer::default();
                }
            },
            _ = tick.tick() => {
                if receive && heard.elapsed() > INACTIVITY {
                    stats.failure = Some(MediaFailure::Inactivity);
                    break;
                }
                let captured = media.lock().map(|mut io| io.capture(&mut frame)).unwrap_or(false);
                if !captured || muted {
                    frame.fill(0);
                }
                if send {
                    if let Some((plan, timestamp)) = dtmf.as_mut() {
                        match (plan.next(), params.event_pt) {
                            (Some(packet), Some(pt)) => {
                                if sender.send(pt, packet.marker, *timestamp, &packet.event.to_bytes()).await {
                                    stats.sent += 1;
                                }
                            }
                            _ => dtmf = None,
                        }
                    }
                    if dtmf.is_none() {
                        for (out, sample) in encoded.iter_mut().zip(frame) {
                            *out = codec.encode_sample(sample);
                        }
                        let marker = std::mem::replace(&mut sender.marker, false);
                        let timestamp = sender.timestamp;
                        if sender.send(codec.payload_type(), marker, timestamp, &encoded).await {
                            stats.sent += 1;
                        }
                    }
                }
                sender.timestamp = sender.timestamp.wrapping_add(FRAME_SAMPLES as u32);
                let played = if receive { jitter.pop() } else { None };
                if let Ok(mut io) = media.lock() {
                    io.play(&played.unwrap_or([0; FRAME_SAMPLES]));
                }
            },
            result = params.socket.recv_from(&mut buffer) => {
                let Ok((length, source)) = result else { continue };
                // Only the negotiated media address is accepted (no latching).
                if source != sender.remote || !receive {
                    continue;
                }
                let clear = match bank.as_mut() {
                    Some(bank) => match bank.unprotect_rtp(&buffer[..length], std::time::Instant::now()) {
                        Ok(clear) => clear,
                        Err(SecureError::Closed) => {
                            stats.failure = Some(MediaFailure::Crypto);
                            break;
                        }
                        Err(_) => { stats.rejected += 1; continue; }
                    },
                    None if secure => {
                        stats.failure = Some(MediaFailure::Crypto);
                        break;
                    }
                    None => buffer[..length].to_vec(),
                };
                let Ok(packet) = RtpPacket::parse(&clear) else { stats.rejected += 1; continue };
                // Any authentic RTP (comfort noise, events, other ptime) proves liveness.
                heard = tokio::time::Instant::now();
                if packet.header.payload_type != codec.payload_type() || packet.payload.len() != FRAME_SAMPLES {
                    continue;
                }
                let mut decoded = [0i16; FRAME_SAMPLES];
                for (out, octet) in decoded.iter_mut().zip(packet.payload) {
                    *out = codec.decode_sample(*octet);
                }
                jitter.push(packet.header.ssrc, packet.header.sequence, decoded);
                stats.received += 1;
            }
        }
    }
    stats
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(value: i16) -> [i16; FRAME_SAMPLES] {
        [value; FRAME_SAMPLES]
    }

    #[test]
    fn jitter_buffer_reorders_prefills_and_drops_late_duplicates() {
        let mut buffer = JitterBuffer::default();
        buffer.push(7, 10, frame(10));
        buffer.push(7, 12, frame(12));
        assert!(buffer.pop().is_none(), "prefill");
        buffer.push(7, 11, frame(11));
        assert_eq!(buffer.pop().map(|f| f[0]), Some(10));
        assert_eq!(buffer.pop().map(|f| f[0]), Some(11));
        buffer.push(7, 11, frame(11));
        buffer.push(7, 9, frame(9));
        assert_eq!(buffer.dropped, 2);
        assert_eq!(buffer.pop().map(|f| f[0]), Some(12));
        assert!(buffer.pop().is_none());
        assert_eq!(buffer.underruns, 1);
    }

    #[test]
    fn jitter_buffer_handles_sequence_wrap_and_bound() {
        let mut buffer = JitterBuffer::default();
        for (i, sequence) in [65534u16, 65535, 0, 1].into_iter().enumerate() {
            buffer.push(7, sequence, frame(i as i16));
        }
        let played: Vec<i16> = (0..4).filter_map(|_| buffer.pop().map(|f| f[0])).collect();
        assert_eq!(played, vec![0, 1, 2, 3]);
        let mut buffer = JitterBuffer::default();
        for sequence in 0..100u16 {
            buffer.push(7, sequence, frame(1));
        }
        assert!(buffer.frames.len() <= MAX_BUFFERED);
        assert_eq!(buffer.dropped, 100 - MAX_BUFFERED as u64);
    }

    #[test]
    fn jitter_buffer_restarts_on_new_source_or_persistent_lateness() {
        let mut buffer = JitterBuffer::default();
        for sequence in 40000..40010u16 {
            buffer.push(1, sequence, frame(1));
        }
        for _ in 0..5 {
            buffer.pop();
        }
        // A bridged stream with a lower sequence and a new SSRC must play.
        for sequence in 100..104u16 {
            buffer.push(2, sequence, frame(2));
        }
        assert_eq!(buffer.pop().map(|f| f[0]), Some(2));
        // Same SSRC but a sequence jump backwards recovers after a bounded streak.
        let mut buffer = JitterBuffer::default();
        for sequence in 40000..40004u16 {
            buffer.push(1, sequence, frame(1));
        }
        buffer.pop();
        for sequence in 10..40u16 {
            buffer.push(1, sequence, frame(3));
        }
        let played: Vec<i16> = (0..10).filter_map(|_| buffer.pop().map(|f| f[0])).collect();
        assert!(played.contains(&3));
    }

    struct Tone(u32);
    impl MediaIo for Tone {
        fn capture(&mut self, frame: &mut [i16; FRAME_SAMPLES]) -> bool {
            for sample in frame.iter_mut() {
                *sample = if self.0 % 8 < 4 { 4000 } else { -4000 };
                self.0 += 1;
            }
            true
        }
        fn play(&mut self, _: &[i16; FRAME_SAMPLES]) {}
    }

    #[tokio::test]
    async fn loopback_plain_and_secure_media_and_dtmf() -> Result<(), Box<dyn std::error::Error>> {
        for secure in [false, true] {
            let a = Arc::new(UdpSocket::bind("127.0.0.1:0").await?);
            let peer = UdpSocket::bind("127.0.0.1:0").await?;
            let key = [0x41u8; 30];
            let (tx, rx) = mpsc::channel(4);
            let media: SharedMedia = Arc::new(Mutex::new(Box::new(Tone(0))));
            let task = tokio::spawn(run(
                MediaParams {
                    socket: a.clone(),
                    remote: peer.local_addr()?,
                    codec: Codec::Pcmu,
                    event_pt: Some(101),
                    local_key: secure.then(|| MasterKey::from_bytes(&key)).transpose()?,
                    remote_key: None,
                    send: true,
                    receive: false,
                },
                media,
                rx,
            ));
            let mut buf = [0u8; 1600];
            let (length, _) = peer.recv_from(&mut buf).await?;
            assert_eq!(length, if secure { 12 + 160 + 10 } else { 12 + 160 });
            if !secure {
                let packet = RtpPacket::parse(&buf[..length])?;
                assert_eq!(packet.header.payload_type, 0);
                assert!(packet.header.marker);
            }
            tx.send(MediaControl::Dtmf(DtmfKey::new('5')?)).await?;
            let mut events = 0;
            for _ in 0..30 {
                let (length, _) = peer.recv_from(&mut buf).await?;
                if length == if secure { 12 + 4 + 10 } else { 12 + 4 } {
                    events += 1;
                }
            }
            assert!(events >= 6, "events {events}");
            drop(tx);
            assert!(task.await?.sent > 30);
        }
        Ok(())
    }

    /// Counts non-silent decoded frames handed to playback.
    struct Counter(Arc<std::sync::atomic::AtomicU64>);
    impl MediaIo for Counter {
        fn capture(&mut self, frame: &mut [i16; FRAME_SAMPLES]) -> bool {
            frame.fill(0);
            false
        }
        fn play(&mut self, frame: &[i16; FRAME_SAMPLES]) {
            if frame.iter().any(|s| *s != 0) {
                self.0.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            }
        }
    }

    /// Deterministic xorshift generator: the impairment is reproducible.
    struct Seeded(u64);
    impl Seeded {
        fn next(&mut self) -> u64 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            self.0
        }
    }

    /// AUD-02: a user-space UDP relay drops 1%, delays 0-30 ms (which
    /// reorders 20 ms packets) between two real media tasks. Playback must
    /// stay continuous within the bounded jitter buffer.
    #[tokio::test]
    async fn impaired_network_keeps_audio_flowing_with_bounded_buffers()
    -> Result<(), Box<dyn std::error::Error>> {
        for secure in [false, true] {
            let sender = Arc::new(UdpSocket::bind("127.0.0.1:0").await?);
            let relay = Arc::new(UdpSocket::bind("127.0.0.1:0").await?);
            let receiver = Arc::new(UdpSocket::bind("127.0.0.1:0").await?);
            let key = [0x55u8; 30];
            let played = Arc::new(std::sync::atomic::AtomicU64::new(0));
            let (send_tx, send_rx) = mpsc::channel(4);
            let (recv_tx, recv_rx) = mpsc::channel(4);
            let sending = tokio::spawn(run(
                MediaParams {
                    socket: sender.clone(),
                    remote: relay.local_addr()?,
                    codec: Codec::Pcma,
                    event_pt: None,
                    local_key: secure.then(|| MasterKey::from_bytes(&key)).transpose()?,
                    remote_key: None,
                    send: true,
                    receive: false,
                },
                Arc::new(Mutex::new(Box::new(Tone(0)))),
                send_rx,
            ));
            // The relay forwards to the receiver from its own address, which
            // the receiver treats as the negotiated peer.
            let receiving = tokio::spawn(run(
                MediaParams {
                    socket: receiver.clone(),
                    remote: relay.local_addr()?,
                    codec: Codec::Pcma,
                    event_pt: None,
                    // A secure call always has both directional keys.
                    local_key: secure
                        .then(|| MasterKey::from_bytes(&[0x66; 30]))
                        .transpose()?,
                    remote_key: secure.then(|| MasterKey::from_bytes(&key)).transpose()?,
                    send: false,
                    receive: true,
                },
                Arc::new(Mutex::new(Box::new(Counter(played.clone())))),
                recv_rx,
            ));
            let target = receiver.local_addr()?;
            let forward = relay.clone();
            let relaying = tokio::spawn(async move {
                let mut rng = Seeded(0x9e37_79b9_7f4a_7c15);
                let (mut forwarded, mut dropped) = (0u32, 0u32);
                let mut buf = [0u8; 1600];
                let deadline = tokio::time::Instant::now() + Duration::from_secs(4);
                while let Ok(Ok((length, _))) =
                    tokio::time::timeout_at(deadline, forward.recv_from(&mut buf)).await
                {
                    if rng.next().is_multiple_of(100) {
                        dropped += 1;
                        continue;
                    }
                    let delay = Duration::from_millis(rng.next() % 31);
                    let packet = buf[..length].to_vec();
                    let socket = forward.clone();
                    tokio::spawn(async move {
                        tokio::time::sleep(delay).await;
                        let _ = socket.send_to(&packet, target).await;
                    });
                    forwarded += 1;
                }
                (forwarded, dropped)
            });
            let (forwarded, dropped) = relaying.await?;
            tokio::time::sleep(Duration::from_millis(200)).await;
            drop((send_tx, recv_tx));
            let sent = sending.await?.sent;
            let stats = receiving.await?;
            let played = played.load(std::sync::atomic::Ordering::Relaxed);
            assert!(
                sent >= 180 && forwarded >= 180,
                "sent {sent} forwarded {forwarded}"
            );
            assert!(
                dropped >= 1,
                "the seeded 1% loss must drop at least one packet"
            );
            // Nearly every forwarded frame is authenticated, decoded and played.
            assert!(
                stats.received + 2 >= u64::from(forwarded),
                "received {} of {forwarded}",
                stats.received
            );
            assert!(
                played * 100 >= u64::from(forwarded) * 90,
                "played {played} of {forwarded}"
            );
            assert!(stats.failure.is_none());
        }
        Ok(())
    }

    #[tokio::test]
    async fn devices_open_only_for_a_call_and_are_released_after() {
        use std::sync::atomic::{AtomicU32, Ordering};
        struct Counted(Arc<(AtomicU32, AtomicU32)>, bool);
        impl MediaIo for Counted {
            fn capture(&mut self, _: &mut [i16; FRAME_SAMPLES]) -> bool {
                false
            }
            fn play(&mut self, _: &[i16; FRAME_SAMPLES]) {}
            fn start(&mut self) -> bool {
                self.0.0.fetch_add(1, Ordering::SeqCst);
                self.1
            }
            fn stop(&mut self) {
                self.0.1.fetch_add(1, Ordering::SeqCst);
            }
        }
        let counts = Arc::new((AtomicU32::new(0), AtomicU32::new(0)));
        let media: SharedMedia = Arc::new(Mutex::new(Box::new(Counted(Arc::clone(&counts), true))));
        assert_eq!(
            counts.0.load(Ordering::SeqCst),
            0,
            "nothing opened at creation"
        );
        let (opened, guard) = open_devices(&media).await;
        assert!(opened);
        drop(guard);
        settle().await;
        assert_eq!(
            (
                counts.0.load(Ordering::SeqCst),
                counts.1.load(Ordering::SeqCst)
            ),
            (1, 1)
        );
        // An aborted media task still releases the devices.
        let held = Arc::clone(&media);
        let task = tokio::spawn(async move {
            let _guard = open_devices(&held).await.1;
            std::future::pending::<()>().await;
        });
        tokio::time::sleep(Duration::from_millis(50)).await;
        task.abort();
        let _ = task.await;
        settle().await;
        assert_eq!(counts.1.load(Ordering::SeqCst), 2, "released on abort");
        let missing: SharedMedia =
            Arc::new(Mutex::new(Box::new(Counted(Arc::clone(&counts), false))));
        let (opened, guard) = open_devices(&missing).await;
        assert!(!opened, "a missing device is reported");
        drop(guard);
        settle().await;
        assert_eq!(counts.1.load(Ordering::SeqCst), 3);
    }

    /// Wait for the guard's blocking `stop` to run.
    async fn settle() {
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    #[tokio::test]
    async fn aborted_while_opening_still_releases() {
        use std::sync::atomic::{AtomicU32, Ordering};
        struct Slow(Arc<AtomicU32>);
        impl MediaIo for Slow {
            fn capture(&mut self, _: &mut [i16; FRAME_SAMPLES]) -> bool {
                false
            }
            fn play(&mut self, _: &[i16; FRAME_SAMPLES]) {}
            fn start(&mut self) -> bool {
                std::thread::sleep(Duration::from_millis(200));
                true
            }
            fn stop(&mut self) {
                self.0.fetch_add(1, Ordering::SeqCst);
            }
        }
        let stops = Arc::new(AtomicU32::new(0));
        let media: SharedMedia = Arc::new(Mutex::new(Box::new(Slow(Arc::clone(&stops)))));
        let task = tokio::spawn(async move {
            let _guard = open_devices(&media).await.1;
        });
        tokio::time::sleep(Duration::from_millis(20)).await;
        task.abort();
        let _ = task.await;
        tokio::time::sleep(Duration::from_millis(500)).await;
        assert_eq!(
            stops.load(Ordering::SeqCst),
            1,
            "stop after an aborted open"
        );
    }
}
