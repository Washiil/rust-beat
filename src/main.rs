use crossbeam::channel::{unbounded, Receiver, Sender};
use device_query::{DeviceQuery, DeviceState, Keycode};
use enigo::{
    Direction::{Press, Release},
    Enigo, Key, Keyboard, Settings,
};
use parking_lot::{
    Mutex,
    RwLock,
    RwLockWriteGuard
};

use std::{sync::{
    atomic::{AtomicBool, AtomicU64, AtomicU8, Ordering},
    Arc,
}, time::Instant};
use std::{thread, time::Duration};
use xcap::Window;

fn find_roblox_window() -> Result<Window, &'static str> {
    Window::all()
        .map_err(|_| "Could not access open windows, try running in administrator mode")?
        .into_iter()
        .find(|win| win.app_name() == "Roblox Game Client")
        .ok_or("Roblox is not open or the window could not be found")
}

struct TrackData {
    tracks: [Arc<AtomicU8>; 4]
}

impl TrackData {
    pub fn new() -> Self {
        TrackData {
            tracks: [Arc::new(AtomicU8::new(0)), Arc::new(AtomicU8::new(0)), Arc::new(AtomicU8::new(0)), Arc::new(AtomicU8::new(0))]
        }
    }
}

fn producer_v2(data: Arc<TrackData>, stop_signal: Arc<AtomicBool>) {
    let offsets: [i32; 4] = [-143, -48, 48, 143];
    let mut timer = Instant::now();
    let mut updates = 0;

    while !stop_signal.load(Ordering::Relaxed) {
        if let Ok(window) = find_roblox_window() {
            if let Ok(buffer) = window.capture_image() {
                let height = window.height() as i32;
                let width = window.width() as i32;
                let buffer = buffer.to_vec();

                // Calculate base
                let base_index = ((height / 72) * width * 69) + (width / 2);

                for (i, offset) in offsets.iter().enumerate() {
                    let idx = ((base_index + offset) * 4) as usize;
                    data.tracks[i].store(buffer[idx], Ordering::Release);
                }

                updates += 1;
            }
        }

        if timer.elapsed() > Duration::from_secs(5) {
            println!("Thread (producer) {} updates/sec", updates / 5);
            timer = Instant::now();
            updates = 0;
        }
    }
    println!("Shuttind Down Producer Thread");
}

fn producer_main_loop(consumers: Vec<Sender<[[u8; 3]; 4]>>, stop_signal: Arc<AtomicBool>) {
    // Need to change this so that offsets update dynamically
    let offsets: [i32; 4] = [-143, -48, 48, 143];

    // Implementing a double buffer
    let mut pixel_buffer = [[1u8; 3]; 4];
    let mut pixels = [[0u8; 3]; 4];

    while !stop_signal.load(Ordering::Relaxed) {
        if let Ok(window) = find_roblox_window() {
            if let Ok(buffer) = window.capture_image() {
                let height = window.height() as i32;
                let width = window.width() as i32;
                let buffer = buffer.to_vec();

                // Calculate base index once
                let base_index = ((height / 72) * width * 69) + (width / 2);

                // Use iterator for more efficient processing
                for (pixel, &offset) in pixels.iter_mut().zip(offsets.iter()) {
                    let idx = ((base_index + offset) * 4) as usize;
                    if idx + 2 < buffer.len() {
                        pixel.copy_from_slice(&buffer[idx..idx + 3]);
                    }
                }

                // Simple check that reduces redundant sends to our consumer threads
                if pixels != pixel_buffer {
                    pixel_buffer = pixels;
                    for sender in &consumers {
                        let _ = sender.send(pixels); // Ignore errors for speed
                    }
                }
            }
        }
    }

    println!("Shutting down producer.");
}

fn consumer_v2(
    data: Arc<AtomicU8>,
    stop_signal: Arc<AtomicBool>,
    note_delay: Arc<AtomicU64>,
    index: usize,
    key: char,
    mut controller: Enigo,
) {
    let mut key_down = false;
    let mut last_action_time = std::time::Instant::now();

    let mut timer = Instant::now();
    let mut updates = 0;

    while !stop_signal.load(Ordering::Relaxed) {
        let val = data.load(Ordering::Acquire);
        updates += 1;
        let delay = note_delay.load(Ordering::Relaxed);

        if val > 220 {
            if !key_down {
                thread::sleep(Duration::from_millis(delay));
                let _ = controller.key(Key::Unicode(key), Press);
                key_down = true;
                last_action_time = std::time::Instant::now();
            }
        } else if key_down && last_action_time.elapsed() >= Duration::from_millis(delay) {
            let _ = controller.key(Key::Unicode(key), Release);
            key_down = false;
            last_action_time = std::time::Instant::now();
        }

        if timer.elapsed() > Duration::from_secs(5) {
            println!("Thread: ({}) {} read/sec", key, updates / 5);
            timer = Instant::now();
            updates = 0;
        }

        // This one thread sleep single handedly saves your cpu
        thread::sleep(Duration::from_millis(1));
    }

    println!("Shuttind down track: {}", key);
}

// Optimized consumer with better state management
fn consumer_main_loop(
    rx: Receiver<[[u8; 3]; 4]>,
    stop_signal: Arc<AtomicBool>,
    note_delay: Arc<AtomicU64>,
    index: usize,
    key: char,
    mut controller: Enigo,
) {
    let mut key_down = false;
    let mut last_action_time = std::time::Instant::now();

    while !stop_signal.load(Ordering::Relaxed) {
        if let Ok(screen_data) = rx.try_recv() {
            let del = note_delay.load(Ordering::Relaxed);
            // Need to work on this delay calculation to handle the changes in the varied note delays while retaining the holds on long notes
            if screen_data[index][0] > 220 {
                if !key_down {
                    thread::sleep(Duration::from_millis(del));
                    let _ = controller.key(Key::Unicode(key), Press);
                    key_down = true;
                    last_action_time = std::time::Instant::now();
                }
            } else if key_down && last_action_time.elapsed() >= Duration::from_millis(del) {
                let _ = controller.key(Key::Unicode(key), Release);
                key_down = false;
                last_action_time = std::time::Instant::now();
            }
        }
    }

    println!("Shuttind down track: {}", key);
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let tracks = ['d', 'f', 'j', 'k'];

    let note_delay = Arc::new(AtomicU64::new(20));
    let stop_signal = Arc::new(AtomicBool::new(false));

    let device_state = DeviceState::new();

    // Set up Ctrl+C handler
    let shutdown_signal = Arc::clone(&stop_signal);
    ctrlc::set_handler(move || {
        println!("Ctrl+C detected, shutting down...");
        shutdown_signal.store(true, Ordering::Relaxed);
    })?;

    // Pre-allocate vectors
    let mut track_threads = Vec::with_capacity(tracks.len());

    let track_data = Arc::new(TrackData::new());

    // Create channels and spawn consumer threads
    for (i, &track_id) in tracks.iter().enumerate() {
        let consumer_stop_signal = Arc::clone(&stop_signal);
        let consumer_note_delay = Arc::clone(&note_delay);
        let consumer_track_data = Arc::clone(&track_data.tracks[i]);
        
        let enigo = Enigo::new(&Settings::default())?;

        let handle = thread::spawn(move || {
            consumer_v2(
                consumer_track_data,
                consumer_stop_signal,
                consumer_note_delay,
                i,
                track_id,
                enigo,
            )
        });
        track_threads.push(handle);
    }

    // Spawn producer thread with cloned stop signal
    let producer_stop_signal = Arc::clone(&stop_signal);
    let producer_handle =
        thread::spawn(move || producer_v2(Arc::clone(&track_data), producer_stop_signal));

    // Main control loop with improved key handling
    let mut previous_keys = Vec::with_capacity(4);
    while !stop_signal.load(Ordering::Relaxed) {
        let keys = device_state.get_keys();

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
                        stop_signal.store(true, Ordering::Relaxed);
                        break;
                    }
                    _ => {}
                }
            }
        }

        previous_keys = keys;
        thread::sleep(Duration::from_millis(10));
    }

    // Wait for threads to finish
    for handle in track_threads {
        let _ = handle.join();
    }
    let _ = producer_handle.join();

    println!("Shutdown complete");
    Ok(())
}
