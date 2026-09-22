use std::{
    collections::BTreeMap,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};
#[derive(Default)]
pub struct WaveRequests {
    entries: BTreeMap<String, (String, Arc<AtomicBool>, bool)>,
}
impl WaveRequests {
    pub fn begin(&mut self, id: String, capture: String) -> Result<(), String> {
        if id.is_empty() || id.len() > 128 || id.contains('\0') {
            return Err("Invalid wave request identity".into());
        }
        if self.entries.contains_key(&id) {
            return Err("Wave request identity is already active".into());
        }
        if self.entries.len() >= 64 {
            return Err("Too many active wave requests".into());
        }
        self.entries
            .insert(id, (capture, Arc::new(AtomicBool::new(false)), false));
        Ok(())
    }
    pub fn claim(&mut self, id: &str, capture: &str) -> Result<Arc<AtomicBool>, String> {
        let (owner, flag, claimed) = self
            .entries
            .get_mut(id)
            .ok_or("Wave request is no longer active")?;
        if owner != capture {
            return Err("Wave request capture mismatch".into());
        }
        if *claimed {
            return Err("Wave request is already running".into());
        }
        *claimed = true;
        Ok(flag.clone())
    }
    pub fn cancel(&mut self, id: &str) {
        if let Some((_, flag, _)) = self.entries.remove(id) {
            flag.store(true, Ordering::Relaxed);
        }
    }
    pub fn cancel_capture(&mut self, capture: &str) {
        self.entries.retain(|_, (owner, flag, _)| {
            if owner == capture {
                flag.store(true, Ordering::Relaxed);
                false
            } else {
                true
            }
        });
    }
    pub fn cancel_all(&mut self) {
        for (_, flag, _) in self.entries.values() {
            flag.store(true, Ordering::Relaxed);
        }
        self.entries.clear();
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cancellation_is_scoped_and_survives_removal_from_registry() {
        let mut requests = WaveRequests::default();
        requests.begin("a".into(), "capture-a".into()).unwrap();
        requests.begin("b".into(), "capture-b".into()).unwrap();
        let a = requests.claim("a", "capture-a").unwrap();
        let b = requests.claim("b", "capture-b").unwrap();
        assert!(requests.claim("a", "capture-b").is_err());
        assert!(requests.claim("a", "capture-a").is_err());
        requests.cancel("unknown");
        assert!(!a.load(Ordering::Relaxed));
        requests.cancel_capture("capture-a");
        assert!(a.load(Ordering::Relaxed));
        assert!(!b.load(Ordering::Relaxed));
        assert!(requests.claim("a", "capture-a").is_err());
        requests.cancel_all();
        assert!(b.load(Ordering::Relaxed));
    }
    #[test]
    fn request_capacity_is_bounded_and_reusable_after_cleanup() {
        let mut requests = WaveRequests::default();
        for i in 0..64 {
            requests.begin(i.to_string(), "capture".into()).unwrap();
        }
        assert!(requests.begin("0".into(), "other".into()).is_err());
        assert!(requests.begin("65".into(), "capture".into()).is_err());
        requests.cancel("0");
        requests.begin("65".into(), "capture".into()).unwrap();
        requests.cancel_all();
        requests.begin("66".into(), "capture".into()).unwrap();
    }
}
