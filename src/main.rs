use xcap::Window;
use std::sync::Arc;
use crossbeam::channel::{unbounded, Receiver, Sender};

use rgb::RGB8;
use enigo::{
    Direction::{Click, Press, Release},
    Enigo, Key, Keyboard, Mouse, Settings, InputError
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
        .ok_or("Roblox is not open or the window could not be found").cloned()
}

struct pixel_reader<'a> {
    window_ref: Arc<RwLock<Window>>,
    height: Arc<RwLock<i32>>,
    width: Arc<RwLock<i32>>,
    pixels: Arc<RwLock<Vec<&'a [u8]>>>,
}

impl<'a> pixel_reader<'a> {
    fn update_window_reference(&mut self) -> Result<bool, Box<dyn std::error::Error>> {
        let roblox_win = find_roblox_window()?;

        {
            let mut win_ref = self.window_ref.write().unwrap();
            *win_ref = roblox_win
        }

        let mut height_ref = self.height.write().unwrap();
        
        Ok(true)
    }

    pub fn main_loop(&mut self, stop_signal: Arc<AtomicBool>) -> Result<(), Box<dyn std::error::Error>> {
        loop {
            if stop_signal.load(Ordering::Relaxed) {
                break;
            }

            let roblox_win = match find_roblox_window() {
                Ok(win) => win,
                Err(err) => {
                    eprintln!("Error finding Roblox window: {}", err);
                    thread::sleep(Duration::from_millis(100)); // Wait before retrying
                    continue;
                }
            };

            // Update dimensions
            // Height
            {
                let mut height_ref = self.height.write().unwrap();
                *height_ref = roblox_win.height() as i32;
            }
            // Width
            {
                let mut width_ref = self.width.write().unwrap();
                *width_ref = roblox_win.width() as i32;
            }

            
            // Capture image and update pixel data
            match roblox_win.capture_image() {
                Ok(image) => {
                    let buffer = image.to_vec();
                    let rgba_pixels: Vec<&[u8]> = buffer.chunks_exact(4).collect();

                    // Update the shared pixels vector
                    let mut pixels_ref = self.pixels.write().unwrap();
                    *pixels_ref = rgba_pixels;
                }
                Err(err) => {
                    eprintln!("Error capturing image: {}", err);
                    thread::sleep(Duration::from_millis(100)); // Wait before retrying
                    continue;
                }
            }

            // Sleep to limit the frequency of updates (e.g., 60 FPS = 16ms per frame)
            thread::sleep(Duration::from_millis(16));
        }
        Ok(())
    }
}



// The bottom of the score line on the right is 66% of the way down the screen so essentially cut in thirds

fn get_pixels() -> Result<Vec<RGB8>, Box<dyn std::error::Error>> {
    // Get the primary screen
    let roblox_win = find_roblox_window()?;

    let height  = roblox_win.height();
    let width = roblox_win.width();

    let mut tracks = Vec::with_capacity(4);

    let image = roblox_win.capture_image()?;
    let buffer = image.to_vec();
    let rgba: Vec<&[u8]> = buffer.chunks_exact(4).collect();

    let two_thirds = ((((height / 9) * width) * 6) + (roblox_win.width() / 2)) as i32;


    let center_screen = ((height / 2) * width) + (roblox_win.width() / 2);
    let quarter_screen = ((center_screen) + ((height / 6) * width)) as i32;
    
    let offsets: [i32; 4] = [
        -130, -50, 50, 130
    ];

    for off in offsets {
        let value = rgba[(two_thirds + off) as usize];
        tracks.push(RGB8::new(
            value[0],
            value[1],
            value[2]
        ));
    }

    Ok(tracks)
}

fn press_key(enigo: &mut Enigo, key: char) -> Result<(), InputError> {
    enigo.key(Key::Unicode(key), Press)?;
    thread::sleep(Duration::from_millis(15));
    enigo.key(Key::Unicode(key), Release)?;
    Ok(())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let tracks = ['d', 'f', 'j', 'k'];
    let mut enigo = Enigo::new(&Settings::default())?;
    
    // Target color (yellow note)
    let yellow_note = RGB8::new(255, 229, 15);
    
    println!("Starting color monitoring...");
    println!("Press Ctrl+C to stop");

    loop {
        if let Ok(colors) = get_pixels() {
            // Print all colors and check for matches
            for (i, color) in colors.iter().enumerate() {
                if color.r > 230 {
                    if let Err(e) = press_key(&mut enigo, tracks[i]) {
                        println!("Key press error: {}", e);
                    }
                }
            }
            // println!();
        }

        thread::sleep(Duration::from_millis(20));
    }
}