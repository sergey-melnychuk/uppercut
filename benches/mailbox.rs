#[macro_use]
extern crate bencher;
use bencher::Bencher;
use std::collections::{HashMap, VecDeque};

const ENTITIES: usize = 1024;
const CAPACITY: usize = 128;
const PREFIX: usize = 16;

fn vec(b: &mut Bencher) {
    let mut map: HashMap<usize, Vec<usize>> = HashMap::new();
    for id in 0..ENTITIES {
        map.insert(id, Vec::with_capacity(CAPACITY));
    }
    b.iter(|| {
        map.insert(0, Vec::with_capacity(CAPACITY));
        let vec = map.get_mut(&0).unwrap();
        for x in 0..CAPACITY {
            vec.push(x);
        }

        let mut queue = map.remove(&0).unwrap();
        let remaining = queue.split_off(PREFIX);
        map.insert(0, remaining);

        assert_eq!(
            queue,
            vec![0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15]
        );
        assert_eq!(map.remove(&0).unwrap().len(), CAPACITY - PREFIX);
    });
}

fn deq(b: &mut Bencher) {
    let mut map: HashMap<usize, VecDeque<usize>> = HashMap::new();
    for id in 0..ENTITIES {
        map.insert(id, VecDeque::with_capacity(CAPACITY));
    }
    b.iter(|| {
        map.insert(0, VecDeque::with_capacity(CAPACITY));
        let deq = map.get_mut(&0).unwrap();
        for x in 0..CAPACITY {
            deq.push_back(x);
        }

        let selected = deq.drain(..PREFIX).collect::<Vec<_>>();
        assert_eq!(
            selected,
            vec![0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15]
        );
        assert_eq!(map.remove(&0).unwrap().len(), CAPACITY - PREFIX);
    });
}

benchmark_group!(mailbox, vec, deq);
benchmark_main!(mailbox);
