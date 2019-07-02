use std::collections::LinkedList;
use hashbrown::HashMap;
use std::time::SystemTime;
use std::hash::Hash;

pub struct PriorityMap<K: Eq + Hash,V> {
    data: HashMap<K,(V, SystemTime)>,
}

impl<K: Eq + Hash + Clone, V> PriorityMap<K,V> {
    pub fn new() -> Self {
        PriorityMap {
            data: HashMap::new(),
        }
    }

    pub fn with_capacity(capacity: usize) -> Self {
        PriorityMap {
            data: HashMap::with_capacity(capacity),
        }
    }

    pub fn insert(&mut self, key: K, value: V) {
        if self.data.capacity() == self.data.len() {
            let min = self.data.iter().min_by(|x, y| (x.1).1.cmp(&(y.1).1)).map(|val| val.0.clone());
            if let Some(old) = min {
                self.data.remove(&old);
            }
        }

        self.data.insert(key, (value, SystemTime::now()));
    }

    pub fn get(&self, key: &K) -> Option<&V> {
        let value = self.data.get(key);
        if let Some((pop_value, mut pop_time)) = value {
            pop_time = SystemTime::now();
            Some(pop_value)
        } else {
            None
        }
    }
}