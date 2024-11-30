use crossbeam::channel::{unbounded, Receiver, Sender};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use xcap::Window;

use enigo::{
    Direction::{Click, Press, Release},
    Enigo, InputError, Key, Keyboard, Mouse, Settings,
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
struct ScreenData {
    height: i32,
    width: i32,
    pixels: Vec<u8>, // Owned buffer of RGBA pixels
}

// The bottom of the score line on the right is 66% of the way down the screen so essentially cut in thirds
fn get_pixels() -> Result<Vec<RGB8>, Box<dyn std::error::Error>> {
    // Get the primary screen
    let roblox_win = find_roblox_window()?;

    let height = roblox_win.height();
    let width = roblox_win.width();

    let mut tracks = Vec::with_capacity(4);

    let image = roblox_win.capture_image()?;
    let buffer = image.to_vec();
    let rgba: Vec<&[u8]> = buffer.chunks_exact(4).collect();

    let two_thirds = ((((height / 9) * width) * 6) + (roblox_win.width() / 2)) as i32;

    let center_screen = ((height / 2) * width) + (roblox_win.width() / 2);
    let quarter_screen = ((center_screen) + ((height / 6) * width)) as i32;

    let offsets: [i32; 4] = [-130, -50, 50, 130];

    for off in offsets {
        let value = rgba[(two_thirds + off) as usize];
        tracks.push(RGB8::new(value[0], value[1], value[2]));
    }

    Ok(tracks)
}

fn press_key(enigo: &mut Enigo, key: char) -> Result<(), InputError> {
    enigo.key(Key::Unicode(key), Press)?;
    thread::sleep(Duration::from_millis(15));
    enigo.key(Key::Unicode(key), Release)?;
    Ok(())
}

// Producer: Captures screen data and sends it through the channel
fn producer_main_loop(
    consumers: Vec<Sender<ScreenData>>,
    stop_signal: Arc<std::sync::atomic::AtomicBool>,
) {
    while !stop_signal.load(std::sync::atomic::Ordering::Relaxed) {
        match find_roblox_window() {
            Ok(window) => {
                let height = window.height() as i32;
                let width = window.width() as i32;
                match window.capture_image() {
                    Ok(buffer) => {
                        let screen_data = ScreenData {
                            height,
                            width,
                            pixels: buffer.to_vec(),
                        };

                        for sender in &consumers {
                            if let Err(err) = sender.send(screen_data.clone()) {
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
        // Simulate 60 FPS (16ms per frame)
        thread::sleep(Duration::from_millis(16));
    }
}

// Consumer: Processes the latest screen data
fn consumer_main_loop(
    rx: Receiver<ScreenData>,
    stop_signal: Arc<AtomicBool>,
    offset: i32,
    key: char,
    mut controller: Enigo,
) {
    let mut key_down = false;

    while !stop_signal.load(Ordering::Relaxed) {
        // Drain the channel and get the latest data
        if let Some(screen_data) = rx.try_iter().last() {
            // Process the received screen data
            let index = (((((screen_data.height / 18) * screen_data.width) * 16)
                + (screen_data.width / 2))
                + offset) as usize;

            let red = screen_data
                .pixels
                .get(index * 4)
                .expect("Could not find colour!");
            let green = screen_data
                .pixels
                .get((index * 4) + 1)
                .expect("Could not find colour!");
            let blue = screen_data
                .pixels
                .get((index * 4) + 2)
                .expect("Could not find colour!");

            // First condition is for single notes, second condition is for holds

            // Note Color: 254, 226, 19
            if *red > 200 {
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
            thread::sleep(Duration::from_millis(50));
        }

        // Sleep briefly to avoid busy looping
        thread::sleep(Duration::from_millis(10));
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let tracks = ['d', 'f', 'j', 'k'];
    let offsets: [i32; 4] = [-150, -50, 50, 150];

    println!("Starting color monitoring...");
    println!("Press Ctrl+C to stop");

    // Stop signal to gracefully terminate the threads
    let stop_signal: Arc<AtomicBool> = Arc::new(AtomicBool::new(false));

    // Create channels for each consumer
    let mut consumers: Vec<Sender<ScreenData>> = Vec::new();
    let mut track_threads: Vec<thread::JoinHandle<()>> = Vec::new();

    for (i, track_id) in tracks.iter().enumerate() {
        let (tx, rx): (Sender<ScreenData>, Receiver<ScreenData>) = unbounded();
        consumers.push(tx);

        let enigo = Enigo::new(&Settings::default())?;

        let consumer_stop_signal = Arc::clone(&stop_signal);
        let key = *track_id;

        // Spawn a consumer thread for each track
        let handle = thread::spawn(move || {
            consumer_main_loop(rx, consumer_stop_signal, offsets[i], key, enigo)
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
