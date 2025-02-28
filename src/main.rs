use device_query::{DeviceQuery, DeviceState, Keycode};
use enigo::{Direction, Enigo, Key, Settings, Keyboard};
use std::{
    sync::{
        atomic::{AtomicBool, AtomicU64, AtomicU8, Ordering},
        Arc,
    },
    thread,
    time::{Duration, Instant},
};
use xcap::Window;

// Constants
const WINDOW_CHECK_INTERVAL: Duration = Duration::from_millis(500);
const TRACK_KEYS: [char; 4] = ['d', 'f', 'j', 'k'];
const TRACK_OFFSETS: [i32; 4] = [-143, -48, 48, 143];
const ROBLOX_APP_NAME: &str = "Roblox Game Client";
const PIXEL_THRESHOLD: u8 = 220;

// Track data shared between threads
struct TrackData {
    tracks: [Arc<AtomicU8>; 4],
}

impl TrackData {
    fn new() -> Self {
        Self {
            tracks: core::array::from_fn(|_| Arc::new(AtomicU8::new(0))),
        }
    }
}

// Window caching to avoid frequent lookups
struct WindowCache {
    window: Option<Window>,
    last_check: Instant,
}

impl WindowCache {
    fn new() -> Self {
        Self {
            window: None,
            last_check: Instant::now(),
        }
    }

    fn get_window(&mut self) -> Option<Window> {
        if self.window.is_none() || self.last_check.elapsed() > WINDOW_CHECK_INTERVAL {
            self.window = Window::all()
                .ok()?
                .into_iter()
                .find(|win| win.app_name() == ROBLOX_APP_NAME);
            self.last_check = Instant::now();
        }
        self.window.clone()
    }
}

// Producer thread that captures screen pixels
fn producer(
    data: Arc<TrackData>,
    operation_tracker: Arc<AtomicU64>,
    stop_signal: Arc<AtomicBool>,
) {
    let mut window_cache = WindowCache::new();

    while !stop_signal.load(Ordering::Relaxed) {
        if let Some(window) = window_cache.get_window() {
            if let Ok(buffer) = window.capture_image() {
                let height = window.height() as i32;
                let width = window.width() as i32;
                let buffer = buffer.to_vec();

                // Calculate base position for tracking
                let base_index = (((height / 72) * width) * 71) + (width / 2);

                for (i, offset) in TRACK_OFFSETS.iter().enumerate() {
                    let idx = ((base_index + offset) * 4) as usize;
                    if idx < buffer.len() {
                        data.tracks[i].store(buffer[idx], Ordering::Release);
                    }
                }
                operation_tracker.fetch_add(1, Ordering::Relaxed);
            }
        }
        
        // Small sleep to prevent CPU hogging
        thread::sleep(Duration::from_millis(1));
    }
    println!("Shutting down producer thread");
}

// Consumer thread that presses keys based on track data
fn consumer_track(
    data: Arc<AtomicU8>,
    stop_signal: Arc<AtomicBool>,
    note_delay: Arc<AtomicU64>,
    operation_tracker: Arc<AtomicU64>,
    key: char,
    mut controller: Enigo,
) {
    let mut key_down = false;
    let unicode_key = Key::Unicode(key);

    while !stop_signal.load(Ordering::Relaxed) {
        let val = data.load(Ordering::Acquire);
        let delay = note_delay.load(Ordering::Relaxed);

        if val > PIXEL_THRESHOLD {
            if !key_down {
                thread::sleep(Duration::from_millis(delay));
                let _ = controller.key(unicode_key, Direction::Press);
                key_down = true;
            }
        } else if key_down {
            let _ = controller.key(unicode_key, Direction::Release);
            key_down = false;
        }
        operation_tracker.fetch_add(1, Ordering::Relaxed);
        
        // Small sleep to prevent CPU hogging
        thread::sleep(Duration::from_millis(1));
    }

    // Ensure key is released on shutdown
    if key_down {
        let _ = controller.key(unicode_key, Direction::Release);
    }

    println!("Shutting down track {}", key);
}

// Setup the shared metrics
struct Metrics {
    note_delay: Arc<AtomicU64>,
    stop_signal: Arc<AtomicBool>,
    ops_per_sec: Vec<Arc<AtomicU64>>,
}

impl Metrics {
    fn new() -> Self {
        Self {
            note_delay: Arc::new(AtomicU64::new(5)),
            stop_signal: Arc::new(AtomicBool::new(false)),
            ops_per_sec: (0..5).map(|_| Arc::new(AtomicU64::new(0))).collect(),
        }
    }
    
    fn increase_delay(&self) {
        self.note_delay.fetch_add(1, Ordering::Relaxed);
    }
    
    fn decrease_delay(&self) {
        let current = self.note_delay.load(Ordering::Relaxed);
        if current > 0 {
            self.note_delay.fetch_sub(1, Ordering::Relaxed);
        }
    }
    
    fn request_stop(&self) {
        self.stop_signal.store(true, Ordering::Relaxed);
    }
    
    fn is_stopping(&self) -> bool {
        self.stop_signal.load(Ordering::Relaxed)
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize metrics
    let metrics = Metrics::new();
    
    // Set up Ctrl+C handler
    let shutdown_signal = Arc::clone(&metrics.stop_signal);
    ctrlc::set_handler(move || {
        println!("Ctrl+C detected, shutting down...");
        shutdown_signal.store(true, Ordering::Relaxed);
    })?;

    let track_data = Arc::new(TrackData::new());
    let mut track_threads = Vec::with_capacity(TRACK_KEYS.len());

    // Start the consumer threads for each track
    for (i, &track_id) in TRACK_KEYS.iter().enumerate() {
        let consumer_stop_signal = Arc::clone(&metrics.stop_signal);
        let consumer_note_delay = Arc::clone(&metrics.note_delay);
        let consumer_track_data = Arc::clone(&track_data.tracks[i]);
        let consumer_operation_tracker = Arc::clone(&metrics.ops_per_sec[i + 1]);
        
        // Create keyboard controller
        let enigo = match Enigo::new(&Settings::default()) {
            Ok(e) => e,
            Err(e) => {
                eprintln!("Error creating keyboard controller: {}", e);
                return Err(e.into());
            }
        };

        let handle = thread::spawn(move || {
            consumer_track(
                consumer_track_data,
                consumer_stop_signal,
                consumer_note_delay,
                consumer_operation_tracker,
                track_id,
                enigo,
            )
        });
        track_threads.push(handle);
    }

    let start_time = Instant::now();

    // Start the producer thread
    let producer_handle = {
        let producer_stop_signal = Arc::clone(&metrics.stop_signal);
        let producer_track_data = Arc::clone(&track_data);
        let producer_operation_tracker = Arc::clone(&metrics.ops_per_sec[0]);

        thread::spawn(move || {
            producer(
                producer_track_data,
                producer_operation_tracker,
                producer_stop_signal,
            )
        })
    };

    // Main control loop
    let device_state = DeviceState::new();
    let mut previous_keys = Vec::new();
    
    while !metrics.is_stopping() {
        thread::sleep(Duration::from_millis(100));
        let keys = device_state.get_keys();

        // Process keyboard input
        for key in keys.iter() {
            if !previous_keys.contains(key) {
                match key {
                    Keycode::Up => metrics.increase_delay(),
                    Keycode::Down => metrics.decrease_delay(),
                    Keycode::Escape => {
                        metrics.request_stop();
                        break;
                    }
                    _ => {}
                }
            }
        }
        
        // Update display every second
        if start_time.elapsed().as_millis() % 1000 < 100 {
            print_status(&metrics, &start_time, &TRACK_KEYS);
        }
        
        previous_keys = keys;
    }

    // Wait for all threads to finish
    for handle in track_threads {
        let _ = handle.join();
    }
    let _ = producer_handle.join();

    println!("Shutdown complete");
    Ok(())
}

// Display current status information
fn print_status(metrics: &Metrics, start_time: &Instant, track_keys: &[char]) {
    // Clear screen
    print!("\x1B[2J\x1B[1;1H");

    println!("- Robeats Robot -");
    println!("Delay: {}ms", metrics.note_delay.load(Ordering::Relaxed));

    let seconds = start_time.elapsed().as_secs().max(1); // Avoid division by zero

    println!(
        "Producer: {} writes/sec",
        metrics.ops_per_sec[0].load(Ordering::Relaxed) / seconds
    );

    for (i, c) in track_keys.iter().enumerate() {
        println!(
            "Track [{}]: {} reads/sec",
            c,
            metrics.ops_per_sec[i + 1].load(Ordering::Relaxed) / seconds
        );
    }
}