use crossbeam::channel::{unbounded, Receiver, Sender};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use xcap::Window;

use enigo::{
    Direction::{Click, Press, Release},
    Enigo, InputError, Key, Keyboard, Settings,
};
use rgb::RGB8;
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

#[derive(Clone)]
struct ScreenData1 {
    height: i32,
    width: i32,
    pixels: Vec<u8>, // Owned buffer of RGBA pixels
}

// Producer: Captures screen data and sends it through the channel
fn producer_main_loop(
    consumers: Vec<Sender<[[u8; 3]; 4]>>,
    stop_signal: Arc<std::sync::atomic::AtomicBool>,
) {
    let offsets: [i32; 4] = [-150, -50, 50, 150];

    while !stop_signal.load(std::sync::atomic::Ordering::Relaxed) {
        match find_roblox_window() {
            Ok(window) => {
                let height = window.height() as i32;
                let width = window.width() as i32;
                match window.capture_image() {
                    Ok(buffer) => {
                        let buffer = buffer.to_vec();

                        let index = (((height / 18) *width) * 16) + (width / 2);
                        let mut pixels = [[0, 0, 0]; 4];

                        for (i, off) in offsets.iter().enumerate() {
                            let temp_index = ((index + off) * 4) as usize;
                            pixels[i][0] = *buffer.get(temp_index).expect("Could not find colour.");
                            pixels[i][1] = *buffer.get(temp_index + 1).expect("Could not find colour.");
                            pixels[i][2] = *buffer.get(temp_index + 2).expect("Could not find colour.");
                        }

                        for sender in &consumers {
                            if let Err(err) = sender.send(pixels) {
                                eprintln!("Failed to send screen data: {}", err);
                                break;
                            }
                        }
                    }
                    Err(err) => {
                        eprintln!("Failed to capture image: {}", err);
                    }
                }
            }
            Err(err) => {
                eprintln!("Failed to find Roblox window: {}", err);
            }
        }
    }
}

// Consumer: Processes the latest screen data
fn consumer_main_loop(
    rx: Receiver<[[u8; 3]; 4]>,
    stop_signal: Arc<AtomicBool>,
    index: usize,
    key: char,
    mut controller: Enigo,
) {
    let mut key_down = false;

    while !stop_signal.load(Ordering::Relaxed) {
        // Drain the channel and get the latest data
        if let Some(screen_data) = rx.try_iter().last() {
            // Note Color: 254, 226, 19
            if screen_data[index][0] > 200 {
                if key_down {
                } else {
                    thread::sleep(Duration::from_millis(25));
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

    println!("Starting color monitoring...");
    println!("Press Ctrl+C to stop");

    // Stop signal to gracefully terminate the threads
    let stop_signal: Arc<AtomicBool> = Arc::new(AtomicBool::new(false));

    // Create channels for each consumer
    let mut consumers: Vec<Sender<[[u8; 3]; 4]>> = Vec::new();
    let mut track_threads: Vec<thread::JoinHandle<()>> = Vec::new();

    for (i, track_id) in tracks.iter().enumerate() {
        let (tx, rx): (Sender<[[u8; 3]; 4]>, Receiver<[[u8; 3]; 4]>) = unbounded();
        consumers.push(tx);

        let enigo = Enigo::new(&Settings::default())?;

        let consumer_stop_signal = Arc::clone(&stop_signal);
        let key = *track_id;

        // Spawn a consumer thread for each track
        let handle = thread::spawn(move || {
            consumer_main_loop(rx, consumer_stop_signal, i, key, enigo)
        });
        track_threads.push(handle);
    }

    // Spawn the producer thread
    let producer_stop_signal = Arc::clone(&stop_signal);
    let producer_handle =
        thread::spawn(move || producer_main_loop(consumers, producer_stop_signal));

    // Set up Ctrl+C handler
    let shutdown_signal = Arc::clone(&stop_signal);
    ctrlc::set_handler(move || {
        println!("Ctrl+C detected, shutting down...");
        shutdown_signal.store(true, Ordering::Relaxed);
    })
    .expect("Error setting Ctrl-C handler");

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
