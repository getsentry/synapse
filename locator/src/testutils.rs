use std::net::TcpStream;
use std::process::{Child, Command};
use std::time::Duration;

pub struct TestControlPlaneServer {
    child: Child,
}

impl TestControlPlaneServer {
    pub fn spawn(host: &str, port: u16) -> Result<Self, Box<dyn std::error::Error>> {
        let addr = format!("{}:{}", host, port);

        let child = Command::new("python")
            .arg("../scripts/mock_control_api.py")
            .arg("--host")
            .arg(host)
            .arg("--port")
            .arg(port.to_string())
            .spawn()?;

        // Wait for tcp
        for _ in 0..10 {
            if TcpStream::connect(&addr).is_err() {
                std::thread::sleep(Duration::from_millis(100));
            } else {
                return Ok(Self { child });
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
