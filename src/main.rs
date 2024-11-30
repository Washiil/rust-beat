use crossbeam::channel::{unbounded, Receiver, Sender};
use device_query::{DeviceQuery, DeviceState, Keycode};
use std::sync::{
    atomic::{AtomicBool, AtomicU64, Ordering},
    Arc, Mutex,
};
use xcap::Window;

use enigo::{
    Direction::{Click, Press, Release},
    Enigo, InputError, Key, Keyboard, Settings,
};
use std::{thread, time::Duration};

/// Uses the xcap library to get the roblox window by comparing the list of open window app_names
fn find_roblox_window() -> Result<Window, &'static str> {
    let windows = Window::all()
        .map_err(|_| "Could not access open windows, try running in administrator mode")?;

    let roblox_windows: Vec<&Window> = windows
        .iter()
        .filter(|&win| win.app_name() == "Roblox Game Client")
        .collect();

    roblox_windows
        .first()
        .copied()
        .ok_or("Roblox is not open or the window could not be found")
        .cloned()
}

// Producer: Captures screen data and sends it through the channel
fn producer_main_loop(
    consumers: Vec<Sender<[[u8; 3]; 4]>>,
    stop_signal: Arc<std::sync::atomic::AtomicBool>,
) {
    let offsets: [i32; 4] = [-150, -50, 50, 150];
    let mut pixels = [[0u8; 3]; 4];

    while !stop_signal.load(std::sync::atomic::Ordering::Relaxed) {
        if let Ok(window) = find_roblox_window() {
            if let Ok(buffer) = window.capture_image() {
                let buffer = buffer.to_vec();
                let height = window.height() as i32;
                let width = window.width() as i32;

                // Calculate base index once
                let base_index = ((height / 36) * width * 32) + (width / 2);
                
                // Use iterator for more efficient processing
                for (pixel, &offset) in pixels.iter_mut().zip(offsets.iter()) {
                    let idx = ((base_index + offset) * 4) as usize;
                    if idx + 2 < buffer.len() {
                        pixel.copy_from_slice(&buffer[idx..idx + 3]);
                    }
                }

                // Send to all consumers using a single allocation
                for sender in &consumers {
                    let _ = sender.send(pixels);  // Ignore errors for speed
                }
            }
        }
        // Idk if I want this
        thread::sleep(Duration::from_micros(100));
    }
}

// Consumer: Processes the latest screen data
fn consumer_main_loop(
    rx: Receiver<[[u8; 3]; 4]>,
    stop_signal: Arc<AtomicBool>,
    note_delay: Arc<AtomicU64>,
    index: usize,
    key: char,
    mut controller: Enigo,
) {
    let mut key_down = false;

    while !stop_signal.load(Ordering::Relaxed) {
        // Drain the channel and get the latest data
        if let Ok(screen_data) = rx.try_recv() {
            // Note Color: 254, 226, 19
            if screen_data[index][0] > 220 {
                if key_down {
                } else {
                    thread::sleep(Duration::from_millis(note_delay.load(Ordering::Relaxed)));
                    controller.key(Key::Unicode(key), Press);
                    key_down = true;
                    thread::sleep(Duration::from_millis(15));
                }
            } else {
                if key_down {
                    controller.key(Key::Unicode(key), Release);
                    key_down = false;
                }
            }
        }

        // Sleep briefly to avoid busy looping
        thread::sleep(Duration::from_millis(10));
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let tracks = ['d', 'f', 'j', 'k'];
    // Note delay implementation works but is a bit spotty on hold and ~30 and up causes consistent breaks
    let note_delay: Arc<AtomicU64> = Arc::new(AtomicU64::new(20));
    let device_state = DeviceState::new();

    println!("Starting color monitoring...");

    let stop_signal: Arc<AtomicBool> = Arc::new(AtomicBool::new(false));

    // Set up Ctrl+C handler
    let shutdown_signal = Arc::clone(&stop_signal);
    ctrlc::set_handler(move || {
        println!("Ctrl+C detected, shutting down...");
        shutdown_signal.store(true, Ordering::Relaxed);
    })
    .expect("Error setting Ctrl-C handler");

    // Data type is 4 rgb values (1 for each lane)
    let mut consumers: Vec<Sender<[[u8; 3]; 4]>> = Vec::new();

    let mut track_threads: Vec<thread::JoinHandle<()>> = Vec::new();
    for (i, track_id) in tracks.iter().enumerate() {
        let (tx, rx): (Sender<[[u8; 3]; 4]>, Receiver<[[u8; 3]; 4]>) = unbounded();
        consumers.push(tx);

        let enigo = Enigo::new(&Settings::default())?;

        let consumer_stop_signal = Arc::clone(&stop_signal);
        let consumer_note_delay = Arc::clone(&note_delay);

        let key = *track_id;

        // Spawn a consumer thread for each track
        let handle = thread::spawn(move || {
            consumer_main_loop(rx, consumer_stop_signal, consumer_note_delay, i, key, enigo)
        });
        track_threads.push(handle);
        println!("({}) Now tracking track {}", i, track_id);
    }

    // Spawn the producer thread
    let producer_stop_signal = Arc::clone(&stop_signal);
    let producer_handle =
        thread::spawn(move || producer_main_loop(consumers, producer_stop_signal));

    println!("All threads now running!\n");

    println!("Press:");
    println!("  UP ARROW : Increment counter");
    println!("  DOWN ARROW : Decrement counter");
    println!("  ESC : Exit\n");

    println!("Press Ctrl+C to stop\n");
    let mut previous_keys = vec![];

    while !stop_signal.load(Ordering::Relaxed) {
        // Get currently pressed keys
        let keys: Vec<Keycode> = device_state.get_keys();

        // Only process keys that were just pressed (not held)
        for key in keys.iter() {
            if !previous_keys.contains(key) {
                match key {
                    Keycode::Up => {
                        let new_value = note_delay.fetch_add(1, Ordering::Relaxed);
                        println!("Incremented to: {}", new_value + 1);
                    }
                    Keycode::Down => {
                        let new_value = note_delay.fetch_sub(1, Ordering::Relaxed);
                        println!("Decremented to: {}", new_value - 1);
                    }
                    Keycode::Escape => {
                        println!("Final value: {}", note_delay.load(Ordering::Relaxed));
                        break;
                    }
                    _ => {}
                }
            }
        }

        previous_keys = keys;
        thread::sleep(Duration::from_millis(10)); // Prevent high CPU usage
    }

    // Wait for all threads to finish
    for t in track_threads {
        t.join().expect("Error joining consumer thread");
    }

    producer_handle
        .join()
        .expect("Error joining producer thread");

    println!("All threads have shut down. Exiting.");
    Ok(())
}
