#![no_std]
#![no_main]
#![deny(
    clippy::mem_forget,
    reason = "mem::forget is generally not safe to do with esp_hal types, especially those \
    holding buffers for the duration of a data transfer."
)]
#![deny(clippy::large_stack_frames)]

use es8311::{ClockConfig, Es8311};
use esp_hal::clock::CpuClock;
use esp_hal::delay::Delay;
use esp_hal::i2c::master::I2c;
use esp_hal::i2s::master::{Channels, I2s};
use esp_hal::peripherals::GPIO9;
use esp_hal::time::Rate;
use esp_hal::{dma_buffers, i2c, i2s, main};
use log::{error, info};

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

    // 13 sda
    // 12 scl
    // 11 mck
    // 10 bck
    // 9 di
    // 3 ws
    // 8 do

    let config = i2c::master::Config::default().with_frequency(Rate::from_khz(100));
    let mut i2c = I2c::new(peripherals.I2C0, config)
        .unwrap()
        .with_sda(peripherals.GPIO13)
        .with_scl(peripherals.GPIO12);

    let codec = Es8311::new(0x18);

    let clock_cfg = ClockConfig {
        mclk_inverted: false,
        sclk_inverted: false,
        mclk_from_mclk_pin: true,
        mclk_frequency: 12_288_000,
        sample_frequency: 48_000,
    };
    codec
        .init(
            &mut i2c,
            &clock_cfg,
            es8311::Resolution::Bits16,
            es8311::Resolution::Bits16,
            &mut Delay::new(),
        )
        .unwrap();

    codec.volume_set(&mut i2c, 100, None).unwrap();

    let (mut rx_buffer, rx_descriptors, _, _) = dma_buffers!(4 * 64, 0);
    let (mut tx_buffer, tx_descriptors, _, _) = dma_buffers!(4 * 64, 0);

    let i2s = I2s::new(
        peripherals.I2S0,
        peripherals.DMA_CH0,
        i2s::master::Config::new_tdm_philips()
            .with_sample_rate(Rate::from_hz(48_000))
            .with_data_format(i2s::master::DataFormat::Data16Channel16)
            .with_channels(Channels::MONO),
    )
    .unwrap()
    .with_mclk(peripherals.GPIO11);

    let mut i2s_rx = i2s
        .i2s_rx
        .with_bclk(peripherals.GPIO10)
        .with_ws(peripherals.GPIO3)
        .with_din(peripherals.GPIO8)
        .build(rx_descriptors);
    let mut i2s_tx = i2s
        .i2s_tx
        .with_dout(peripherals.GPIO9)
        .build(tx_descriptors);

    esp_alloc::heap_allocator!(#[esp_hal::ram(reclaimed)] size: 73744);

    let delay = Delay::new();
    loop {}

    // for inspiration have a look at the examples at https://github.com/esp-rs/esp-hal/tree/esp-hal-v1.1.0/examples
}
