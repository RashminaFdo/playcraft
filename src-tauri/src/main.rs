// Unconditionally prevents console window on Windows (both debug and release), DO NOT REMOVE!!
#![windows_subsystem = "windows"]

//! PlayCraft Main Binary Entrypoint
//! Author: Rashmina Fernando (https://www.rashmina.dev)
//! Copyright (c) 2026 Rashmina Fernando (www.rashmina.dev). All Rights Reserved.

fn main() {
    playcraft_lib::run();
}
