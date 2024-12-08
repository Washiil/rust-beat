use crossbeam::channel::{unbounded, Receiver, Sender};
use device_query::{DeviceQuery, DeviceState, Keycode};
use enigo::{
    Direction::{Press, Release},
    Enigo, Key, Keyboard, Settings,
};
use parking_lot::{Mutex, RwLock, RwLockWriteGuard};

use std::{
    sync::{
        atomic::{AtomicBool, AtomicU64, AtomicU8, Ordering},
        Arc,
    },
    time::Instant,
};
use std::{thread, time::Duration};
use xcap::Window;

struct TrackData {
    tracks: [Arc<AtomicU8>; 4],
}

impl TrackData {
    pub fn new() -> Self {
        TrackData {
            tracks: [
                Arc::new(AtomicU8::new(0)),
                Arc::new(AtomicU8::new(0)),
                Arc::new(AtomicU8::new(0)),
                Arc::new(AtomicU8::new(0)),
            ],
        }
    }
}

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
        // Only check for window every 500ms
        if self.window.is_none() || self.last_check.elapsed() > Duration::from_millis(500) {
            self.window = Window::all()
                .ok()?
                .into_iter()
                .find(|win| win.app_name() == "Roblox Game Client");
            self.last_check = Instant::now();
        }
        self.window.clone()
    }
}

fn producer_v2(
    data: Arc<TrackData>,
    operation_tracker: Arc<AtomicU64>,
    stop_signal: Arc<AtomicBool>,
) {
    let offsets: [i32; 4] = [-143, -48, 48, 143];
    let mut window_cache = WindowCache::new();

    while !stop_signal.load(Ordering::Relaxed) {
        if let Some(window) = window_cache.get_window() {
            if let Ok(buffer) = window.capture_image() {
                let height = window.height() as i32;
                let width = window.width() as i32;
                let buffer = buffer.to_vec();

                // Calculate base
                let base_index = (((height / 72) * width) * 71) + (width / 2);

                for (i, offset) in offsets.iter().enumerate() {
                    let idx = ((base_index + offset) * 4) as usize;
                    data.tracks[i].store(buffer[idx], Ordering::Release);
                }
                operation_tracker.fetch_add(1, Ordering::Relaxed);
            }
        }
    }
    println!("Shuttind Down Producer Thread");
}

fn consumer_v2(
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

        if val > 220 {
            if !key_down {
                thread::sleep(Duration::from_millis(delay));
                let _ = controller.key(unicode_key, Press);
                key_down = true;
            }
        } else if key_down {
            let _ = controller.key(unicode_key, Release);
            key_down = false;
        }
        operation_tracker.fetch_add(1, Ordering::Relaxed);
    }

    println!("Shutting down track {}", key);
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let tracks = ['d', 'f', 'j', 'k'];

    let note_delay = Arc::new(AtomicU64::new(20));
    let stop_signal = Arc::new(AtomicBool::new(false));
    let mut ops_per_sec: [Arc<AtomicU64>; 5] = [
        Arc::new(AtomicU64::new(0)),
        Arc::new(AtomicU64::new(0)),
        Arc::new(AtomicU64::new(0)),
        Arc::new(AtomicU64::new(0)),
        Arc::new(AtomicU64::new(0)),
    ];

    let device_state = DeviceState::new();

    // Set up Ctrl+C handler
    let shutdown_signal = Arc::clone(&stop_signal);
    ctrlc::set_handler(move || {
        println!("Ctrl+C detected, shutting down...");
        shutdown_signal.store(true, Ordering::Relaxed);
    })?;

    let track_data = Arc::new(TrackData::new());
    let mut track_threads = Vec::with_capacity(tracks.len());

    // Spawn consumer threads
    for (i, &track_id) in tracks.iter().enumerate() {
        let consumer_stop_signal = Arc::clone(&stop_signal);
        let consumer_note_delay = Arc::clone(&note_delay);
        let consumer_track_data = Arc::clone(&track_data.tracks[i]);
        let consumer_operation_tracker = Arc::clone(&ops_per_sec[i + 1]);
        let enigo = Enigo::new(&Settings::default())?;

        let handle = thread::spawn(move || {
            consumer_v2(
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

    let consumer_start_time = Instant::now();

    // Spawn producer thread
    let producer_handle = thread::spawn({
        let producer_stop_signal = Arc::clone(&stop_signal);
        let producer_track_data = Arc::clone(&track_data);
        let producer_operation_tracker = Arc::clone(&ops_per_sec[0]);

        move || {
            producer_v2(
                producer_track_data,
                producer_operation_tracker,
                producer_stop_signal,
            )
        }
    });

    // Main control loop
    let mut previous_keys = Vec::with_capacity(4);
    while !stop_signal.load(Ordering::Relaxed) {
        thread::sleep(Duration::from_secs(1));
        let keys = device_state.get_keys();

        for key in keys.iter() {
            if !previous_keys.contains(key) {
                match key {
                    Keycode::Up => {
                        let _ = note_delay.fetch_add(1, Ordering::Relaxed);
                    }
                    Keycode::Down => {
                        let _ = note_delay.fetch_sub(1, Ordering::Relaxed);
                    }
                    Keycode::Escape => {
                        stop_signal.store(true, Ordering::Relaxed);
                        break;
                    }
                    _ => {}
                }
            }
        }

        print!("\x1B[2J\x1B[1;1H");

        println!("- Robeats Robot -");
        println!("Delay: {}ms", note_delay.load(Ordering::Relaxed));

        let seconds = consumer_start_time.elapsed().as_secs();

        println!(
            "Producer: {} writes/sec",
            ops_per_sec[0].load(Ordering::Relaxed) / seconds
        );

        for (i, c) in tracks.iter().enumerate() {
            println!(
                "Track [{}]: {} reads/sec",
                c,
                ops_per_sec[i + 1].load(Ordering::Relaxed) / seconds
            );
        }

        thread::sleep(Duration::from_millis(10));
        previous_keys = keys;
    }

    // Wait for threads to finish
    for handle in track_threads {
        let _ = handle.join();
    }
    let _ = producer_handle.join();

    println!("Shutdown complete");
    Ok(())
}
