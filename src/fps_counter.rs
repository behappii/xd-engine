use std::time::{Duration, Instant};

pub struct FpsCounter {
    last_update: Instant,
    frame_count: u32,
    fps: u32,
}

impl FpsCounter {
    pub fn new() -> Self {
        Self {
            last_update: Instant::now(),
            frame_count: 0,
            fps: 0,
        }
    }

    pub fn tick(&mut self) -> bool {
        self.frame_count += 1;

        if self.last_update.elapsed() >= Duration::from_secs(1) {
            self.fps = self.frame_count;
            self.frame_count = 0;
            self.last_update = Instant::now();

            return true;
        }

        false
    }

    pub fn fps(&self) -> u32 {
        self.fps
    }
}
