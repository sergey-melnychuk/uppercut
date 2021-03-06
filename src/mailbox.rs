use crate::api::Envelope;

pub(crate) struct Mailbox {
    throughput: usize,
    queue: Vec<Envelope>,
}

impl Mailbox {
    pub(crate) fn new(throughput: usize, capacity: usize) -> Self {
        Mailbox {
            throughput,
            queue: Vec::with_capacity(capacity),
        }
    }

    pub(crate) fn get(&mut self) -> Vec<Envelope> {
        if self.queue.len() < self.throughput {
            self.queue.drain(..).collect()
        } else {
            let remaining = self.queue.split_off(self.throughput);
            let returned = self.queue.drain(..).collect();
            self.queue = remaining;
            returned
        }
    }

    pub(crate) fn put(&mut self, mut queue: Vec<Envelope>) {
        self.queue.append(&mut queue);
    }

    pub(crate) fn len(&self) -> usize {
        self.queue.len()
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const THROUGHPUT: usize = 4;
    const CAPACITY: usize = 16;

    fn make_envelope() -> Envelope {
        Envelope::of(42i32).to("Bob").from("Alice")
    }

    #[test]
    fn test_put_and_get() {
        const N: usize = 10;
        let mut mbox = Mailbox::new(THROUGHPUT, CAPACITY);
        let queue = (0..N).map(|_| make_envelope()).collect::<Vec<_>>();

        mbox.put(queue);
        assert_eq!(N, mbox.len());

        let taken = mbox.get();
        assert_eq!(THROUGHPUT, taken.len());
        assert_eq!(N - THROUGHPUT * 1, mbox.len());

        let taken = mbox.get();
        assert_eq!(THROUGHPUT, taken.len());
        assert_eq!(N - THROUGHPUT * 2, mbox.len());

        let taken = mbox.get();
        assert_eq!(2, taken.len());
        assert_eq!(0, mbox.len());
    }
}
