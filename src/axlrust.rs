use std::fs::File;
use std::thread;

/// Minimal stub representing the former `AxlRust` binary.
///
/// In the original code the tunnel process was spawned as an external
/// command.  Here we simply run the functionality in a separate thread.
pub struct AxlRust {
    args: Vec<String>,
    stderr: Option<File>,
}

impl AxlRust {
    pub fn new(args: Vec<String>, stderr: Option<File>) -> Self {
        Self { args, stderr }
    }

    /// Spawn the AxlRust logic in a thread.  For the purposes of this
    /// repository we simply print the provided arguments.
    pub fn spawn(self) -> thread::JoinHandle<()> {
        thread::spawn(move || {
            if let Some(mut f) = self.stderr {
                use std::io::Write;
                let _ = writeln!(f, "AxlRust invoked with args: {:?}", self.args);
            } else {
                println!("AxlRust invoked with args: {:?}", self.args);
            }
        })
    }
}
