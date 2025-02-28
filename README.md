# Robeats Robot

A high-performance automated player for the Roblox rhythm game Robeats. This tool detects and responds to in-game note patterns by analyzing screen pixels and simulating keystrokes.

## Features

- **Real-time screen analysis**: Captures and processes game screen pixels to detect incoming notes
- **Multi-track support**: Handles all 4 game tracks simultaneously (mapped to keys D, F, J, K)
- **Adjustable timing**: Fine-tune note hit delay for optimal accuracy
- **Performance monitoring**: Real-time stats for hit rate and system performance
- **Resource efficient**: Optimized threading model with minimal CPU usage

## Technical Overview

Robeats Robot uses a producer-consumer architecture with multiple threads:

1. **Producer Thread**: Captures screen pixels at specific positions where notes appear in the game
2. **Consumer Threads**: One per track (4 total), each monitoring pixel data and triggering keystrokes
3. **Control Thread**: Handles user input and displays performance statistics

The application uses atomic variables for thread-safe communication and employs a windowing cache to minimize resource usage when capturing screen pixels.

## Requirements

- Rust 1.60 or newer
- Windows 10/11 (for `xcap` window capture functionality)
- Administrator privileges (for simulating keystrokes)
- Dependencies:
  - `device_query`: Keyboard input detection
  - `enigo`: Keyboard simulation
  - `xcap`: Screen capture
  - `ctrlc`: Signal handling

## Installation

1. Ensure you have Rust and Cargo installed ([rustup.rs](https://rustup.rs))

2. Clone the repository:
   ```bash
   git clone https://github.com/yourusername/robeats-robot.git
   cd robeats-robot
   ```

3. Build the project:
   ```bash
   cargo build --release
   ```
   Please note that the `--release` flag is necessary to get competitive performance

4. The executable will be available at `target/release/robeats-robot.exe`

## Configuration

The application uses several constants that can be adjusted in the source code:

```rust
const WINDOW_CHECK_INTERVAL: Duration = Duration::from_millis(500);
const TRACK_KEYS: [char; 4] = ['d', 'f', 'j', 'k'];
const TRACK_OFFSETS: [i32; 4] = [-143, -48, 48, 143]; 
const PIXEL_THRESHOLD: u8 = 220;
```

- `WINDOW_CHECK_INTERVAL`: How often to refresh the window handle
- `TRACK_KEYS`: Keyboard keys to simulate for each track
- `TRACK_OFFSETS`: Pixel offsets from center for each track
- `PIXEL_THRESHOLD`: Brightness threshold for note detection

## Troubleshooting

**Q: The application doesn't detect the Roblox window**  
A: Ensure Roblox is running with the window title "Roblox Game Client". Try running both Roblox and the bot with administrator privileges.

**Q: Notes aren't being hit accurately**  
A: Adjust the delay using Up/Down arrow keys. The optimal value depends on your system performance and network latency.

**Q: The application crashes or freezes**  
A: Check that you're running the latest build. Try increasing `WINDOW_CHECK_INTERVAL` if your CPU usage is high.

## How It Works

1. The producer thread captures the game window at regular intervals
2. For each track (lane), it analyzes specific pixels where notes appear
3. When a note is detected (brightness > threshold), the corresponding key is pressed
4. The key is released when the note passes (brightness < threshold)
5. The delay parameter controls the timing between detection and keystroke

The pixel offsets are calibrated for the standard Robeats layout. If the game interface changes, you may need to adjust the `TRACK_OFFSETS` constant.

## Performance Optimization

The application employs several optimizations:
- Window handle caching to reduce API calls
- Thread synchronization with atomic variables (lock-free)
- Small sleep intervals to prevent CPU saturation
- Efficient screen capture with rectangular region selection

## Legal Notice

This software is provided for educational purposes only. Using automation tools may violate the Roblox Terms of Service. Use at your own risk.

## Contributing

Contributions are welcome! Please feel free to submit a Pull Request.

## License

This project is licensed under the MIT License - see the LICENSE file for details.
