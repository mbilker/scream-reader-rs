# Scream Stream Reader

A simple [Scream](https://github.com/duncanthrax/scream) stream reader in Rust using cpal for
cross-platform audio handling.

## Featues

- Cross-platform audio handling using the [cpal](https://github.com/RustAudio/cpal) crate
- Sample rate switching (dependent on host API support, no support for toggling WASAPI auto
  conversion at the time of writing)

## Issues

- Playback stutters due to no thread priority boost