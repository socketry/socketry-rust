use socketry_concurrent::{Scheduler, wait};
use std::cell::RefCell;
use std::future::poll_fn;
use std::rc::Rc;
use std::task::{Poll, Waker};

#[derive(Default)]
struct Message {
    value: Option<String>,
    reader: Option<Waker>,
}

// An ordinary synchronous function can wait for a future at any call depth.
fn read_message(message: &RefCell<Message>) -> String {
    wait(poll_fn(|context| {
        let mut message = message.borrow_mut();
        if let Some(value) = message.value.take() {
            Poll::Ready(value)
        } else {
            message.reader = Some(context.waker().clone());
            Poll::Pending
        }
    }))
}

fn process_message(message: &RefCell<Message>) {
    println!("consumer: waiting");
    let value = read_message(message);
    println!("consumer: received {value}");
}

fn main() -> std::io::Result<()> {
    let mut scheduler = Scheduler::new(256 * 1024);
    let message = Rc::new(RefCell::new(Message::default()));

    let consumer_message = Rc::clone(&message);
    scheduler.spawn(async move {
        process_message(&consumer_message);
    })?;

    scheduler.spawn(async move {
        println!("producer: sending");
        let reader = {
            let mut message = message.borrow_mut();
            message.value = Some(String::from("hello"));
            message.reader.take()
        };
        if let Some(reader) = reader {
            reader.wake();
        }
    })?;

    scheduler.run();
    Ok(())
}
