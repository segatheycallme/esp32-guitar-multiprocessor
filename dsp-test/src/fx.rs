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
struct PeakingConstantQBuilder {
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

    fn build(&self) -> BiQuad {
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
struct BiQuad {
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
                * (self.xddl.get(self.delay - 1).unwrap_or(&0.0)
                    + self.fb * self.yddl.get(self.delay - 1).unwrap_or(&0f32));
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
        if y >= self.threshold.abs() { x } else { 0.0 }
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
