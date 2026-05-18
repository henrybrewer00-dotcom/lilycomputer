pub mod protocol;
pub mod config;

pub const DAEMON_PORT: u16 = 7777;
pub const DAEMON_HOST: &str = "127.0.0.1";

pub fn daemon_base_url() -> String {
    format!("http://{}:{}", DAEMON_HOST, DAEMON_PORT)
}
