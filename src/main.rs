use std::mem;
use std::net::{Ipv4Addr, SocketAddrV4, UdpSocket};
use std::slice;

use anyhow::{Context, Result};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{
    BufferSize, BuildStreamError, Device, HostId, OutputCallbackInfo, Sample, SampleRate, Stream,
    StreamConfig,
};
use ringbuf::{Consumer, RingBuffer};

#[derive(Debug, Default, Eq, PartialEq)]
struct AudioSettings {
    sample_rate: u32,
    channels: u8,
    channel_map: u16,
}

fn main() -> Result<()> {
    let host_id = if cfg!(windows) {
        cpal::available_hosts()
            .into_iter()
            .find(|id| *id == HostId::Wasapi)
            .context("WASAPI host API not found")?
    } else {
        cpal::available_hosts()
            .into_iter()
            .nth(0)
            .context("First host API not found")?
    };
    let host = cpal::host_from_id(host_id)?;
    println!("Host: {:#?}", host.id());

    let mut device = host
        .default_output_device()
        .context("No output device available")?;

    let mut current_audio_settings = AudioSettings::default();
    let mut stream: Option<Stream> = None;
    let mut producer = None;

    let mut is_stream_playing = false;

    let listen_addr = SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, 4010);
    let multicast_addr = Ipv4Addr::new(239, 255, 77, 77);
    let socket = UdpSocket::bind(listen_addr)?;

    socket.set_read_timeout(None)?;
    socket.join_multicast_v4(&multicast_addr, &Ipv4Addr::BROADCAST)?;

    let mut data = [0u8; 16 * 1024];

    loop {
        let len = socket.recv(&mut data)?;
        let data = &data[..len];

        if data.len() <= 5 {
            continue;
        }

        let sample_rate = if data[0] >= 128 { 44100 } else { 48000 };
        let sample_width = data[1];
        let channels = data[2];
        let channel_map = (data[3] as u16) << 8 | data[4] as u16;

        let audio_settings = AudioSettings { sample_rate, channels, channel_map };

        if current_audio_settings != audio_settings {
            if let Some(stream) = stream.take() {
                let _ = stream.pause();
            }

            println!(
                "sample_rate: {}, sample_width: {}, channels: {}, channel_map: {}",
                sample_rate, sample_width, channels, channel_map
            );

            let config = StreamConfig {
                channels: channels as u16,
                sample_rate: SampleRate(sample_rate),
                buffer_size: BufferSize::Default,
            };
            let (new_producer, consumer) = RingBuffer::<f32>::new(16 * 1024).split();
            let new_stream = build_stream::<f32>(&mut device, &config, consumer)?;
            println!("created stream");

            current_audio_settings = audio_settings;
            stream = Some(new_stream);
            producer = Some(new_producer);

            is_stream_playing = false;
        }

        if let Some(producer) = producer.as_mut() {
            let data = &data[5..];

            match sample_width {
                16 => {
                    let data = unsafe {
                        slice::from_raw_parts(
                            data.as_ptr() as *const i16,
                            data.len() / mem::size_of::<i16>(),
                        )
                    };

                    /*
                    for frame in data.chunks(channels as usize) {
                        producer.push_iter(&mut frame.iter().take(2).map(Sample::from));
                    }
                    */

                    producer.push_iter(&mut data.iter().map(Sample::from));
                }
                32 => {
                    let data = unsafe {
                        slice::from_raw_parts(
                            data.as_ptr() as *const i32,
                            data.len() / mem::size_of::<i32>(),
                        )
                    };

                    let sample_map = |&sample| {
                        if sample < 0 {
                            sample as f32 / -(i32::MIN as f32)
                        } else {
                            sample as f32 / i32::MAX as f32
                        }
                    };

                    /*
                    for frame in data.chunks(channels as usize) {
                        producer.push_iter(&mut frame.iter().take(2).map(sample_map));
                    }
                    */

                    producer.push_iter(&mut data.iter().map(sample_map));
                }
                _ => {}
            }

            if !is_stream_playing && producer.len() >= data.len() * 4 {
                if let Some(stream) = &stream {
                    stream.play()?;
                    println!("started stream");

                    is_stream_playing = true;
                }
            }
        }
    }
}

fn build_stream<T: Sample>(
    device: &mut Device,
    config: &StreamConfig,
    mut consumer: Consumer<f32>,
) -> Result<Stream, BuildStreamError> {
    //let channels = config.channels as usize;

    let mut staging = Vec::new();

    device.build_output_stream(
        &config,
        move |data: &mut [T], _: &OutputCallbackInfo| {
            /*
            for frame in data.chunks_mut(channels) {
                for sample in frame.iter_mut() {
                    *sample = Sample::from::<f32>(&consumer.pop().unwrap_or_default());
                }
            }
            */

            staging.resize(data.len(), 0.0);
            consumer.pop_slice(&mut staging);

            for (sample, source) in data.iter_mut().zip(staging.iter()) {
                *sample = Sample::from::<f32>(source);
            }
        },
        |err| {
            eprintln!("{:?}", err);
        },
    )
}
