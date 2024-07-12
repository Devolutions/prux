use log::debug;
use parking_lot::Mutex;
use priority_queue::PriorityQueue;
use std::{
    cmp::Reverse,
    collections::{hash_map::RandomState, HashMap},
    fmt::{Debug, Formatter},
    hash::Hash,
    time::{Duration, SystemTime},
};

pub struct PriorityMap<K: Eq + Hash + Clone + Debug, V: Debug> {
    data: HashMap<K, V>,
    priority: Mutex<PriorityQueue<K, Reverse<SystemTime>, RandomState>>,
    prune_after: Duration,
    prune_check_interval: Duration,
    last_prune: SystemTime,
    capacity: usize,
}

impl<K: Eq + Hash + Clone + Debug, V: Debug> Debug for PriorityMap<K, V> {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        self.data.fmt(f)
    }
}

impl<K: Eq + Hash + Clone + Debug, V: Debug> PriorityMap<K, V> {
    pub fn new(capacity: usize, prune_after: Duration, prune_check_interval: Duration) -> Self {
        PriorityMap {
            data: HashMap::with_capacity(capacity),
            priority: Mutex::new(PriorityQueue::with_capacity(capacity)),
            prune_after,
            prune_check_interval,
            last_prune: SystemTime::now(),
            capacity,
        }
    }

    pub fn insert(&mut self, key: K, value: V) -> Option<V> {
        self.check_prune();
        if self.data.len() >= self.capacity {
            self.prune_last();
        }

        self.priority
            .lock()
            .push(key.clone(), Reverse(SystemTime::now()));
        self.data.insert(key, value)
    }

    #[allow(unused)]
    pub fn get(&self, key: &K) -> Option<&V> {
        if let Some(value) = self.data.get(key) {
            self.priority
                .lock()
                .push(key.clone(), Reverse(SystemTime::now()));
            Some(value)
        } else {
            None
        }
    }

    #[allow(unused)]
    pub fn get_mut(&mut self, key: &K) -> Option<&mut V> {
        self.check_prune();
        if let Some(value) = self.data.get_mut(key) {
            self.priority
                .lock()
                .push(key.clone(), Reverse(SystemTime::now()));
            Some(value)
        } else {
            None
        }
    }

    #[allow(unused)]
    pub fn capacity(&self) -> usize {
        self.capacity
    }

    #[allow(unused)]
    pub fn len(&self) -> usize {
        self.data.len()
    }

    #[allow(unused)]
    pub fn contains_key(&self, key: &K) -> bool {
        self.data.contains_key(key)
    }

    pub fn check_prune(&mut self) {
        let elapsed = self.last_prune.elapsed();
        if elapsed.is_err() || elapsed.expect("unreachable") >= self.prune_check_interval {
            let mut priority = self.priority.lock();
            while let Some((key, Reverse(updated_at))) = priority.peek() {
                let elapsed = updated_at.elapsed();
                if elapsed.is_err() || elapsed.expect("unreachable") < self.prune_after {
                    break;
                }
                self.data.remove(key);
                priority.pop();
            }

            debug!("Cache size after prune: {}", self.data.len());
            self.last_prune = SystemTime::now();
        }
    }

    fn prune_last(&mut self) {
        if let Some((key, _)) = self.priority.lock().pop() {
            self.data.remove(&key);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::PriorityMap;
    use std::time::Duration;

    #[test]
    pub fn test_capacity() {
        let mut map = PriorityMap::new(5, Duration::from_secs(5), Duration::from_secs(5));
        for i in 0..6 {
            map.insert(i, i);
        }
        assert_eq!(map.len(), 5);
    }

    #[test]
    pub fn test_prune_order() {
        let mut map = PriorityMap::new(5, Duration::from_secs(5), Duration::from_secs(5));
        for i in 0..6 {
            map.insert(i, i);
        }
        assert_eq!(map.get_mut(&0), None);
        map.insert(1, 1);
        map.insert(6, 6);
        assert_eq!(map.get_mut(&1), Some(&mut 1));
        assert_eq!(map.get_mut(&2), None);
    }

    #[test]
    pub fn test_prune_expired() {
        use std::thread::sleep;

        let mut map = PriorityMap::new(4, Duration::from_secs(2), Duration::from_secs(2));
        for i in 0..3 {
            map.insert(i, i);
        }
        sleep(Duration::from_secs(1));
        map.insert(3, 3);
        assert_eq!(map.len(), 4);
        sleep(Duration::from_secs(1));
        map.insert(4, 4);
        assert_eq!(map.len(), 2);
    }
}
