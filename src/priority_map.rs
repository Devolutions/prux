use std::time::SystemTime;
use std::hash::Hash;
use parking_lot::RwLock;
use hashbrown::HashMap;
use log::info;

pub struct PriorityMap<K: Eq + Hash,V> {
    data: HashMap<K,(V, RwLock<SystemTime>)>,
}

impl<K: Eq + Hash + Clone, V> PriorityMap<K,V> {
    pub fn new(capacity: usize) -> Self {
        PriorityMap {
            data: HashMap::with_capacity(capacity),
        }
    }

    pub fn insert(&mut self, key: K, value: V) {
        if self.data.capacity() == self.data.len() {
            let min = self.data.iter().min_by(|x, y| (x.1).1.read().cmp(&(y.1).1.read())).map(|val| val.0.clone());
            if let Some(old) = min {
                self.data.remove(&old);
                info!("Cache size after prune: {}", self.data.len());
            }
        }

        self.data.insert(key, (value, RwLock::new(SystemTime::now())));
    }

    pub fn get(&self, key: &K) -> Option<&V> {
        let value = self.data.get(key);
        if let Some((pop_value, ref pop_time)) = value {
            *pop_time.write() = SystemTime::now();
            Some(pop_value)
        } else {
            None
        }
    }
}