use std::process::{Child, Command};

pub struct TestControlPlaneServer {
    child: Child,
}

impl TestControlPlaneServer {
    pub fn spawn(host: &str, port: u16) -> std::io::Result<Self> {
        let child = Command::new("python")
            .arg("../scripts/mock_control_api.py")
            .arg("--host")
            .arg(host)
            .arg("--port")
            .arg(port.to_string())
            .spawn()?;

        Ok(Self { child })
    }
}

impl Drop for TestControlPlaneServer {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
