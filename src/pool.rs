use crossbeam_channel::{unbounded, Sender, Receiver};
use std::thread;
use std::thread::JoinHandle;

extern crate core_affinity;
use core_affinity::CoreId;
use crate::config::Config;

pub type Runnable = Box<dyn FnOnce() + Send + 'static>;

enum Job {
    Task(Runnable),
    Stop,
}

pub struct ThreadPool {
    sender: Sender<Job>,
    workers: Vec<Worker>,
}

impl ThreadPool {
    pub fn for_config(config: &Config) -> ThreadPool {
        ThreadPool::new(config.scheduler.total_threads_required())
    }

    pub fn new(size: usize) -> ThreadPool {
        let (sender, receiver) = unbounded();
        let mut workers = Vec::with_capacity(size);

        let core_ids = core_affinity::get_core_ids().unwrap();
        for idx in 0..size {
            let core_id = if idx < core_ids.len() {
                Some(core_ids.get(idx).unwrap().to_owned())
            } else {
                None
            };
            let worker = Worker::new(receiver.clone(), core_id);
            workers.push(worker);
        }

        ThreadPool {
            sender,
            workers,
        }
    }

    pub fn submit<F>(&self, f: F)
        where
            F: FnOnce() + Send + 'static
    {
        let job = Job::Task(Box::new(f));
        self.sender.send(job).unwrap();
    }

    pub fn size(&self) -> usize {
        self.workers.len()
    }

    pub fn link<'a>(&self) -> impl Fn(Runnable) + 'a {
        let sender = self.sender.clone();
        move |r: Runnable| {
            sender.send(Job::Task(r)).unwrap();
        }
    }
}

impl Drop for ThreadPool {
    fn drop(&mut self) {
        for _ in 0..self.workers.len() {
            self.sender.send(Job::Stop).unwrap();
        }

        for worker in &mut self.workers {
            if let Some(thread) = worker.thread.take() {
                thread.join().unwrap();
            }
        }
    }
}

struct Worker {
    thread: Option<JoinHandle<()>>,
}

impl Worker {
    fn new(receiver: Receiver<Job>, core_id: Option<CoreId>) -> Worker {
        let thread = thread::spawn(move || {
            if let Some(id) = core_id {
                core_affinity::set_for_current(id);
            }
            loop {
                let job = receiver.recv();
                match job {
                    Ok(Job::Task(f)) => f(),
                    Ok(Job::Stop) => break,
                    Err(_) => break
                }
            }
        });

        Worker {
            thread: Some(thread),
        }
    }
}
