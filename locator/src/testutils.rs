use std::net::{TcpListener, TcpStream};
use std::process::{Child, Command, Stdio};
use std::time::Duration;

pub struct TestControlPlaneServer {
    child: Child,
    pub port: u16,
}

impl TestControlPlaneServer {
    pub fn spawn(host: &str) -> Result<Self, Box<dyn std::error::Error>> {
        // Bind to port 0 to find a free port from the OS, then drop it so the Python server can bind to it.
        let listener = TcpListener::bind(format!("{}:0", host))?;
        let port = listener.local_addr()?.port();
        drop(listener);

        let child = Command::new("python")
            .arg("../scripts/mock_control_api.py")
            .arg("--host")
            .arg(host)
            .arg("--port")
            .arg(port.to_string())
            .stdin(Stdio::piped()) // Python server exits when this pipe closes
            .spawn()?;

        // Wait for the server to be ready
        let addr = format!("{}:{}", host, port);
        for _ in 0..10 {
            if TcpStream::connect(&addr).is_err() {
                std::thread::sleep(Duration::from_millis(100));
            } else {
                return Ok(Self { child, port });
            }
        }

        Err("Failed to connect".into())
    }
}

impl Drop for TestControlPlaneServer {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
