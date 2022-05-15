use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy)]
pub struct Task {
    pub id: usize,
    pub update_time: Instant,
}

pub struct UpdateScheduler {
    pub schedule: Vec<Task>,
}

impl UpdateScheduler {
    pub fn new(blocks_cnt: usize) -> UpdateScheduler {
        let mut schedule = Vec::with_capacity(blocks_cnt);

        let now = Instant::now();
        for id in 0..blocks_cnt {
            schedule.push(Task {
                id,
                update_time: now,
            });
        }

        UpdateScheduler { schedule }
    }

    pub fn time_to_next_update(&self) -> Option<Duration> {
        let now = Instant::now();
        let mut dur: Option<Duration> = None;
        for task in &self.schedule {
            if task.update_time <= now {
                return Some(Duration::ZERO);
            }
            if let Some(dur) = &mut dur {
                *dur = (*dur).min(task.update_time - now);
            } else {
                dur = Some(task.update_time - now);
            }
        }
        dur
    }

    pub fn push(&mut self, id: usize, when: Instant) {
        self.schedule.push(Task {
            id,
            update_time: when,
        });
    }

    pub fn pop(&mut self, id: usize) {
        self.schedule.retain(|task| task.id != id);
    }
}
