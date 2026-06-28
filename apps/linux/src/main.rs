#[cfg(target_os = "linux")]
mod app;

#[cfg(target_os = "linux")]
fn main() {
    app::run();
}

#[cfg(not(target_os = "linux"))]
fn main() {
    eprintln!("yanklog-linux-native runs on Linux. Use cargo test -p yanklog-core on this host.");
}
