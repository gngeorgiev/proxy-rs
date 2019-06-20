pub mod tracer {
    use std::cell::RefCell;
    use std::rc::Rc;
    use std::time::{Duration, Instant};
    use std::collections::BTreeMap;

    use log::trace;

    pub fn new<T: Into<String>>(id: T) -> Tracer {
        Tracer {
            id: id.into(),
            traces: BTreeMap::new(),
            at_least: None,
            started_at: Instant::now(),
            ignore_log: false,
        }
    }

    #[derive(Debug)]
    pub struct Tracer {
        id: String,
        traces: BTreeMap<String, Vec<Trace>>,
        at_least: Option<Duration>,
        started_at: Instant,
        ignore_log: bool,
    }

    impl Tracer {
        pub fn ignore(mut self, ignore: bool) -> Self {
            self.ignore_log = ignore;
            self
        }

        pub fn at_least_duration(mut self, duration: Duration) -> Self {
            self.at_least = Some(duration);
            self
        }

        pub fn trace<T: Into<String>>(&self, id: T) -> Trace {
            Trace {
                id: id.into(),
                started_at: Instant::now(),
                duration: None,
            }
        }

        pub fn done(&mut self, mut trace: Trace) {
            trace.duration = Some(trace.started_at.elapsed());
            self.traces.entry(trace.id.clone())
                .or_insert_with(|| Vec::new())
                .push(trace);
//            dbg!(self.traces.get(&trace.id.clone()));
        }

        pub fn print(&mut self, mark: bool) {
            if self.ignore_log {
                return;
            }

            if mark {
                self.ignore_log = true;
            }

            let duration = self.started_at.elapsed();
            if self.at_least.is_some() && duration < self.at_least.unwrap() {
                return;
            }

            let mut out = String::with_capacity(1000);
            out.push_str(&format!("Tracer > {} -> total: {:?}\n", self.id, duration));
            for (id, traces) in self.traces.iter() {
                out.push_str(&format!("\t > Trace > {} > called {} times\n", id, traces.len()));
                for t in traces {
                    out.push_str(&format!("\t\t > {} > {:?}\n", t.id, t.duration.unwrap()));
                }
            }

            trace!("{}", out);
        }
    }

    impl Drop for Tracer {
        fn drop(&mut self) {
            self.print(true);
        }
    }

    #[derive(Debug)]
    pub struct Trace {
        id: String,
        started_at: Instant,
        duration: Option<Duration>,
    }
}

pub(crate) struct Elapsed {
    started: std::time::Instant,
    identifier: &'static str,
}

impl Elapsed {
    pub(crate) fn new(identifier: &'static str) -> Elapsed {
        let started = std::time::Instant::now();
        Elapsed {
            started,
            identifier,
        }
    }
}

impl Drop for Elapsed {
    fn drop(&mut self) {
        trace!(
            "ELAPSED: {} -> {:?}",
            self.identifier,
            self.started.elapsed()
        );
    }
}

#[macro_export]
macro_rules! poll_future_01_in_03 {
    ($e:expr) => {
        match $e {
            Ok(futures01::Async::Ready(b)) => b,
            Ok(futures01::Async::NotReady) => return Poll::Pending,
            Err(err) => return Poll::Ready(Err(err)),
        }
    };
}
