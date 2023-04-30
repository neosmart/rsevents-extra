// rsevents_extra re-exports rsevents::Awaitable
use rsevents_extra::{Awaitable, CountdownEvent};
use std::{thread, time::Duration};

fn main() {
    let countdown = CountdownEvent::new(42);

    thread::scope(|scope| {
        // Start two worker threads to each do some of the work
        for i in 0..2 {
            // Shadow some variables to allow us to `move` into the closure
            let i = i;
            let countdown = &countdown;

            scope.spawn(move || {
                println!("Worker thread reporting for duty!");

                // Worker thread will pretend to do some work
                for i in (500 * i)..(280 * (i + 1)) {
                    if i % 7 == 3 {
                        countdown.tick();
                    }

                    thread::sleep(Duration::from_millis(18));
                }
            });
        }

        // The main thread will wait for 42 tasks to be completed before it does
        // its thing... whatever that is.
        while !countdown.wait_for(Duration::from_secs(1)) {
            // Report progress every 1 second until we've finished
            eprintln!("Work in progress. {} items remaining.", countdown.count());
        }

        eprintln!("Work completed!");
    });
}
