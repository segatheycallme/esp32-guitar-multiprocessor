#![no_std]
#![no_main]
#![deny(
    clippy::mem_forget,
    reason = "mem::forget is generally not safe to do with esp_hal types, especially those \
    holding buffers for the duration of a data transfer."
)]
#![deny(clippy::large_stack_frames)]

use esp_hal::clock::CpuClock;
use esp_hal::delay::Delay;
use esp_hal::gpio::Flex;
use esp_hal::i2c::master::I2c;
use esp_hal::{i2c, main, peripherals};
use log::error;

#[panic_handler]
fn panic(panic_info: &core::panic::PanicInfo) -> ! {
    error!("{}", panic_info);
    loop {}
}

extern crate alloc;

// This creates a default app-descriptor required by the esp-idf bootloader.
// For more information see: <https://docs.espressif.com/projects/esp-idf/en/stable/esp32/api-reference/system/app_image_format.html#application-description>
esp_bootloader_esp_idf::esp_app_desc!();

#[allow(
    clippy::large_stack_frames,
    reason = "it's not unusual to allocate larger buffers etc. in main"
)]
#[main]
fn main() -> ! {
    // generator version: 1.3.0
    // generator parameters: --chip esp32s3 -o esp32s3-wroom-1-psram -o unstable-hal -o alloc -o log -o neovim

    esp_println::logger::init_logger_from_env();

    let config = esp_hal::Config::default().with_cpu_clock(CpuClock::max());
    let peripherals = esp_hal::init(config);

    // The following pins are used to bootstrap the chip. They are available
    // for use, but check the datasheet of the module for more information on them.
    // - GPIO0
    // - GPIO3
    // - GPIO45
    // - GPIO46
    // These GPIO pins are in use by some feature of the module and should not be used.
    let _ = peripherals.GPIO26;
    let _ = peripherals.GPIO27;
    let _ = peripherals.GPIO28;
    let _ = peripherals.GPIO29;
    let _ = peripherals.GPIO30;
    let _ = peripherals.GPIO31;
    let _ = peripherals.GPIO32;

    // 4 sda
    // 5 scl
    // 6 bck
    // 12 ws
    // 13 di
    // 14 do

    let mut i2c = I2c::new(peripherals.I2C0, i2c::master::Config::default())
        .unwrap()
        .with_sda(peripherals.GPIO4)
        .with_scl(peripherals.GPIO5);

    let addr = 0x1a;
    i2c.write(addr, &[0x23, 0x80]).unwrap(); // master clock
    i2c.write(addr, &[0x22, 0x0b]).unwrap(); // 48khz
    i2c.write(addr, &[0x29, 0xe8]).unwrap(); // enable tdm dai stereo i2s

    i2c.write(addr, &[0x60, 0xc4]).unwrap(); // mute aux l
    i2c.write(addr, &[0x61, 0xc4]).unwrap(); // mute aux r
    i2c.write(addr, &[0x33, 0x01]).unwrap(); // auxl to input mixer l
    i2c.write(addr, &[0x34, 0x01]).unwrap(); // auxr to input mixer r
    i2c.write(addr, &[0x65, 0xa8]).unwrap(); // enable input mixer l
    i2c.write(addr, &[0x66, 0xa8]).unwrap(); // enable input mixer r
    i2c.write(addr, &[0x67, 0xf0]).unwrap(); // mute adc l
    i2c.write(addr, &[0x68, 0xf0]).unwrap(); // mute adc r

    i2c.write(addr, &[0x60, 0x84]).unwrap(); // unmute aux l
    i2c.write(addr, &[0x61, 0x84]).unwrap(); // unmute aux r
    i2c.write(addr, &[0x67, 0xa0]).unwrap(); // unmute adc l
    i2c.write(addr, &[0x68, 0xa0]).unwrap(); // unmute adc r

    esp_alloc::heap_allocator!(size: 64000);
    panic!()

    // for inspiration have a look at the examples at https://github.com/esp-rs/esp-hal/tree/esp-hal-v1.1.0/examples
}

fn segment_number(leds: &mut [Flex; 5], delay: Delay, delay_time: u32, n: u8, left: bool) {
    let offset = if left { 0 } else { 7 };
    match n {
        0 => {
            for i in [1, 2, 3, 5, 6, 7] {
                segment_control(leds, i + offset, true);
                delay.delay_micros(delay_time);
                segment_control(leds, i + offset, false);
            }
        }
        1 => {
            for i in [3, 6] {
                segment_control(leds, i + offset, true);
                delay.delay_micros(delay_time);
                segment_control(leds, i + offset, false);
            }
        }
        2 => {
            for i in [1, 3, 4, 5, 7] {
                segment_control(leds, i + offset, true);
                delay.delay_micros(delay_time);
                segment_control(leds, i + offset, false);
            }
        }
        3 => {
            for i in [1, 3, 4, 6, 7] {
                segment_control(leds, i + offset, true);
                delay.delay_micros(delay_time);
                segment_control(leds, i + offset, false);
            }
        }
        4 => {
            for i in [2, 3, 4, 6] {
                segment_control(leds, i + offset, true);
                delay.delay_micros(delay_time);
                segment_control(leds, i + offset, false);
            }
        }
        5 => {
            for i in [1, 2, 4, 6, 7] {
                segment_control(leds, i + offset, true);
                delay.delay_micros(delay_time);
                segment_control(leds, i + offset, false);
            }
        }
        6 => {
            for i in [1, 2, 4, 5, 6, 7] {
                segment_control(leds, i + offset, true);
                delay.delay_micros(delay_time);
                segment_control(leds, i + offset, false);
            }
        }
        7 => {
            for i in [1, 3, 6] {
                segment_control(leds, i + offset, true);
                delay.delay_micros(delay_time);
                segment_control(leds, i + offset, false);
            }
        }
        8 => {
            for i in [1, 2, 3, 4, 5, 6, 7] {
                segment_control(leds, i + offset, true);
                delay.delay_micros(delay_time);
                segment_control(leds, i + offset, false);
            }
        }
        9 => {
            for i in [1, 2, 3, 4, 6, 7] {
                segment_control(leds, i + offset, true);
                delay.delay_micros(delay_time);
                segment_control(leds, i + offset, false);
            }
        }
        _ => {}
    }
}

fn segment_control(leds: &mut [Flex; 5], seg: u8, enable: bool) {
    let (on, off) = match seg {
        1 => (1, 2),
        2 => (0, 4),
        3 => (1, 4),
        4 => (2, 4),
        5 => (0, 2),
        6 => (4, 2),
        7 => (3, 1),
        8 => (3, 0),
        9 => (2, 0),
        10 => (0, 1),
        11 => (2, 1),
        12 => (4, 0),
        13 => (4, 1),
        14 => (1, 0),
        _ => (0, 0),
    };
    if enable {
        leds[on].set_high();
        leds[off].set_low();
        leds[on].set_output_enable(true);
        leds[off].set_output_enable(true);
    } else {
        leds[on].set_output_enable(false);
        leds[off].set_output_enable(false);
    }
}
