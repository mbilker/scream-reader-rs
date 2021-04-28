use std::cmp;
use std::mem;
use std::net::{Ipv4Addr, SocketAddrV4, UdpSocket};
use std::slice;

use anyhow::{anyhow, Context, Result};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{
    BuildStreamError, Device, HostId, OutputCallbackInfo, Sample, SampleFormat, SampleRate, Stream,
    StreamConfig,
};
use ringbuf::{Consumer, RingBuffer};

fn main() -> Result<()> {
    let wasapi_host_id = cpal::available_hosts()
        .into_iter()
        .find(|id| *id == HostId::Wasapi)
        .context("WASAPI host API not found")?;
    let host = cpal::host_from_id(wasapi_host_id)?;
    println!("Host: {:#?}", host.id());

    let mut device = host
        .default_output_device()
        .context("No output device available")?;

    let output_config = device.default_output_config()?;
    println!("Output Config: {:#?}", output_config);

    let mut config;
    let mut stream = None;

    let rb: RingBuffer<f32> = RingBuffer::new(16 * 1024);
    let (mut producer, consumer) = rb.split();
    let mut consumer = Some(consumer);

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

        if stream.is_none() {
            println!(
                "sample_rate: {}, sample_width: {}, channels: {}, channel_map: {}",
                sample_rate, sample_width, channels, channel_map
            );

            /*
            let sample_format = match sample_width {
                16 => SampleFormat::I16,
                32 => SampleFormat::F32,
                sample_width => {
                    return Err(anyhow!("Unknown sample format: {}", sample_width));
                }
            };
            */

            config = StreamConfig {
                //channels: cmp::min(channels as u16, 2),
                channels: channels as u16,
                sample_rate: SampleRate(sample_rate),
                buffer_size: output_config.config().buffer_size,
            };

            if let Some(consumer) = consumer.take() {
                /*
                let new_stream = match sample_format {
                    SampleFormat::U16 => build_stream::<u16>(&mut device, &config, consumer)?,
                    SampleFormat::I16 => build_stream::<i16>(&mut device, &config, consumer)?,
                    SampleFormat::F32 => build_stream::<f32>(&mut device, &config, consumer)?,
                };
                */
                let new_stream = build_stream::<f32>(&mut device, &config, consumer)?;
                new_stream.play()?;
                println!("created stream");

                stream = Some(new_stream);
            }
        }

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
    }
}

fn build_stream<T: Sample>(
    device: &mut Device,
    config: &StreamConfig,
    mut consumer: Consumer<f32>,
) -> Result<Stream, BuildStreamError> {
    let channels = config.channels as usize;

    device.build_output_stream(
        &config,
        move |data: &mut [T], _: &OutputCallbackInfo| {
            for frame in data.chunks_mut(channels) {
                for sample in frame.iter_mut() {
                    *sample = Sample::from::<f32>(&consumer.pop().unwrap_or_default());
                }
            }
        },
        |err| {
            eprintln!("{:?}", err);
        },
    )
}
