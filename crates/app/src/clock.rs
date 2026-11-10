//! Time since boot, including time spent in suspend: a laptop that sleeps
//! through the end of a period shows "Time's up" right after waking up.

use std::time::Duration;

pub fn now() -> Duration {
    let mut ts = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    // SAFETY: `ts` is a valid, writable timespec.
    let rc = unsafe { libc::clock_gettime(libc::CLOCK_BOOTTIME, &mut ts) };
    assert_eq!(
        rc, 0,
        "CLOCK_BOOTTIME is available on every supported Linux"
    );
    Duration::new(ts.tv_sec as u64, ts.tv_nsec as u32)
}

#[cfg(test)]
mod tests {
    #[test]
    fn moves_forward() {
        let a = super::now();
        std::thread::sleep(std::time::Duration::from_millis(5));
        assert!(super::now() > a);
    }
}
