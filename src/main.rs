use std::env;
use std::fs::File;
use std::io::{self, BufRead, BufReader, Seek, SeekFrom, Write};
use std::path::Path;
use std::thread;
use std::time::Duration;
use std::process;
use std::fs;
use std::time::SystemTime;

// Windows-specific imports for console handling
#[cfg(windows)]
// Removed unused import for SetConsoleCtrlHandler
#[cfg(windows)]
use winapi::um::consoleapi::SetConsoleMode;
#[cfg(windows)]
use winapi::um::wincon::{ENABLE_VIRTUAL_TERMINAL_PROCESSING, ENABLE_PROCESSED_INPUT};
#[cfg(windows)]
use winapi::um::processenv::GetStdHandle;
#[cfg(windows)]
use winapi::um::winbase::STD_OUTPUT_HANDLE;

// Setup proper console handling for Windows
#[cfg(windows)]
fn setup_windows_console() -> io::Result<()> {
    unsafe {
        let handle = GetStdHandle(STD_OUTPUT_HANDLE);
        if handle == std::ptr::null_mut() {
            return Err(io::Error::last_os_error());
        }
        
        let mut mode = 0;
        if SetConsoleMode(handle, ENABLE_VIRTUAL_TERMINAL_PROCESSING | ENABLE_PROCESSED_INPUT) == 0 {
            return Err(io::Error::last_os_error());
        }
    }
    Ok(())
}

#[cfg(not(windows))]
fn setup_windows_console() -> io::Result<()> {
    // No-op for non-Windows platforms
    Ok(())
}

fn main() -> io::Result<()> {
    // Set up Windows console for better terminal handling
    setup_windows_console()?;
    
    let args: Vec<String> = env::args().collect();
    
    if args.len() < 2 {
        eprintln!("Usage: {} <filename> [-f] [-n lines]", args[0]);
        eprintln!("  -f              Follow mode: output appended data as the file grows");
        eprintln!("  -n <num_lines>  Output the last NUM lines (default: 10)");
        eprintln!("  --retry         Keep trying to open the file if it's not accessible");
        return Ok(());
    }
    
    let filename = &args[1];
    let mut follow_mode = false;
    let mut num_lines = 10;
    let mut retry_mode = false;
    
    let mut i = 2;
    while i < args.len() {
        match args[i].as_str() {
            "-f" => {
                follow_mode = true;
                i += 1;
            }
            "--retry" => {
                retry_mode = true;
                i += 1;
            }
            "-n" => {
                if i + 1 < args.len() {
                    match args[i + 1].parse::<usize>() {
                        Ok(n) => num_lines = n,
                        Err(_) => {
                            eprintln!("Error: Invalid number of lines: {}", args[i + 1]);
                            process::exit(1);
                        }
                    }
                    i += 2;
                } else {
                    eprintln!("Error: -n requires a number argument");
                    process::exit(1);
                }
            }
            _ => {
                eprintln!("Unknown option: {}", args[i]);
                process::exit(1);
            }
        }
    }

    // Check if file exists first
    let path = Path::new(filename);
    if !path.exists() && !retry_mode {
        eprintln!("Error: File '{}' not found", filename);
        process::exit(1);
    }

    if retry_mode {
        while !path.exists() {
            println!("Waiting for file '{}' to appear...", filename);
            thread::sleep(Duration::from_secs(1));
        }
    }

    // Print last N lines
    match tail_file(filename, num_lines) {
        Ok(_) => {}
        Err(e) => {
            eprintln!("Error reading file: {}", e);
            if retry_mode {
                println!("Retrying in 1 second...");
                thread::sleep(Duration::from_secs(1));
            } else {
                process::exit(1);
            }
        }
    }

    // If follow mode, monitor file for changes
    if follow_mode {
        println!("Following file '{}'. Press Ctrl+C to stop.", filename);
        follow_file(filename, retry_mode)?;
    }

    Ok(())
}

fn tail_file(filename: &str, num_lines: usize) -> io::Result<()> {
    let file = File::open(filename)?;
    let mut reader = BufReader::new(file);
    
    let mut lines = Vec::new();
    let mut line = String::new();
    
    while reader.read_line(&mut line)? > 0 {
        // Handle Windows CRLF line endings
        if line.ends_with("\r\n") {
            line.pop();
            line.pop();
            line.push('\n');
        } else if line.ends_with('\n') {
            // Leave Unix-style line endings as is
        } else {
            line.push('\n'); // Add newline if missing
        }
        
        lines.push(line.clone());
        if lines.len() > num_lines {
            lines.remove(0);
        }
        line.clear();
    }
    
    for line in &lines {
        print!("{}", line);
    }
    
    io::stdout().flush().unwrap();
    Ok(())
}

fn follow_file(filename: &str, retry_mode: bool) -> io::Result<()> {
    let mut file = match File::open(filename) {
        Ok(f) => BufReader::new(f),
        Err(e) => {
            if retry_mode {
                println!("Error opening file: {}. Retrying...", e);
                thread::sleep(Duration::from_secs(1));
                return follow_file(filename, retry_mode);
            } else {
                return Err(e);
            }
        }
    };
    
    // Seek to the end
    let mut pos = file.seek(SeekFrom::End(0))?;
    
    let mut last_modified = match fs::metadata(filename) {
        Ok(metadata) => metadata.modified().unwrap_or(SystemTime::now()),
        Err(_) => SystemTime::now(),
    };
    
    loop {
        // Check if file has been rotated (common in Windows logs)
        match fs::metadata(filename) {
            Ok(metadata) => {
                let current_modified = metadata.modified().unwrap_or(SystemTime::now());
                
                // If the file's modified time changed and it's smaller than before, it was probably rotated
                let current_size = metadata.len();
                if current_modified != last_modified && current_size < pos as u64 {
                    println!("\n--- Log file rotation detected ---\n");
                    // Reopen the file
                    drop(file);
                    file = BufReader::new(File::open(filename)?);
                    pos = 0;
                }
                
                last_modified = current_modified;
            },
            Err(e) => {
                if retry_mode {
                    println!("File access error: {}. Retrying...", e);
                    thread::sleep(Duration::from_secs(1));
                    continue;
                } else {
                    return Err(e);
                }
            }
        }
        
        // Seek to where we were before
        file.seek(SeekFrom::Start(pos))?;
        
        let mut buffer = String::new();
        let bytes_read = file.read_line(&mut buffer)?;
        
        if bytes_read > 0 {
            // Handle Windows CRLF line endings
            if buffer.ends_with("\r\n") {
                buffer.pop();
                buffer.pop();
                buffer.push('\n');
            }
            
            print!("{}", buffer);
            io::stdout().flush().unwrap();
            pos += bytes_read as u64;
        } else {
            // No new data, wait a bit before checking again
            // Windows file locking might prevent access, so we use a shorter interval
            thread::sleep(Duration::from_millis(100));
            
            // Handle the case where the file was truncated (common in log rotation)
            let metadata = fs::metadata(filename)?;
            let size = metadata.len();
            if size < pos {
                println!("\n--- File was truncated or rotated ---\n");
                // Start from the beginning
                file.seek(SeekFrom::Start(0))?;
                pos = 0;
            }
        }
    }
}
