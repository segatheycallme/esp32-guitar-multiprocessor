use core::panic;
use std::{
    collections::VecDeque,
    f32::consts::PI,
    fs::File,
    io::{self, Read},
    path::Path,
    sync::{Arc, Mutex},
};

use jack::{AudioIn, AudioOut, Client, ClientOptions, Control, ProcessHandler, ProcessScope};
use rouille::Response;

trait Fx: Send + std::fmt::Debug {
    fn process_one(&mut self, x: f32) -> f32;
}

#[derive(Debug)]
struct ClippingGain {
    gain: f32,
}

impl ClippingGain {
    fn new(gain: f32) -> Self {
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
    fn new(frequency: f32, sample_rate: f32, quality: f32, gain: f32) -> Self {
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
struct EQ {
    filters: Vec<BiQuad>,
    filter_builders: Vec<PeakingConstantQBuilder>,
}

impl EQ {
    fn new(n: usize, sample_rate: f32) -> Self {
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

    fn set_gain(&mut self, band: usize, gain: f32) {
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
struct Delay {
    delay: usize,
    xddl: VecDeque<f32>,
    yddl: VecDeque<f32>,
    mix: f32,
    fb: f32,
}

impl Delay {
    fn new(delay: usize, mix: f32, feedback: f32) -> Self {
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
struct SineModulatedDelay {
    delay: usize,
    delay_depth: f32,
    period: f32,
    phase: f32,
    inner_delay: Delay,
}

impl SineModulatedDelay {
    fn new(delay: usize, delay_depth: f32, mix: f32, feedback: f32, period: f32) -> Self {
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
struct ArctanWaveShape {
    k: f32,
}

impl ArctanWaveShape {
    fn new(k: f32) -> Self {
        ArctanWaveShape { k }
    }
}

impl Fx for ArctanWaveShape {
    fn process_one(&mut self, x: f32) -> f32 {
        self.k.atan().recip() * (x * self.k).atan()
    }
}

#[derive(Debug)]
struct EnvelopeDetector {
    attack: f32,
    release: f32,
    y: f32,
}

impl EnvelopeDetector {
    fn new(attack: f32, release: f32) -> Self {
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
struct Gate {
    threshold: f32,
    detector: EnvelopeDetector,
}

impl Gate {
    fn new(attack: f32, release: f32, threshold: f32) -> Self {
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

#[derive(Debug, Default)]
struct Dsp {
    fx: Vec<Box<dyn Fx>>,
}

impl Dsp {
    // fn process_vec(&mut self, mut s: Vec<f32>) -> Vec<f32> {
    //     for x in &mut s {
    //         for fx in &mut self.fx {
    //             *x = fx.process_one(*x);
    //         }
    //     }
    //     s
    // }
    fn process_slice(&mut self, input: &[f32], output: &mut [f32]) {
        for i in 0..(input.len()) {
            output[i] = input[i];
            for fx in &mut self.fx {
                output[i] = fx.process_one(output[i]);
            }
        }
    }
}

// fn wav() {
//     let mut eq = EQ::new(10, 44100.0);
//     eq.set_gain(5, 0.0);
//
//     // let delay = SineModulatedDelay::new((7.0 * 44.1) as usize, 0.45, 1.0, -0.999, 44100.0 / 1.8);
//     // let delay = Delay::new((1.0 * 44.1) as usize, 0.5, 0.0);
//     // delay.set_delay(8820);
//     // delay.set_delay(4410);
//     let gate = Gate::new(0.2 * 44.1, 10.0 * 44.1, 0.02);
//
//     let mut dsp = Dsp {
//         fx: vec![
//             // Box::new(ClippingGain::new(50.0)),
//             // Box::new(dbg!(eq)),
//             // Box::new(dbg!(ArctanWaveShape::new(2.0))),
//             // Box::new(dbg!(ArctanWaveShape::new(2.0))),
//             // Box::new(dbg!(ArctanWaveShape::new(2.0))),
//             // Box::new(dbg!(ArctanWaveShape::new(2.0))),
//             // Box::new(dbg!(delay)),
//             Box::new(dbg!(gate)),
//             Box::new(ClippingGain::new(32767.0)),
//         ],
//     };
//
//     let mut reader = hound::WavReader::open("in.wav").unwrap();
//     let left: Vec<_> = reader
//         .samples::<i16>()
//         .map(|x| x.unwrap())
//         .map(|x| (x as f32) / (i16::MAX as f32))
//         .collect();
//
//     let spec = hound::WavSpec {
//         channels: 1,
//         sample_rate: 44100,
//         bits_per_sample: 16,
//         sample_format: hound::SampleFormat::Int,
//     };
//     let mut writer = hound::WavWriter::create("out.wav", spec).unwrap();
//     for sample in dsp.process_vec(left) {
//         writer
//             .write_sample((sample * (i16::MAX as f32)) as i16)
//             .unwrap();
//     }
//     writer.finalize().unwrap();
// }

struct Handler {
    input: jack::Port<AudioIn>,
    output: jack::Port<AudioOut>,
    dsp: Arc<Mutex<Dsp>>,
}

impl ProcessHandler for Handler {
    fn process(&mut self, _client: &Client, ps: &ProcessScope) -> Control {
        let input = self.input.as_slice(ps);
        let output = self.output.as_mut_slice(ps);

        let mut dsp = self.dsp.lock().unwrap();
        dsp.process_slice(input, output);

        Control::Continue
    }
}

fn parse_dsp(buf: String, sample_rate: f32) -> Dsp {
    let obj: serde_json::Value = serde_json::from_str(&buf).unwrap();

    let mut fx: Vec<Box<dyn Fx>> = vec![];
    for el in obj.as_array().unwrap() {
        let fx_obj = el.as_object().unwrap();
        let fx_type = fx_obj.get("type").unwrap().as_str().unwrap();
        let fx_arg: Vec<f32> = fx_obj
            .get("arguments")
            .unwrap()
            .as_array()
            .unwrap()
            .clone()
            .into_iter()
            .map(|x| x.as_number().unwrap().as_f64().unwrap() as f32)
            .collect();
        fx.push(match fx_type {
            "ClippingGain" => Box::new(ClippingGain::new(fx_arg[0])),
            "ArctanWaveShape" => Box::new(ArctanWaveShape::new(fx_arg[0])),
            "Delay" => Box::new(Delay::new(
                (fx_arg[0] * sample_rate / 1000.0) as usize,
                fx_arg[1],
                fx_arg[2],
            )),
            "SineModulatedDelay" => Box::new(SineModulatedDelay::new(
                (fx_arg[0] * sample_rate / 1000.0) as usize,
                fx_arg[1],
                fx_arg[2],
                fx_arg[3],
                sample_rate / fx_arg[4],
            )),
            "Gate" => Box::new(Gate::new(
                fx_arg[0] * sample_rate / 1000.0,
                fx_arg[1] * sample_rate / 1000.0,
                fx_arg[2],
            )),
            "EQ" => {
                let mut eq = EQ::new(fx_arg[0] as usize, sample_rate);
                for (band, gain) in fx_arg.iter().enumerate() {
                    if band == 0 {
                        continue;
                    }
                    eq.set_gain(band, *gain);
                }
                Box::new(eq)
            }
            _ => panic!("unknown_type"),
        });
    }

    dbg!(Dsp { fx })
}

fn main() {
    let (client, _status) = Client::new("rust-loopback", ClientOptions::NO_START_SERVER).unwrap();

    let sample_rate = client.sample_rate() as f32;

    let mut buf = String::new();
    let myb_file = File::open("plugin.json");
    let dsp = if let Ok(mut file) = myb_file {
        file.read_to_string(&mut buf).unwrap();
        parse_dsp(buf, sample_rate)
    } else {
        Default::default()
    };

    let dsp = Arc::new(Mutex::new(dsp));

    let input = client.register_port("input", AudioIn::default()).unwrap();

    let output = client.register_port("output", AudioOut::default()).unwrap();

    let handler = Handler {
        input,
        output,
        dsp: dsp.clone(),
    };

    let _active_client = Some(client.activate_async((), handler).unwrap());

    println!("entering main loop");
    rouille::start_server("0.0.0.0:3000", move |req| {
        let mut buf = String::new();
        req.data().unwrap().read_to_string(&mut buf).unwrap();
        dsp.lock().unwrap().fx = parse_dsp(buf, sample_rate).fx;
        dbg!(&dsp);
        Response::text("success")
    });
}
