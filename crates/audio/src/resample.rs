//! Deterministic streaming sample-rate converter (windowed-sinc).
//!
//! Each output sample is a Blackman-windowed sinc interpolation of the input
//! around its exact rational position. The low-pass cutoff is 90 % of the
//! lower Nyquist frequency (3.6 kHz when either side is 8 kHz), so the same
//! filter is the anti-alias filter when downsampling and the anti-image filter
//! when upsampling. Taps are normalized per output so DC is preserved exactly.
//! Positions are tracked as integers (`in_rate`/`out_rate` ratio), so there is
//! no floating-point phase drift. Latency is `half_width` input samples
//! (about 2.2 ms at 48 kHz → 8 kHz and 8 kHz → 48 kHz).

/// Sinc zero crossings on each side of the kernel center.
const ZERO_CROSSINGS: usize = 16;
/// Kernel table resolution per zero crossing (linearly interpolated).
const TABLE_RESOLUTION: usize = 256;
/// Cutoff as a fraction of the lower Nyquist frequency.
const CUTOFF: f64 = 0.9;

/// Streaming converter from `in_rate` to `out_rate` mono samples.
#[derive(Debug)]
pub struct Resampler {
    in_rate: u64,
    out_rate: u64,
    /// Input samples on each side of an output position.
    half: usize,
    /// Kernel argument (in zero crossings) per input-sample distance.
    scale: f64,
    /// `sinc(u) * blackman(u / ZERO_CROSSINGS)` for `u` in `0..=ZERO_CROSSINGS`.
    table: Box<[f32]>,
    /// Retained input; `history[0]` is the oldest sample still needed.
    history: Vec<f32>,
    /// Next output position in `history`, in units of `1 / out_rate` input samples.
    pos: u64,
}

impl Resampler {
    /// Create a converter. Rates of zero are treated as 1 Hz (never panics).
    pub fn new(in_rate: u32, out_rate: u32) -> Self {
        let (in_rate, out_rate) = (u64::from(in_rate.max(1)), u64::from(out_rate.max(1)));
        // Cutoff in cycles per input sample.
        let fc = 0.5 * CUTOFF * (out_rate as f64 / in_rate as f64).min(1.0);
        let scale = 2.0 * fc;
        let half = (ZERO_CROSSINGS as f64 / scale).ceil() as usize;
        let table = (0..=ZERO_CROSSINGS * TABLE_RESOLUTION)
            .map(|i| {
                let u = i as f64 / TABLE_RESOLUTION as f64;
                let sinc = if i == 0 {
                    1.0
                } else {
                    (std::f64::consts::PI * u).sin() / (std::f64::consts::PI * u)
                };
                let x = std::f64::consts::PI * u / ZERO_CROSSINGS as f64;
                let blackman = 0.42 + 0.5 * x.cos() + 0.08 * (2.0 * x).cos();
                (sinc * blackman) as f32
            })
            .collect();
        let mut history = Vec::with_capacity(16_384);
        history.resize(half, 0.0);
        Self {
            in_rate,
            out_rate,
            half,
            scale,
            table,
            history,
            pos: half as u64 * out_rate,
        }
    }

    /// Feed `input` and call `emit` for every output sample that became computable.
    ///
    /// Allocation happens only if one call feeds more than ~16k input samples.
    // ponytail: direct-form FIR with interpolated table (~3.5M taps/s for a 48 kHz call);
    // switch to a precomputed polyphase bank if CPU use matters.
    pub fn process(&mut self, input: &[f32], mut emit: impl FnMut(f32)) {
        self.history
            .extend(input.iter().map(|x| if x.is_finite() { *x } else { 0.0 }));
        loop {
            let center = (self.pos / self.out_rate) as usize;
            if center + self.half >= self.history.len() {
                break;
            }
            let frac = (self.pos % self.out_rate) as f64 / self.out_rate as f64;
            let (mut acc, mut norm) = (0.0_f64, 0.0_f64);
            for j in center + 1 - self.half..=center + self.half {
                let distance = ((j as f64 - center as f64) - frac).abs();
                let w = f64::from(self.kernel(distance * self.scale));
                acc += f64::from(self.history.get(j).copied().unwrap_or(0.0)) * w;
                norm += w;
            }
            emit(if norm > 0.0 { (acc / norm) as f32 } else { 0.0 });
            self.pos += self.in_rate;
        }
        let consumed = ((self.pos / self.out_rate) as usize).saturating_sub(self.half);
        self.history.drain(..consumed.min(self.history.len()));
        self.pos -= consumed as u64 * self.out_rate;
    }

    fn kernel(&self, u: f64) -> f32 {
        let x = u * TABLE_RESOLUTION as f64;
        let i = x as usize;
        match (self.table.get(i), self.table.get(i + 1)) {
            (Some(a), Some(b)) => a + (b - a) * (x - i as f64) as f32,
            (Some(a), None) => *a,
            _ => 0.0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(freq: f64, rate: f64, len: usize, amp: f64) -> Vec<f32> {
        (0..len)
            .map(|n| (amp * (2.0 * std::f64::consts::PI * freq * n as f64 / rate).sin()) as f32)
            .collect()
    }

    fn run(r: &mut Resampler, input: &[f32], chunk: usize) -> Vec<f32> {
        let mut out = Vec::new();
        for c in input.chunks(chunk) {
            r.process(c, |y| out.push(y));
        }
        out
    }

    fn rms(x: &[f32]) -> f64 {
        (x.iter().map(|v| f64::from(*v).powi(2)).sum::<f64>() / x.len() as f64).sqrt()
    }

    /// Frequency estimate from positive-going zero crossings.
    fn frequency(x: &[f32], rate: f64) -> f64 {
        let crossings: Vec<usize> = (1..x.len())
            .filter(|&i| x[i - 1] < 0.0 && x[i] >= 0.0)
            .collect();
        let (first, last) = (crossings[0], crossings[crossings.len() - 1]);
        (crossings.len() - 1) as f64 * rate / (last - first) as f64
    }

    #[test]
    fn tone_survives_48k_to_8k_to_48k() {
        let input = tone(1000.0, 48_000.0, 48_000, 0.5);
        let down = run(&mut Resampler::new(48_000, 8_000), &input, 960);
        assert!((down.len() as i64 - 8_000).abs() < 40, "{}", down.len());
        let up = run(&mut Resampler::new(8_000, 48_000), &down, 160);
        let mid = &up[4_800..up.len() - 4_800];
        assert!((frequency(&down[800..7_000], 8_000.0) - 1000.0).abs() < 1.0);
        assert!((frequency(mid, 48_000.0) - 1000.0).abs() < 1.0);
        let gain = rms(mid) / (0.5 / 2f64.sqrt());
        assert!((gain - 1.0).abs() < 0.02, "gain {gain}");
    }

    #[test]
    fn tone_survives_44k1_to_8k() {
        let input = tone(1000.0, 44_100.0, 44_100, 0.5);
        let down = run(&mut Resampler::new(44_100, 8_000), &input, 441);
        assert!((down.len() as i64 - 8_000).abs() < 40, "{}", down.len());
        assert!((frequency(&down[800..7_000], 8_000.0) - 1000.0).abs() < 1.0);
        assert!((rms(&down[800..7_000]) / (0.5 / 2f64.sqrt()) - 1.0).abs() < 0.02);
    }

    #[test]
    fn content_above_new_nyquist_is_rejected() {
        // 6 kHz would alias to 2 kHz at 8 kHz without the low-pass.
        let input = tone(6000.0, 48_000.0, 48_000, 0.5);
        let down = run(&mut Resampler::new(48_000, 8_000), &input, 960);
        let ratio = rms(&down[800..7_000]) / (0.5 / 2f64.sqrt());
        assert!(ratio < 0.01, "alias attenuation only {ratio}");
    }

    #[test]
    fn dc_and_silence_are_preserved_and_garbage_is_bounded() {
        let mut r = Resampler::new(48_000, 8_000);
        let out = run(&mut r, &vec![0.25; 9_600], 480);
        assert!(out[100..].iter().all(|y| (y - 0.25).abs() < 1e-5));
        let out = run(&mut Resampler::new(8_000, 44_100), &[0.0; 1_600], 160);
        assert!(out.iter().all(|y| *y == 0.0));
        let mut junk = vec![f32::NAN, f32::INFINITY, 1.0, -1.0];
        junk.resize(4_000, 1.0);
        let out = run(&mut Resampler::new(8_000, 48_000), &junk, 160);
        assert!(out.iter().all(|y| y.is_finite() && y.abs() < 1.5));
    }
}
