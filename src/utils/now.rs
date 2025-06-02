use chrono::Utc;

pub fn now() -> u64 {
    Utc::now().timestamp_micros() as u64
}
