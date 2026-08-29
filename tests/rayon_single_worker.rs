/*
Copyright 2021 Google LLC

Licensed under the Apache License, Version 2.0 (the "License");
you may not use this file except in compliance with the License.
You may obtain a copy of the License at

     https://www.apache.org/licenses/LICENSE-2.0

Unless required by applicable law or agreed to in writing, software
distributed under the License is distributed on an "AS IS" BASIS,
WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
See the License for the specific language governing permissions and
limitations under the License.
*/

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::time::Duration;

#[test]
fn blocking_consumer_starves_scoped_producer_with_one_worker() {
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(1)
        .build()
        .unwrap();
    let consumer_timed_out = AtomicBool::new(false);

    pool.install(|| {
        rayon::scope(|scope| {
            let (sender, receiver) = mpsc::channel();

            scope.spawn(move |_| {
                // Ignore disconnection: the receiver deliberately gives up after
                // the timeout so this characterization test cannot deadlock.
                let _ = sender.send(());
            });
            let consumer_timed_out = &consumer_timed_out;
            scope.spawn(move |_| {
                if receiver.recv_timeout(Duration::from_millis(250))
                    == Err(mpsc::RecvTimeoutError::Timeout)
                {
                    consumer_timed_out.store(true, Ordering::SeqCst);
                }
            });
        });
    });

    assert!(
        consumer_timed_out.load(Ordering::SeqCst),
        "the blocking consumer unexpectedly allowed the queued producer to run"
    );
}
