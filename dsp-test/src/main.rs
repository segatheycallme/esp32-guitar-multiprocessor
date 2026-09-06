mod fx;

use std::{
    env,
    fs::File,
    io::Read,
    ops::DerefMut,
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

fn wav(dsp: &mut Dsp) {
    let mut reader = hound::WavReader::open("in.wav").unwrap();
    let input: Vec<_> = reader
        .samples::<i16>()
        .map(|x| x.unwrap())
        .map(|x| (x as f32) / (i16::MAX as f32))
        .collect();
    let mut output = input.clone();

    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: 44100,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut writer = hound::WavWriter::create("out.wav", spec).unwrap();
    dsp.process_slice(&input, &mut output);
    for sample in output {
        writer
            .write_sample((sample * (i16::MAX as f32)) as i16)
            .unwrap();
    }
    writer.finalize().unwrap();
}

fn main() {
    let arg1 = env::args().nth(1);
    let mode = arg1.is_some();
    let wav_file = arg1.unwrap_or("plugin.json".to_string());

    let (client, _status) = Client::new("rust-loopback", ClientOptions::NO_START_SERVER).unwrap();

    let sample_rate = client.sample_rate() as f32;

    if mode {
        let mut buf = String::new();
        let myb_file = File::open(wav_file);
        let dsp = if let Ok(mut file) = myb_file {
            file.read_to_string(&mut buf).unwrap();
            parse_dsp(buf, sample_rate)
        } else {
            Default::default()
        };

        let dsp = Arc::new(Mutex::new(dsp));

        wav(dsp.lock().unwrap().deref_mut());
    } else {
        let dsp = Arc::new(Mutex::new(Dsp::default()));

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
}
