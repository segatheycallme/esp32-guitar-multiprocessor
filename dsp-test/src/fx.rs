use std::{collections::VecDeque, f32::consts::PI};

pub trait Fx: Send + std::fmt::Debug {
    fn process_one(&mut self, x: f32) -> f32;
}

#[derive(Debug)]
pub struct ClippingGain {
    gain: f32,
}

impl ClippingGain {
    pub fn new(gain: f32) -> Self {
        ClippingGain { gain }
    }
}
impl Fx for ClippingGain {
    fn process_one(&mut self, x: f32) -> f32 {
        (x * self.gain).clamp(-1.0, 1.0)
    }
}

#[derive(Debug, Clone)]
pub struct PeakingConstantQBuilder {
    frequency: f32,
    sample_rate: f32,
    quality: f32,
    gain: f32,
}

impl PeakingConstantQBuilder {
    pub fn new(frequency: f32, sample_rate: f32, quality: f32, gain: f32) -> Self {
        PeakingConstantQBuilder {
            frequency,
            sample_rate,
            quality,
            gain,
        }
    }

    pub fn build(&self) -> BiQuad {
        let q = self.quality;
        let k = (PI * self.frequency / self.sample_rate).tan();
        let v0 = 10f32.powf(self.gain / 20.0);
        let d0 = 1.0 + k / q + k * k;
        let e0 = 1.0 + k / q / v0 + k * k;

        let alpha = 1.0 + v0 * k / q + k * k;
        let beta = 2.0 * k * k - 2.0;
        let gamma = 1.0 - v0 * k / q + k * k;
        let delta = 1.0 - k / q + k * k;
        let eta = 1.0 - k / q / v0 + k * k;

        if self.gain >= 0.0 {
            BiQuad {
                a0: alpha / d0,
                a1: beta / d0,
                a2: gamma / d0,
                b1: beta / d0,
                b2: delta / d0,
                ..Default::default()
            }
        } else {
            BiQuad {
                a0: d0 / e0,
                a1: beta / e0,
                a2: delta / e0,
                b1: beta / e0,
                b2: eta / e0,
                ..Default::default()
            }
        }
    }
}

#[derive(Debug, Default)]
pub struct BiQuad {
    a0: f32,
    a1: f32,
    a2: f32,
    b1: f32,
    b2: f32,
    xz1: f32,
    xz2: f32,
    yz1: f32,
    yz2: f32,
}

impl BiQuad {
    #[allow(dead_code)]
    fn new(freq: f32, sample_rate: f32, quality: f32, low: bool) -> Self {
        let sign = if low { 1.0 } else { -1.0 };
        let theta = (2.0 * PI * freq) / sample_rate;
        let d = 1.0 / quality / 2.0;
        let beta = 0.5 * (1.0 - d * theta.sin()) / (1.0 + d * theta.sin());
        let y = (0.5 + beta) * theta.cos() * sign;
        let a0 = (0.5 + beta - y) / 2.0;
        let a1 = (0.5 + beta - y) * sign;
        let a2 = (0.5 + beta - y) / 2.0;
        let b1 = -2.0 * y * sign;
        let b2 = 2.0 * beta;

        BiQuad {
            a0,
            a1,
            a2,
            b1,
            b2,
            ..Default::default()
        }
    }
}
impl Fx for BiQuad {
    fn process_one(&mut self, x: f32) -> f32 {
        let y = self.a0 * x + self.a1 * self.xz1 + self.a2 * self.xz2
            - self.b1 * self.yz1
            - self.b2 * self.yz2;
        self.xz2 = self.xz1;
        self.xz1 = x;
        self.yz2 = self.yz1;
        self.yz1 = y;
        y
    }
}

#[derive(Debug)]
pub struct EQ {
    filters: Vec<BiQuad>,
    filter_builders: Vec<PeakingConstantQBuilder>,
}

impl EQ {
    pub fn new(n: usize, sample_rate: f32) -> Self {
        let mut filter_builders = vec![];

        let n = n as isize;
        let q = 2f32.powf(10.0 / n as f32).sqrt() / (2f32.powf(10.0 / n as f32) - 1.0);
        for i in 0..n {
            filter_builders.push(PeakingConstantQBuilder::new(
                1000.0 * 2f32.powf((i - n / 2) as f32 / n as f32 * 10.0),
                sample_rate,
                q,
                0.0,
            ));
        }

        let filters = filter_builders.iter().map(|x| x.build()).collect();

        EQ {
            filter_builders,
            filters,
        }
    }

    // fn band_n(&self) -> usize {
    //     self.filters.len()
    // }

    pub fn set_gain(&mut self, band: usize, gain: f32) {
        self.filter_builders[band - 1].gain = gain;
        self.filters[band - 1] = self.filter_builders[band - 1].build();
    }
}

impl Fx for EQ {
    fn process_one(&mut self, x: f32) -> f32 {
        let mut y = x;
        for filter in &mut self.filters {
            y = filter.process_one(y);
        }
        y
    }
}

#[derive(Debug)]
pub struct Delay {
    delay: usize,
    xddl: VecDeque<f32>,
    yddl: VecDeque<f32>,
    mix: f32,
    fb: f32,
}

impl Delay {
    pub fn new(delay: usize, mix: f32, feedback: f32) -> Self {
        Delay {
            delay,
            xddl: VecDeque::with_capacity(delay * 2),
            yddl: VecDeque::with_capacity(delay * 2),
            mix,
            fb: feedback,
        }
    }

    fn set_delay(&mut self, delay: usize) {
        self.delay = delay;
        if self.xddl.capacity() < self.delay * 2 {
            self.xddl.reserve_exact(self.delay * 2);
        }
        if self.yddl.capacity() < self.delay * 2 {
            self.yddl.reserve_exact(self.delay * 2);
        }
    }
}

impl Fx for Delay {
    fn process_one(&mut self, x: f32) -> f32 {
        let y = x * (1.0 - self.mix)
            + self.mix
                * (self.xddl.get(self.delay.saturating_sub(1)).unwrap_or(&0.0)
                    + self.fb * self.yddl.get(self.delay.saturating_sub(1)).unwrap_or(&0f32));
        if self.xddl.len() == self.xddl.capacity() {
            self.xddl.pop_back();
        }
        if self.yddl.len() == self.yddl.capacity() {
            self.yddl.pop_back();
        }
        self.xddl.push_front(x);
        self.yddl.push_front(y);
        y
    }
}

#[derive(Debug)]
pub struct SineModulatedDelay {
    delay: usize,
    delay_depth: f32,
    period: f32,
    phase: f32,
    inner_delay: Delay,
}

impl SineModulatedDelay {
    pub fn new(delay: usize, delay_depth: f32, mix: f32, feedback: f32, period: f32) -> Self {
        SineModulatedDelay {
            delay,
            delay_depth,
            period,
            phase: 0.0,
            inner_delay: Delay::new(delay, mix, feedback),
        }
    }
}

impl Fx for SineModulatedDelay {
    fn process_one(&mut self, x: f32) -> f32 {
        self.inner_delay.set_delay(
            (self.delay as f32
                - self.delay as f32
                    * self.delay_depth
                    * (self.phase / self.period * 2.0 * PI).sin()) as usize,
        );
        let y = self.inner_delay.process_one(x);
        self.phase = (self.phase + 1.0) % self.period;
        y
    }
}

#[derive(Debug)]
pub struct ArctanWaveShape {
    k: f32,
}

impl ArctanWaveShape {
    pub fn new(k: f32) -> Self {
        ArctanWaveShape { k }
    }
}

impl Fx for ArctanWaveShape {
    fn process_one(&mut self, x: f32) -> f32 {
        self.k.atan().recip() * (x * self.k).atan()
    }
}

#[derive(Debug)]
pub struct EnvelopeDetector {
    attack: f32,
    release: f32,
    y: f32,
}

impl EnvelopeDetector {
    pub fn new(attack: f32, release: f32) -> Self {
        EnvelopeDetector {
            attack: 2.71f32.powf(-2.0 / attack),
            release: 2.71f32.powf(-2.0 / release),
            y: 0.0,
        }
    }
}

impl Fx for EnvelopeDetector {
    fn process_one(&mut self, x: f32) -> f32 {
        let x = x.abs();
        if self.y < x {
            self.y = self.attack * (self.y - x) + x
        } else {
            self.y = self.release * (self.y - x) + x
        }
        self.y
    }
}

#[derive(Debug)]
pub struct Gate {
    threshold: f32,
    detector: EnvelopeDetector,
}

impl Gate {
    pub fn new(attack: f32, release: f32, threshold: f32) -> Self {
        Gate {
            threshold,
            detector: EnvelopeDetector::new(attack, release),
        }
    }
}

impl Fx for Gate {
    fn process_one(&mut self, x: f32) -> f32 {
        let y = self.detector.process_one(x);
        if y.log10() * 20.0 >= self.threshold {
            x
        } else {
            0.0
        }
    }
}

#[derive(Debug)]
pub struct DCOffset {
    offset: f32,
}

impl DCOffset {
    pub fn new(offset: f32) -> Self {
        DCOffset { offset }
    }
}

impl Fx for DCOffset {
    fn process_one(&mut self, x: f32) -> f32 {
        x + self.offset
    }
}

#[derive(Debug)]
pub struct Dynamics {
    threshold: f32,
    ratio: f32,
    knee: f32,
    gain: f32,
    expander: bool,
    detector: EnvelopeDetector,
    delay: Delay,
}

impl Dynamics {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        attack: f32,
        release: f32,
        threshold: f32,
        ratio: f32,
        knee: f32,
        gain: f32,
        delay: usize,
        expander: bool,
    ) -> Self {
        Dynamics {
            threshold,
            ratio,
            knee,
            gain,
            expander,
            delay: Delay::new(delay, 1.0, 0.0),
            detector: EnvelopeDetector::new(attack, release),
        }
    }
}

// TODO: add delay
impl Fx for Dynamics {
    fn process_one(&mut self, x: f32) -> f32 {
        let x = x * self.gain;

        let det = (self.detector.process_one(x)).log10() * 20.0;
        let d = det - self.threshold;

        #[allow(clippy::collapsible_else_if)]
        let y = if self.expander {
            if d < self.knee / -2.0 {
                self.threshold + d * self.ratio
            } else if d.abs() <= self.knee / 2.0 {
                det + (1.0 - self.ratio) * (d - self.knee / 2.0).powi(2) / (2.0 * self.knee)
            } else {
                det
            }
        } else {
            if d < self.knee / -2.0 {
                det
            } else if d.abs() <= self.knee / 2.0 {
                det + (self.ratio.recip() - 1.0) * (d + self.knee / 2.0).powi(2) / (2.0 * self.knee)
            } else {
                self.threshold + d / self.ratio
            }
        };

        let yg = y - det;
        let y = 10f32.powf(yg / 20.0) * self.delay.process_one(x);
        if y.is_nan() { 0.0 } else { y }
    }
}

#[derive(Debug, Default)]
pub struct TrapezoidalSVH {
    v0z: f32,
    v1: f32,
    v2: f32,
    g1: f32,
    g2: f32,
    g3: f32,
    g4: f32,
}

impl TrapezoidalSVH {
    pub fn new(freq: f32, q: f32) -> Self {
        let mut svh = TrapezoidalSVH {
            ..Default::default()
        };
        svh.set(freq, q);
        svh
    }
    pub fn set(&mut self, freq: f32, q: f32) {
        let g = (PI * freq).tan();
        let k = 1.0 / q;
        let ginv = g / (1.0 + g * (g + k));
        self.g1 = ginv;
        self.g2 = 2.0 * (g + k) * ginv;
        self.g3 = g * ginv;
        self.g4 = 2.0 * ginv;
    }
}

impl Fx for TrapezoidalSVH {
    fn process_one(&mut self, x: f32) -> f32 {
        let v0 = x;
        let v1z = self.v1;
        let v2z = self.v2;

        let v3 = v0 + self.v0z - 2.0 * v2z;
        self.v1 += self.g1 * v3 - self.g2 * v1z;
        self.v2 += self.g3 * v3 + self.g4 * v1z;

        self.v0z = v0;

        self.v1
    }
}

#[derive(Debug)]
pub struct SineModulatedWah {
    mod_freq: f32,
    base_freq: f32,
    depth: f32,
    state: f32,
    q: f32,
    band_pass: TrapezoidalSVH,
}

impl SineModulatedWah {
    pub fn new(mod_freq: f32, base_freq: f32, depth: f32, q: f32) -> Self {
        SineModulatedWah {
            mod_freq,
            base_freq,
            depth,
            state: 0.0,
            q,
            band_pass: TrapezoidalSVH::new(base_freq, q),
        }
    }
}

impl Fx for SineModulatedWah {
    fn process_one(&mut self, x: f32) -> f32 {
        self.band_pass.set(
            self.base_freq + self.base_freq * (self.state.sin() * self.depth),
            self.q,
        );
        self.state = (self.state + self.mod_freq * 2.0 * PI) % (2.0 * PI);

        self.band_pass.process_one(x)
    }
}

#[derive(Debug)]
pub struct EnvelopeWah {
    min_freq: f32,
    max_freq: f32,
    q: f32,
    band_pass: TrapezoidalSVH,
    envelope_detector: EnvelopeDetector,
    gain: f32,
}

impl EnvelopeWah {
    pub fn new(min_freq: f32, max_freq: f32, q: f32, attack: f32, release: f32, gain: f32) -> Self {
        EnvelopeWah {
            min_freq,
            max_freq,
            q,
            gain,
            band_pass: TrapezoidalSVH::new(min_freq, q),
            envelope_detector: EnvelopeDetector::new(attack, release),
        }
    }
}

impl Fx for EnvelopeWah {
    fn process_one(&mut self, x: f32) -> f32 {
        let db = ((self.envelope_detector.process_one(x * self.gain)).log10() * 20.00).max(-20.0);

        let freq = (self.max_freq - self.min_freq) * (1.0 - db / -20.0) + self.min_freq;

        self.band_pass.set(freq, self.q);

        self.band_pass.process_one(x)
    }
}
