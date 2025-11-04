use std::time::Duration;
use std::process::{Child, Command};
use std::net::TcpStream;

pub struct TestControlPlaneServer {
    child: Child,
}

impl TestControlPlaneServer {
    pub fn spawn(host: &str, port: u16) -> std::io::Result<Self> {
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
            }
        }
        Ok(Self { child })
    }
}

impl Drop for TestControlPlaneServer {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
