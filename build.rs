//! SPDX-License-Identifier: MIT OR Apache-2.0
//!
//! Copyright (c) 2021–2024 The rp-rs Developers
//! Copyright (c) 2021 rp-rs organization
//! Copyright (c) 2025 Raspberry Pi Ltd.
//!
//! Set up linker scripts

use std::fs::{ File, read_to_string };
use std::io::Write;
use std::path::PathBuf;

use regex::Regex;

fn main() {
    println!("cargo::rustc-check-cfg=cfg(rp2040)");
    println!("cargo::rustc-check-cfg=cfg(rp2350)");

    // Put the linker script somewhere the linker can find it
    let out = PathBuf::from(std::env::var_os("OUT_DIR").unwrap());
    println!("cargo:rustc-link-search={}", out.display());

    println!("cargo:rerun-if-changed=.pico-rs");
    let contents = read_to_string(".pico-rs")
        .map(|s| s.trim().to_string().to_lowercase())
        .unwrap_or_else(|e| {
            eprintln!("Failed to read file: {}", e);
            String::new()
        });

    // The file `memory.x` is loaded by cortex-m-rt's `link.x` script, which
    // is what we specify in `.cargo/config.toml` for Arm builds
    let target;
    if contents == "rp2040" {
        target = "thumbv6m-none-eabi";
        let memory_x = include_bytes!("rp2040.x");
        let mut f = File::create(out.join("memory.x")).unwrap();
        f.write_all(memory_x).unwrap();
        println!("cargo::rustc-cfg=rp2040");
        println!("cargo:rerun-if-changed=rp2040.x");
    } else {
        if contents.contains("riscv") {
            target = "riscv32imac-unknown-none-elf";
        } else {
            target = "thumbv8m.main-none-eabihf";
        }
        let memory_x = include_bytes!("rp2350.x");
        let mut f = File::create(out.join("memory.x")).unwrap();
        f.write_all(memory_x).unwrap();
        println!("cargo::rustc-cfg=rp2350");
        println!("cargo:rerun-if-changed=rp2350.x");
    }

    let chip = if contents == "rp2040" { "RP2040" } else { "RP2350" };

    // Read at runtime so manual edits to config.toml survive across builds.
    // include_str! bakes the content at build.rs compile time and silently
    // overwrites manual changes on every cargo build.
    println!("cargo:rerun-if-changed=.cargo/config.toml");
    let config_toml = read_to_string(".cargo/config.toml")
        .expect("failed to read .cargo/config.toml");

    let re_target = Regex::new(r"target = .*").unwrap();
    let result = re_target.replace(&config_toml, format!("target = \"{}\"", target));

    // Replace the chip in every probe-rs runner line.
    // Works on first run (placeholder ${CHIP}) and subsequent runs (real name).
    let re_chip = Regex::new(r"probe-rs run --chip \S+").unwrap();
    let result = re_chip.replace_all(&result, format!("probe-rs run --chip {}", chip));

    let mut f = File::create(".cargo/config.toml").unwrap();
    f.write_all(result.as_bytes()).unwrap();

    // The file `rp2350_riscv.x` is what we specify in `.cargo/config.toml` for
    // RISC-V builds
    let rp2350_riscv_x = include_bytes!("rp2350_riscv.x");
    let mut f = File::create(out.join("rp2350_riscv.x")).unwrap();
    f.write_all(rp2350_riscv_x).unwrap();
    println!("cargo:rerun-if-changed=rp2350_riscv.x");

    println!("cargo:rerun-if-changed=build.rs");
}
