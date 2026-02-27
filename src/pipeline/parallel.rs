use crate::registry::FingerprintRegistry;
use serde_json::Value;
use std::collections::BTreeMap;
use std::sync::mpsc;
use std::thread;

/// Process records in parallel with bounded reorder buffer, emitting in input order.
pub fn process_parallel(
    records: Vec<Value>,
    _registry: &FingerprintRegistry,
    jobs: usize,
) -> Vec<Value> {
    process_parallel_with(records, jobs, |record| record)
}

fn process_parallel_with<F>(records: Vec<Value>, jobs: usize, process: F) -> Vec<Value>
where
    F: Fn(Value) -> Value + Sync,
{
    let worker_count = jobs.max(1);
    let in_flight_limit = worker_count.saturating_mul(2).max(1);
    let mut ordered = Vec::with_capacity(records.len());
    let mut indexed = records.into_iter().enumerate();

    loop {
        let Some(first) = indexed.next() else {
            break;
        };

        let mut batch = Vec::with_capacity(in_flight_limit);
        batch.push(first);
        for _ in 1..in_flight_limit {
            if let Some(next) = indexed.next() {
                batch.push(next);
            } else {
                break;
            }
        }

        if worker_count == 1 {
            for (_index, record) in batch {
                ordered.push(process(record));
            }
            continue;
        }

        let (result_tx, result_rx) = mpsc::channel::<(usize, Value)>();
        thread::scope(|scope| {
            for (index, record) in batch {
                let result_tx = result_tx.clone();
                let process = &process;
                scope.spawn(move || {
                    let processed = process(record);
                    let _ = result_tx.send((index, processed));
                });
            }
        });
        drop(result_tx);

        let mut pending = BTreeMap::new();
        for (index, record) in result_rx {
            pending.insert(index, record);
        }
        for (_, record) in pending {
            ordered.push(record);
        }
    }

    ordered
}

#[cfg(test)]
mod tests {
    use super::{process_parallel, process_parallel_with};
    use crate::registry::FingerprintRegistry;
    use serde_json::{Value, json};
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };
    use std::thread;
    use std::time::Duration;

    fn sample_records(count: usize) -> Vec<Value> {
        (0..count).map(|seq| json!({ "seq": seq })).collect()
    }

    #[test]
    fn preserves_order_with_single_worker() {
        let registry = FingerprintRegistry::new();
        let records = sample_records(8);

        let output = process_parallel(records, &registry, 1);
        let sequence: Vec<u64> = output
            .iter()
            .map(|record| record["seq"].as_u64().expect("u64 seq"))
            .collect();

        assert_eq!(sequence, (0..8).collect::<Vec<_>>());
    }

    #[test]
    fn preserves_order_with_parallel_workers() {
        let records = sample_records(24);
        let output = process_parallel_with(records, 4, |record| {
            let seq = record["seq"].as_u64().expect("u64 seq");
            if seq % 2 == 0 {
                thread::sleep(Duration::from_millis(4));
            } else {
                thread::sleep(Duration::from_millis(1));
            }
            record
        });

        let sequence: Vec<u64> = output
            .iter()
            .map(|record| record["seq"].as_u64().expect("u64 seq"))
            .collect();
        assert_eq!(sequence, (0..24).collect::<Vec<_>>());
    }

    #[test]
    fn bounds_in_flight_work_to_two_times_jobs() {
        let jobs = 3usize;
        let records = sample_records(30);
        let current = Arc::new(AtomicUsize::new(0));
        let observed_max = Arc::new(AtomicUsize::new(0));

        let current_for_closure = Arc::clone(&current);
        let observed_max_for_closure = Arc::clone(&observed_max);

        let _ = process_parallel_with(records, jobs, move |record| {
            let active = current_for_closure.fetch_add(1, Ordering::SeqCst) + 1;
            observed_max_for_closure.fetch_max(active, Ordering::SeqCst);
            thread::sleep(Duration::from_millis(2));
            current_for_closure.fetch_sub(1, Ordering::SeqCst);
            record
        });

        assert!(observed_max.load(Ordering::SeqCst) <= jobs * 2);
    }

    #[test]
    fn handles_empty_input() {
        let registry = FingerprintRegistry::new();
        let output = process_parallel(Vec::new(), &registry, 4);
        assert!(output.is_empty());
    }
}
