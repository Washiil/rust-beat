use crossbeam::channel::{unbounded, Receiver, Sender};
use device_query::{DeviceQuery, DeviceState, Keycode};
use std::sync::{
    atomic::{AtomicBool, AtomicU64, Ordering},
    Arc,
};
use xcap::Window;
use enigo::{Direction::{Click, Press, Release}, Keyboard, Enigo, Key, Settings};
use std::{thread, time::Duration};

// Cache the window reference to avoid repeated lookups
struct WindowCache {
    window: Option<Window>,
    last_check: std::time::Instant,
}

impl WindowCache {
    fn new() -> Self {
        Self {
            window: None,
            last_check: std::time::Instant::now(),
        }
    }

    fn get_window(&mut self) -> Result<&Window, &'static str> {
        // Only refresh cache every second
        if self.window.is_none() || self.last_check.elapsed() > Duration::from_secs(1) {
            self.window = find_roblox_window().ok();
            self.last_check = std::time::Instant::now();
        }
        self.window.as_ref().ok_or("Window not found")
    }
}

fn find_roblox_window() -> Result<Window, &'static str> {
    Window::all()
        .map_err(|_| "Could not access open windows, try running in administrator mode")?
        .into_iter()
        .find(|win| win.app_name() == "Roblox Game Client")
        .ok_or("Roblox is not open or the window could not be found")
}

// Optimized producer that uses pre-allocated buffers and more efficient pixel access
fn producer_main_loop(
    consumers: Vec<Sender<[[u8; 3]; 4]>>,
    stop_signal: Arc<AtomicBool>,
) {
    let mut window_cache = WindowCache::new();
    let offsets: [i32; 4] = [-150, -50, 50, 150];
    let mut pixels = [[0u8; 3]; 4];
    
    while !stop_signal.load(Ordering::Relaxed) {
        if let Ok(window) = window_cache.get_window() {
            if let Ok(buffer) = window.capture_image() {
                let height = window.height() as i32;
                let width = window.width() as i32;
                let buffer = buffer.to_vec();
                
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
        
        // Small sleep to prevent excessive CPU usage
        thread::sleep(Duration::from_micros(100));
    }
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
        if let Ok(screen_data) = rx.try_recv() {  // Use try_recv() instead of try_iter()
            if screen_data[index][0] > 220 {
                if !key_down && last_action_time.elapsed() >= Duration::from_millis(note_delay.load(Ordering::Relaxed)) {
                    controller.key(Key::Unicode(key), Press);
                    key_down = true;
                    last_action_time = std::time::Instant::now();
                }
            } else if key_down {
                controller.key(Key::Unicode(key), Release);
                key_down = false;
                last_action_time = std::time::Instant::now();
            }
        }
        
        thread::sleep(Duration::from_micros(100));
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let tracks = ['d', 'f', 'j', 'k'];
    let note_delay = Arc::new(AtomicU64::new(20));
    let device_state = DeviceState::new();
    let stop_signal = Arc::new(AtomicBool::new(false));

    // Set up Ctrl+C handler
    let shutdown_signal = Arc::clone(&stop_signal);
    ctrlc::set_handler(move || {
        println!("Ctrl+C detected, shutting down...");
        shutdown_signal.store(true, Ordering::Relaxed);
    })?;

    // Pre-allocate vectors
    let mut consumers = Vec::with_capacity(tracks.len());
    let mut track_threads = Vec::with_capacity(tracks.len());

    // Create channels and spawn consumer threads
    for (i, &track_id) in tracks.iter().enumerate() {
        let (tx, rx) = unbounded();
        consumers.push(tx);

        let consumer_stop_signal = Arc::clone(&stop_signal);
        let consumer_note_delay = Arc::clone(&note_delay);
        let enigo = Enigo::new(&Settings::default())?;

        let handle = thread::spawn(move || {
            consumer_main_loop(rx, consumer_stop_signal, consumer_note_delay, i, track_id, enigo)
        });
        track_threads.push(handle);
    }

    // Spawn producer thread with cloned stop signal
    let producer_stop_signal = Arc::clone(&stop_signal);
    let producer_handle = thread::spawn(move || producer_main_loop(consumers, producer_stop_signal));

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