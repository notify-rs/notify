#[cfg(not(test))]
pub use build::*;

#[cfg(not(test))]
mod build {
    use std::time::Instant;

    pub fn now() -> Instant {
        Instant::now()
    }
}

#[cfg(test)]
pub use test::*;

#[cfg(test)]
mod test {
    use std::{
        sync::Mutex,
        time::{Duration, Instant},
    };

    thread_local! {
        static NOW: Mutex<Option<Instant>> = const { Mutex::new(None) };
    }

    pub fn now() -> Instant {
        let time = NOW.with(|now| *now.lock().unwrap());
        time.unwrap_or_else(Instant::now)
    }

    pub struct MockTime;

    impl MockTime {
        pub fn set_time(time: Instant) {
            NOW.with(|now| *now.lock().unwrap() = Some(time));
        }

        pub fn advance(delta: Duration) {
            NOW.with(|now| {
                if let Some(n) = &mut *now.lock().unwrap() {
                    *n += delta;
                }
            });
        }
    }
}
