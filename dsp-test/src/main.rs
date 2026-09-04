mod fx;

use std::{
    fs::File,
    io::Read,
    sync::{Arc, Mutex},
};

use jack::{AudioIn, AudioOut, Client, ClientOptions, Control, ProcessHandler, ProcessScope};
use rouille::Response;

use crate::fx::{ArctanWaveShape, ClippingGain, Delay, EQ, Gate, SineModulatedDelay};
use crate::fx::{DCOffset, Fx};

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
            "DCOffset" => Box::new(DCOffset::new(fx_arg[0])),
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
